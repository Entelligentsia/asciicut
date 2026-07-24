//! `asciicut` — the native CLI entry point.
//!
//! A minimal `std::env::args` dispatcher over the [`asciicut`] library. It ships
//! the `compose <project.json>` subcommand — which reads a `.asciicut.json` edit
//! project plus its source `.cast` and writes the composed `.cast` to stdout —
//! and a bare `<file.cast>` argument that launches the local `asciicut-server`
//! HTTP surface + embedded SPA (SPEC §7.2). The dispatch is shaped so SPEC
//! §8.1's future subcommands (`probe`, `frame`, `render`, `suggest`) slot in
//! cleanly; adopting a parser dependency such as `clap` is deferred to the
//! first multi-flag subcommand.
//!
//! Exit-code contract (SPEC §8): `0` success (document to stdout), `1`
//! I/O / parse error (diagnostic to stderr), `2` argument misuse (usage to
//! stderr). Nothing but the composed document is ever written to stdout.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use asciicut::compose_project;

/// Default loopback port for the local server. The actual bound URL is printed
/// to stdout at launch, so an in-use port surfaces a clear error rather than
/// silently rebinding.
const DEFAULT_PORT: u16 = 8777;

const USAGE: &str = "\
asciicut — compose and preview asciinema recordings

USAGE:
    asciicut compose <project.asciicut.json>   compose an edit project to stdout
    asciicut <file.cast>                      serve the local preview UI

The composed .cast document is written to stdout. Exit codes:
    0  success
    1  I/O or parse error (diagnostic on stderr)
    2  argument misuse (this usage on stderr)";

fn main() -> ExitCode {
    // args[0] is the binary name; skip it.
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("compose") => match args.get(1) {
            Some(project_path) if args.len() == 2 => run_compose(project_path),
            _ => usage_error("compose takes exactly one argument: <project.asciicut.json>"),
        },
        // A bare single `*.cast` argument launches the local server (SPEC §7.2).
        // The `.cast` suffix keeps a non-cast single token (e.g. a typo'd verb)
        // routing to the unknown-subcommand diagnostic rather than a failed read.
        Some(cast_path) if args.len() == 1 && cast_path.ends_with(".cast") => run_serve(cast_path),
        Some(other) => usage_error(&format!("unknown subcommand '{other}'")),
        None => usage_error("no subcommand given"),
    }
}

/// Launch the local server for `cast_path` on a fresh Tokio runtime, blocking
/// until it exits. Returns the process exit code.
fn run_serve(cast_path: &str) -> ExitCode {
    // Build the async runtime here (not in the wasm-safe library): the server's
    // `serve` future needs a reactor, and `main` is the native-only shell.
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("asciicut: cannot start async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(asciicut_server::serve(
        PathBuf::from(cast_path),
        DEFAULT_PORT,
    )) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("asciicut: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Compose `project_path` and stream the result to stdout, or print a diagnostic
/// to stderr. Returns the process exit code.
fn run_compose(project_path: &str) -> ExitCode {
    match compose_project(project_path) {
        Ok(document) => {
            // Write the document as-is (its own trailing newline comes from the
            // serializer). A broken pipe — e.g. `asciicut compose … | head` —
            // is a normal, quiet exit, not a panic-with-backtrace.
            match std::io::stdout().write_all(document.as_bytes()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("asciicut: write error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Err(e) => {
            eprintln!("asciicut: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Print the usage banner plus a one-line reason to stderr and return exit code
/// `2` (argument misuse).
fn usage_error(reason: &str) -> ExitCode {
    eprintln!("asciicut: {reason}\n\n{USAGE}");
    ExitCode::from(2)
}
