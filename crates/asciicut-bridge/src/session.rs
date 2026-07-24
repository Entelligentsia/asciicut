//! [`Session`] — the in-process parsed-cast-plus-path state every [`crate::ops`]
//! function operates on.
//!
//! Mirrors what `asciicut-server`'s `AppState` holds (a parsed source [`Cast`]
//! plus its on-disk path), but the cast is **optional**: the axum server
//! always launches with a cast already resolved from its CLI argument
//! (`AppState::new`/`AppState::load` both construct with one present), while
//! the desktop shell may start with none until a cast is opened (the initial
//! `<file.cast>` launch argument, or a future native Open dialog).

use std::path::{Path, PathBuf};

use asciicut_core::Cast;

use crate::error::BridgeError;

/// The parsed source cast plus the on-disk path it was loaded from.
#[derive(Debug)]
struct Loaded {
    cast: Cast,
    cast_path: PathBuf,
}

/// Derive a sibling of `cast_path` that keeps its stem and takes `suffix`:
/// `<cast_dir>/<cast_stem><suffix>`.
///
/// The single definition of the "derived from the loaded cast path, never from
/// caller input" rule that guards every write target in the app — the project
/// save path ([`Session::save_path`]), the server's `/api/export` composed cast
/// (`asciicut_server::AppState::export_cast_path`), and the desktop export
/// pipeline's own composed cast (`asciicut_desktop_lib::export::export_cast_path`).
/// Those three used to carry their own copy of this derivation; a drift between
/// them would silently split the desktop and server export targets apart.
///
/// A path with no usable stem falls back to `project`, and a path with no parent
/// to the current directory, so the result is always a writable sibling.
#[must_use]
pub fn sibling_path(cast_path: &Path, suffix: &str) -> PathBuf {
    let dir = cast_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = cast_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    dir.join(format!("{stem}{suffix}"))
}

/// Suffix of the project document derived from a cast path (`sample.cast` →
/// `sample.asciicut.json`).
pub const PROJECT_SUFFIX: &str = ".asciicut.json";

/// Suffix of the composed cast derived from a cast path (`sample.cast` →
/// `sample.composed.cast`).
pub const COMPOSED_CAST_SUFFIX: &str = ".composed.cast";

/// In-process session state: at most one loaded source cast plus an
/// optional explicit project save path (set by native Save As / Open).
#[derive(Debug, Default)]
pub struct Session {
    loaded: Option<Loaded>,
    project_path: Option<PathBuf>,
}

impl Session {
    /// An empty session — no cast loaded yet. The desktop shell's starting
    /// point when launched with no positional argument.
    #[must_use]
    pub fn new() -> Self {
        Session {
            loaded: None,
            project_path: None,
        }
    }

    /// Build a session directly from an already-parsed cast and its path
    /// (server `AppState::new` / test / in-process construction; also what the
    /// desktop shell uses once its `<file.cast>` launch argument is parsed).
    #[must_use]
    pub fn with_cast(cast: Cast, cast_path: PathBuf) -> Self {
        Session {
            loaded: Some(Loaded { cast, cast_path }),
            project_path: None,
        }
    }

    /// Read and parse the `.cast` at `cast_path`, replacing any previously
    /// loaded cast and clearing any explicit project path (the new cast has
    /// its own sibling project rule until the user chooses Save As again).
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::ReadCast`] if the file is unreadable and
    /// [`BridgeError::ParseCast`] if it fails to parse. On either error the
    /// previously loaded cast (if any) is left untouched.
    pub fn load(&mut self, cast_path: PathBuf) -> Result<(), BridgeError> {
        let text = std::fs::read_to_string(&cast_path).map_err(|source| BridgeError::ReadCast {
            path: cast_path.clone(),
            source,
        })?;
        let cast = Cast::parse(&text).map_err(BridgeError::ParseCast)?;
        self.loaded = Some(Loaded { cast, cast_path });
        self.project_path = None;
        Ok(())
    }

    /// The loaded source cast.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::NoCastLoaded`] if nothing has been loaded yet.
    pub fn cast(&self) -> Result<&Cast, BridgeError> {
        self.loaded
            .as_ref()
            .map(|l| &l.cast)
            .ok_or(BridgeError::NoCastLoaded)
    }

    /// The loaded cast's on-disk path.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::NoCastLoaded`] if nothing has been loaded yet.
    pub fn cast_path(&self) -> Result<&Path, BridgeError> {
        self.loaded
            .as_ref()
            .map(|l| l.cast_path.as_path())
            .ok_or(BridgeError::NoCastLoaded)
    }

    /// Whether a cast is currently loaded.
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }

    /// The save target. If an explicit project path has been established by
    /// Save As or Open, return it; otherwise derive it purely from the loaded
    /// cast path so `save`/`project` cannot be steered at an arbitrary
    /// filesystem location: `<cast_dir>/<cast_stem>.asciicut.json`.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::NoCastLoaded`] if nothing has been loaded yet.
    pub fn save_path(&self) -> Result<PathBuf, BridgeError> {
        if let Some(ref path) = self.project_path {
            return Ok(path.clone());
        }
        Ok(sibling_path(self.cast_path()?, PROJECT_SUFFIX))
    }

    /// Set (or replace) the explicit project save path. Used by native Save As
    /// and Open to establish the active save location.
    pub fn set_project_path(&mut self, path: PathBuf) {
        self.project_path = Some(path);
    }

    /// The loaded cast's **file name** (e.g. `sample.cast`) — the
    /// authoritative project `source`. Falls back to the whole path string if
    /// the OS path has no final component (never expected for a real launch
    /// cast).
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::NoCastLoaded`] if nothing has been loaded yet.
    pub fn source_name(&self) -> Result<String, BridgeError> {
        let cast_path = self.cast_path()?;
        Ok(cast_path
            .file_name()
            .and_then(|s| s.to_str())
            .map_or_else(|| cast_path.display().to_string(), str::to_owned))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_path_keeps_the_stem_and_directory() {
        assert_eq!(
            sibling_path(Path::new("/recordings/demo.cast"), PROJECT_SUFFIX),
            PathBuf::from("/recordings/demo.asciicut.json")
        );
        assert_eq!(
            sibling_path(Path::new("/recordings/demo.cast"), COMPOSED_CAST_SUFFIX),
            PathBuf::from("/recordings/demo.composed.cast")
        );
    }

    #[test]
    fn sibling_path_only_replaces_the_final_extension() {
        // A stem that itself contains ".cast" must survive intact — the reason
        // this is a stem-based derivation and not a substring replacement.
        assert_eq!(
            sibling_path(Path::new("my.cast.demo.cast"), PROJECT_SUFFIX),
            PathBuf::from("my.cast.demo.asciicut.json")
        );
    }

    #[test]
    fn sibling_path_falls_back_for_stemless_and_parentless_paths() {
        assert_eq!(
            sibling_path(Path::new("demo.cast"), PROJECT_SUFFIX),
            PathBuf::from("demo.asciicut.json"),
            "a bare file name yields a bare sibling, not an absolute path"
        );
        assert_eq!(
            sibling_path(Path::new("/"), PROJECT_SUFFIX),
            PathBuf::from("./project.asciicut.json"),
            "a path with neither stem nor parent falls back to ./project"
        );
    }

    #[test]
    fn save_path_prefers_an_explicit_project_path() {
        let mut session = Session::new();
        assert!(
            matches!(session.save_path(), Err(BridgeError::NoCastLoaded)),
            "an empty session has no save target"
        );
        session.set_project_path(PathBuf::from("/elsewhere/chosen.asciicut.json"));
        assert_eq!(
            session.save_path().unwrap(),
            PathBuf::from("/elsewhere/chosen.asciicut.json"),
            "Save As wins over the cast-derived sibling, with no cast loaded at all"
        );
    }
}
