//! Integration tests for the `asciicut compose` CLI.
//!
//! The headline gate (AC#2) pins the CLI's stdout **byte-for-byte** to the
//! core engine's `compose(...).to_cast_string()` — the exact serializer the
//! in-browser preview renders from. This is a CLI↔native-engine pin: it proves
//! the thin shell adds no drift on top of the standing native↔wasm SPEC §7.1
//! parity contract that asciicut-core's own golden tests anchor semantically.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use asciicut_core::{compose, Cast, Project};

/// Absolute path to the freshly built CLI binary (forces it to build — AC#3).
const BIN: &str = env!("CARGO_BIN_EXE_asciicut");

/// Repo root = `<repo>/crates/asciicut` (CARGO_MANIFEST_DIR) popped two levels.
fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/
    p.pop(); // repo root
    p
}

fn read_repo(rel: &str) -> String {
    let path = repo_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Run the CLI with `args` and return `(exit_code, stdout, stderr)`.
fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("spawn asciicut binary");
    (
        out.status
            .code()
            .expect("process exited via signal, not code"),
        String::from_utf8(out.stdout).expect("stdout is utf-8"),
        String::from_utf8(out.stderr).expect("stderr is utf-8"),
    )
}

/// A unique scratch directory under the OS temp dir, removed on `Drop`.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("asciicut-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).expect("create temp dir");
        TempDir(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// AC#2 — the CLI's stdout is byte-identical to the preview serializer.
#[test]
fn cli_stdout_is_byte_identical_to_core_compose() {
    let project_rel = "samples/sample.asciicut.json";
    let (code, stdout, stderr) = run(&["compose", repo_root().join(project_rel).to_str().unwrap()]);
    assert_eq!(code, 0, "expected success; stderr: {stderr}");
    assert!(
        stderr.is_empty(),
        "success must not write to stderr: {stderr}"
    );

    // Same inputs, same code path, straight through the core engine.
    let project = Project::parse(&read_repo(project_rel)).expect("parse project");
    let source = Cast::parse(&read_repo("samples/sample.cast")).expect("parse source");
    let expected = compose(&source, &project).to_cast_string();

    assert_eq!(
        stdout, expected,
        "CLI stdout must equal core compose() byte-for-byte"
    );
}

/// AC#4 — the fixture project round-trips through `Project::parse` with the exact
/// edit state preserved: segment ORDER, per-segment `speed`/`holdEnd`, the global
/// `idleCap`, and the pass-through `markers`/`output` metadata. This is the Rust
/// side of the load→save fidelity contract the in-browser deep-equal mirrors.
#[test]
fn fixture_project_parse_preserves_exact_edit_state() {
    let project =
        Project::parse(&read_repo("samples/sample.asciicut.json")).expect("fixture project parses");

    // Global idle cap survives verbatim.
    assert!((project.idle_cap - 0.4).abs() < 1e-12, "idleCap preserved");

    // Segment order + per-segment controls survive verbatim (array order IS the
    // compose order).
    assert_eq!(project.segments.len(), 4, "all 4 segments preserved");
    let starts: Vec<f64> = project.segments.iter().map(|s| s.src_start).collect();
    assert_eq!(
        starts,
        vec![
            25.0,
            494.6823598244972,
            915.6428524403356,
            965.4433571430968
        ],
        "segment order preserved"
    );
    assert!(
        (project.segments[1].speed - 2.0).abs() < 1e-12,
        "speed preserved"
    );
    assert!(
        (project.segments[3].hold_end - 3.0).abs() < 1e-12,
        "holdEnd preserved"
    );
    assert_eq!(
        project.segments[0].label.as_deref(),
        Some("sprint command fires"),
        "label preserved",
    );

    // Output geometry (folded into the composed header) survives.
    let output = project.output.as_ref().expect("output block preserved");
    assert_eq!(output.width, Some(120), "output width preserved");
    assert_eq!(output.height, Some(40), "output height preserved");

    // Markers (opaque pass-through) survive.
    assert_eq!(project.markers.len(), 1, "marker preserved");
    assert!(
        (project.markers[0].t - 940.0).abs() < 1e-12,
        "marker time preserved"
    );
    assert_eq!(
        project.markers[0].text.as_deref(),
        Some("8-phase transcript"),
        "marker text preserved",
    );
}

/// `project.source` resolves relative to the project file's directory, not the
/// process CWD (the `proj_dir / source` contract ported from the prototype).
#[test]
fn source_resolves_relative_to_project_file() {
    let tmp = TempDir::new("relpath");
    let sub = tmp.path().join("nested");
    fs::create_dir_all(&sub).unwrap();

    // Copy the real source next to a project that references it by bare name.
    // The bare name only resolves if we join it to the project's own directory
    // — the process CWD (the crate dir, when spawned by cargo) has no such file.
    fs::copy(
        repo_root().join("samples/sample.cast"),
        sub.join("sample.cast"),
    )
    .unwrap();
    fs::copy(
        repo_root().join("samples/sample.asciicut.json"),
        sub.join("project.json"),
    )
    .unwrap();

    let (code, stdout, stderr) = run(&["compose", sub.join("project.json").to_str().unwrap()]);
    assert_eq!(code, 0, "expected success; stderr: {stderr}");
    let cast = Cast::parse(&stdout).expect("project-relative source resolved and composed");
    assert_eq!(cast.header.version, 2, "stdout should be a v2 cast");
}

/// The CLI's stdout re-parses as a version-2 cast — guards against double-newline
/// or encoding drift introduced by the shell layer.
#[test]
fn cli_output_round_trips_to_v2() {
    let (code, stdout, _) = run(&[
        "compose",
        repo_root()
            .join("samples/sample.asciicut.json")
            .to_str()
            .unwrap(),
    ]);
    assert_eq!(code, 0);
    let reparsed = Cast::parse(&stdout).expect("CLI stdout must re-parse as a cast");
    assert_eq!(
        reparsed.header.version, 2,
        "serialized output is normalized to v2"
    );
}

/// The bare `<file.cast>` argument is recognized and routed to the local server
/// launcher (SPEC §7.2). We point it at a nonexistent cast so the dispatch is
/// exercised end-to-end **without binding a socket**: `AppState::load` fails at
/// the file read and the process exits `1` with the server's read diagnostic,
/// proving the branch is wired before any `TcpListener::bind`.
#[test]
fn bare_cast_argument_routes_to_server() {
    let (code, stdout, stderr) = run(&["/no/such/recording.cast"]);
    assert_eq!(code, 1, "missing cast → exit 1; stderr: {stderr}");
    assert!(
        stdout.is_empty(),
        "server launch must not write to stdout on error"
    );
    assert!(
        stderr.contains("cannot read cast"),
        "server read diagnostic on stderr: {stderr}"
    );
}

/// The usage banner advertises both the `compose` subcommand and the bare
/// `<file.cast>` serve form.
#[test]
fn usage_lists_serve_form() {
    let (code, _, stderr) = run(&[]);
    assert_eq!(code, 2);
    assert!(
        stderr.contains("<file.cast>"),
        "usage mentions serve form: {stderr}"
    );
    assert!(
        stderr.contains("compose"),
        "usage mentions compose: {stderr}"
    );
}

/// Exit-code + stderr contract table (SPEC §8).
#[test]
fn exit_codes_and_stderr() {
    // No subcommand → misuse (2), usage on stderr, nothing on stdout.
    let (code, stdout, stderr) = run(&[]);
    assert_eq!(code, 2, "no subcommand → exit 2");
    assert!(stdout.is_empty());
    assert!(stderr.contains("USAGE"), "usage banner on stderr: {stderr}");

    // Unknown subcommand → misuse (2).
    let (code, _, stderr) = run(&["frobnicate"]);
    assert_eq!(code, 2, "unknown subcommand → exit 2");
    assert!(stderr.contains("unknown subcommand"));

    // compose with no argument → misuse (2).
    let (code, _, _) = run(&["compose"]);
    assert_eq!(code, 2, "compose without a path → exit 2");

    // compose with too many arguments → misuse (2).
    let (code, _, _) = run(&["compose", "a.json", "b.json"]);
    assert_eq!(code, 2, "compose with extra args → exit 2");

    // Nonexistent project path → I/O error (1).
    let (code, stdout, stderr) = run(&["compose", "/no/such/project.json"]);
    assert_eq!(code, 1, "missing project → exit 1");
    assert!(stdout.is_empty());
    assert!(stderr.contains("cannot read project"), "stderr: {stderr}");

    // Malformed project JSON → parse error (1).
    let tmp = TempDir::new("errs");
    let bad_proj = tmp.path().join("bad.json");
    fs::write(&bad_proj, "{ this is not json").unwrap();
    let (code, _, stderr) = run(&["compose", bad_proj.to_str().unwrap()]);
    assert_eq!(code, 1, "malformed project → exit 1");
    assert!(stderr.contains("invalid project"), "stderr: {stderr}");

    // Malformed source cast → parse error (1). Valid project pointing at garbage.
    let good_proj = tmp.path().join("proj.json");
    fs::write(&good_proj, "{\"source\":\"garbage.cast\",\"segments\":[]}").unwrap();
    fs::write(
        tmp.path().join("garbage.cast"),
        "not a cast header at all\n",
    )
    .unwrap();
    let (code, _, stderr) = run(&["compose", good_proj.to_str().unwrap()]);
    assert_eq!(code, 1, "malformed source → exit 1");
    assert!(stderr.contains("invalid source cast"), "stderr: {stderr}");
}
