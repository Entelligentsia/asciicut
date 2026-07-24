//! `asciicut` CLI library — the pure orchestration behind the `compose`
//! subcommand.
//!
//! This crate is a **thin native-only shell** over [`asciicut_core`]: it owns
//! filesystem I/O (which the wasm-safe core deliberately avoids) and nothing
//! else. The composition itself — parse → project → serialize — runs entirely
//! through the shared engine, so the CLI's output is byte-identical to the
//! in-browser preview, which renders from the same [`asciicut_core::Cast::to_cast_string`]
//! serializer (SPEC §7.1, §7.4).
//!
//! [`compose_project`] performs no stdout writes of its own — it returns the
//! composed document as a `String` (or a structured [`CliError`]) so it can be
//! exercised directly by integration tests without spawning a subprocess.

use std::fmt;
use std::path::{Path, PathBuf};

use asciicut_core::{compose, Cast, ParseError, Project, ProjectError};

/// A structured error for the `compose` pipeline. Each variant maps to the
/// stage that failed and carries enough context for an agent-friendly stderr
/// diagnostic (SPEC §8). All variants map to process exit code `1`; argument
/// misuse (exit `2`) is handled in `main` before the library is invoked.
#[derive(Debug)]
pub enum CliError {
    /// The project `.asciicut.json` could not be read from disk.
    ReadProject {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The project JSON failed to parse into a [`Project`].
    ParseProject(ProjectError),
    /// The source `.cast` (resolved relative to the project file) was unreadable.
    ReadSource {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The source `.cast` failed to parse into a [`Cast`].
    ParseSource(ParseError),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::ReadProject { path, source } => {
                write!(f, "cannot read project '{}': {source}", path.display())
            }
            CliError::ParseProject(e) => write!(f, "invalid project: {e}"),
            CliError::ReadSource { path, source } => {
                write!(f, "cannot read source '{}': {source}", path.display())
            }
            CliError::ParseSource(e) => write!(f, "invalid source cast: {e}"),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CliError::ReadProject { source, .. } | CliError::ReadSource { source, .. } => {
                Some(source)
            }
            CliError::ParseProject(e) => Some(e),
            CliError::ParseSource(e) => Some(e),
        }
    }
}

/// Compose the project at `project_path` and return the composed `.cast`
/// document as a `String`.
///
/// The pipeline mirrors `prototype/compose.py`: read the project file →
/// [`Project::parse`] → resolve `project.source` **relative to the project
/// file's parent directory** (not the process CWD) → read + [`Cast::parse`] the
/// source → [`asciicut_core::compose`] → [`Cast::to_cast_string`]. The returned
/// string is exactly what the in-browser preview serializes, pinning
/// CLI↔preview byte-identity.
///
/// # Errors
///
/// Returns a [`CliError`] if the project or source file is unreadable, or if
/// either fails to parse.
pub fn compose_project(project_path: impl AsRef<Path>) -> Result<String, CliError> {
    let project_path = project_path.as_ref();

    let project_text =
        std::fs::read_to_string(project_path).map_err(|source| CliError::ReadProject {
            path: project_path.to_path_buf(),
            source,
        })?;
    let project = Project::parse(&project_text).map_err(CliError::ParseProject)?;

    // Resolve the source cast relative to the project file's parent directory
    // (`proj_dir / proj["source"]` in the prototype), so `source` stays a bare
    // filename regardless of the process working directory.
    let proj_dir = project_path.parent().unwrap_or_else(|| Path::new("."));
    let source_path = proj_dir.join(&project.source);

    let source_text =
        std::fs::read_to_string(&source_path).map_err(|source| CliError::ReadSource {
            path: source_path.clone(),
            source,
        })?;
    let source_cast = Cast::parse(&source_text).map_err(CliError::ParseSource)?;

    let composed = compose(&source_cast, &project);
    Ok(composed.to_cast_string())
}
