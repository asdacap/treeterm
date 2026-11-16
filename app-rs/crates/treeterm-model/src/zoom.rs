//! View > Actual Size, Zoom In and Zoom Out. Electron's menu roles move Chromium's page zoom
//! level by half a level (`webContents.zoomLevel ± 0.5`, back to 0 for Actual Size), and
//! Chromium draws the page 1.2^level times its size, kept within 25%–500%. The level is not
//! bounded: past a limit the page stays at it, and zooming back takes as many steps. One level
//! holds for every window (Chromium zooms by origin), and Chromium saves it (in `Preferences`,
//! under `partition.per_host_zoom_levels`), so the app starts at the level it quit at.
//!
//! [`ZoomStore`] does the same with `zoom.json` in the user data directory, saved at every
//! change. The file is the native app's own: Chromium rewrites `Preferences` whole, so a second
//! writer's level would not last, and the two apps each keep their own level.
//!
//! The app zooms each window by [`Zoom::factor`] (GPUI's window zoom, a TreeTerm patch), as
//! Chromium zooms the whole page.

use std::io;
use std::path::{Path, PathBuf};

use gpui_kit::Global;
use serde::{Deserialize, Serialize};

/// Chromium's page zoom limits (`kMinimumBrowserZoomFactor`, `kMaximumBrowserZoomFactor`).
const MIN_FACTOR: f32 = 0.25;
const MAX_FACTOR: f32 = 5.0;

/// The zoom level, counted in Electron's half-level steps from Actual Size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Zoom {
    steps: i32,
}

impl Zoom {
    /// Electron's `webContents.zoomLevel`: half a level per step.
    pub fn level(&self) -> f64 {
        f64::from(self.steps) / 2.
    }

    /// The zoom at Electron's `level`, when that is a whole number of steps from Actual Size.
    pub fn at_level(level: f64) -> Option<Self> {
        let steps = level * 2.;
        let whole =
            steps.fract() == 0. && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&steps);
        whole.then_some(Self {
            steps: steps as i32,
        })
    }

    pub fn zoom_in(&mut self) {
        self.steps = self.steps.saturating_add(1);
    }

    pub fn zoom_out(&mut self) {
        self.steps = self.steps.saturating_sub(1);
    }

    pub fn reset(&mut self) {
        self.steps = 0;
    }

    /// How many times its size the window is drawn: 1.2^level, within Chromium's limits.
    pub fn factor(&self) -> f32 {
        1.2f32
            .powf(self.steps as f32 / 2.)
            .clamp(MIN_FACTOR, MAX_FACTOR)
    }
}

/// `zoom.json` in the user data directory, beside `settings.json` and `ui-state.json`.
pub fn zoom_path(user_data_dir: &Path) -> PathBuf {
    user_data_dir.join("zoom.json")
}

/// What `zoom.json` holds: `{"zoomLevel": <Electron's webContents.zoomLevel>}`.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    zoom_level: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum ZoomFileError {
    #[error("cannot read {path}: {source}")]
    Read { path: String, source: io::Error },
    #[error("{path} is not a saved zoom level: {source}")]
    Parse {
        path: String,
        source: serde_json::Error,
    },
    #[error("{path} holds zoom level {level}, which Zoom In and Zoom Out never reach")]
    OffStep { path: String, level: f64 },
    #[error("cannot write {path}: {source}")]
    Write { path: String, source: io::Error },
}

/// The app's zoom: one for every window, loaded from `zoom.json` at start and saved there at
/// every change, so the app starts at the level it quit at, as Electron's does.
#[derive(Debug)]
pub struct ZoomStore {
    /// `None` for [`ZoomStore::in_memory`]: the level lasts for this run.
    path: Option<PathBuf>,
    zoom: Zoom,
    /// The last load or save failure, also logged; cleared by the next save that works.
    error: Option<ZoomFileError>,
}

impl Global for ZoomStore {}

impl ZoomStore {
    /// The level saved in `user_data_dir`; Actual Size when none is saved yet. An unreadable or
    /// invalid file is reported and the app starts at Actual Size; the next change overwrites
    /// it, as it holds nothing but the level.
    pub fn load(user_data_dir: &Path) -> Self {
        let path = zoom_path(user_data_dir);
        let (zoom, error) = match read(&path) {
            Ok(zoom) => (zoom, None),
            Err(error) => {
                tracing::error!(%error, "zoom level not loaded; starting at Actual Size");
                (Zoom::default(), Some(error))
            }
        };
        Self {
            path: Some(path),
            zoom,
            error,
        }
    }

    /// At Actual Size, saved nowhere: for running without a user data directory, and tests.
    pub fn in_memory() -> Self {
        Self {
            path: None,
            zoom: Zoom::default(),
            error: None,
        }
    }

    pub fn zoom(&self) -> Zoom {
        self.zoom
    }

    pub fn error(&self) -> Option<&ZoomFileError> {
        self.error.as_ref()
    }

    pub fn zoom_in(&mut self) {
        self.zoom.zoom_in();
        self.save();
    }

    pub fn zoom_out(&mut self) {
        self.zoom.zoom_out();
        self.save();
    }

    pub fn reset(&mut self) {
        self.zoom.reset();
        self.save();
    }

    fn save(&mut self) {
        let Some(path) = &self.path else {
            return;
        };
        match write(path, self.zoom) {
            Ok(()) => self.error = None,
            Err(error) => {
                tracing::error!(%error, "zoom level not saved");
                self.error = Some(error);
            }
        }
    }
}

/// A missing file is Actual Size.
fn read(path: &Path) -> Result<Zoom, ZoomFileError> {
    let shown = || path.display().to_string();
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Zoom::default()),
        Err(source) => {
            return Err(ZoomFileError::Read {
                path: shown(),
                source,
            });
        }
    };
    let saved: Saved = serde_json::from_slice(&bytes).map_err(|source| ZoomFileError::Parse {
        path: shown(),
        source,
    })?;
    Zoom::at_level(saved.zoom_level).ok_or_else(|| ZoomFileError::OffStep {
        path: shown(),
        level: saved.zoom_level,
    })
}

/// Written whole through a temporary file, so a crash never leaves half a file.
fn write(path: &Path, zoom: Zoom) -> Result<(), ZoomFileError> {
    let error = |source| ZoomFileError::Write {
        path: path.display().to_string(),
        source,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(error)?;
    }
    let text = serde_json::to_string_pretty(&Saved {
        zoom_level: zoom.level(),
    })
    .map_err(|e| error(io::Error::other(e)))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text).map_err(error)?;
    std::fs::rename(&temp, path).map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zoomed(steps: i32) -> Zoom {
        let mut zoom = Zoom::default();
        for _ in 0..steps.abs() {
            if steps > 0 {
                zoom.zoom_in()
            } else {
                zoom.zoom_out()
            }
        }
        zoom
    }

    #[test]
    fn actual_size_draws_text_at_its_size() {
        assert_eq!(Zoom::default().factor(), 1.);
    }

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "{actual} is not {expected}"
        );
    }

    #[test]
    fn each_step_is_half_a_chromium_level() {
        assert_near(zoomed(1).factor(), 1.2f32.sqrt());
        assert_near(zoomed(2).factor(), 1.2);
        assert_near(zoomed(-2).factor(), 1. / 1.2);

        let mut zoom = zoomed(1);
        zoom.zoom_out();
        assert_eq!(
            zoom.factor(),
            1.,
            "Zoom In then Zoom Out is back to actual size"
        );
    }

    #[test]
    fn actual_size_resets_any_level() {
        for steps in [5, -5, 40] {
            let mut zoom = zoomed(steps);
            zoom.reset();
            assert_eq!(zoom, Zoom::default());
        }
    }

    /// Electron after 30 Zoom Ins is at level 15, drawn at 500%; it takes 13 Zoom Outs to
    /// come under 500% (level 8.5), and the same past 25%.
    #[test]
    fn the_factor_stays_within_chromiums_limits_while_the_level_runs_on() {
        let mut zoom = zoomed(30);
        assert_eq!(zoom.factor(), 5.);
        for _ in 0..12 {
            zoom.zoom_out();
        }
        assert_eq!(zoom.factor(), 5., "level 9 is past 500%");
        zoom.zoom_out();
        assert_near(zoom.factor(), 1.2f32.powf(8.5));

        let mut zoom = zoomed(-30);
        assert_eq!(zoom.factor(), 0.25);
        for _ in 0..14 {
            zoom.zoom_in();
        }
        assert_eq!(zoom.factor(), 0.25, "level -8 is past 25%");
        zoom.zoom_in();
        assert_near(zoom.factor(), 1.2f32.powf(-7.5));
    }

    #[test]
    fn levels_are_electrons_in_half_level_steps() {
        assert_eq!(zoomed(3).level(), 1.5);
        assert_eq!(zoomed(-1).level(), -0.5);
        assert_eq!(Zoom::at_level(1.5), Some(zoomed(3)));
        assert_eq!(Zoom::at_level(-0.5), Some(zoomed(-1)));
        assert_eq!(Zoom::at_level(15.), Some(zoomed(30)));
        for level in [0.3, 1e10, -1e10, f64::INFINITY, f64::NAN] {
            assert_eq!(Zoom::at_level(level), None, "{level}");
        }
    }

    /// Starts with the level saved in `dir`, and zooms `steps` from it.
    fn zoom_from_saved(dir: &Path, steps: i32) {
        let mut store = ZoomStore::load(dir);
        for _ in 0..steps.abs() {
            if steps > 0 {
                store.zoom_in()
            } else {
                store.zoom_out()
            }
        }
    }

    #[test]
    fn the_next_start_is_at_the_level_the_app_quit_at() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ZoomStore::load(dir.path());
        assert_eq!(
            store.zoom(),
            Zoom::default(),
            "nothing saved yet: Actual Size"
        );
        assert!(
            store.error().is_none(),
            "no file yet is no error: {:?}",
            store.error()
        );

        store.zoom_in();
        store.zoom_in();
        store.zoom_in();
        let reloaded = ZoomStore::load(dir.path());
        assert_eq!(reloaded.zoom(), zoomed(3));
        assert_near(reloaded.zoom().factor(), 1.2f32.powf(1.5));
        assert!(reloaded.error().is_none(), "{:?}", reloaded.error());
        let text = std::fs::read_to_string(zoom_path(dir.path())).unwrap();
        assert_eq!(text, "{\n  \"zoomLevel\": 1.5\n}", "Electron's level");

        store.zoom_out();
        assert_eq!(
            ZoomStore::load(dir.path()).zoom(),
            zoomed(2),
            "Zoom Out is saved"
        );

        store.reset();
        assert_eq!(
            ZoomStore::load(dir.path()).zoom().factor(),
            1.,
            "Actual Size is saved"
        );
    }

    /// Past Chromium's limits the level runs on, and is saved as it is.
    #[test]
    fn a_level_past_the_limits_is_restored_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        zoom_from_saved(dir.path(), 30);
        assert_eq!(ZoomStore::load(dir.path()).zoom(), zoomed(30));
        zoom_from_saved(dir.path(), -60);
        assert_eq!(ZoomStore::load(dir.path()).zoom(), zoomed(-30));
    }

    #[test]
    fn an_invalid_file_is_reported_and_the_app_starts_at_actual_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = zoom_path(dir.path());
        for text in ["{nope", "", "{}", r#"{"zoomLevel":"big"}"#, "null"] {
            std::fs::write(&path, text).unwrap();
            let store = ZoomStore::load(dir.path());
            assert_eq!(store.zoom(), Zoom::default(), "{text:?}");
            assert!(
                matches!(store.error(), Some(ZoomFileError::Parse { .. })),
                "{text:?}: {:?}",
                store.error()
            );
        }
        for text in [r#"{"zoomLevel":0.3}"#, r#"{"zoomLevel":1e10}"#] {
            std::fs::write(&path, text).unwrap();
            let store = ZoomStore::load(dir.path());
            assert_eq!(store.zoom(), Zoom::default(), "{text}");
            assert!(
                matches!(store.error(), Some(ZoomFileError::OffStep { .. })),
                "{text}: {:?}",
                store.error()
            );
        }

        // The next change replaces it.
        let mut store = ZoomStore::load(dir.path());
        store.zoom_in();
        assert!(store.error().is_none(), "saved: {:?}", store.error());
        assert_eq!(ZoomStore::load(dir.path()).zoom(), zoomed(1));
    }

    #[test]
    fn a_file_that_cannot_be_read_or_written_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        // A directory that is not empty in the file's place.
        std::fs::create_dir_all(zoom_path(dir.path()).join("full")).unwrap();
        let mut store = ZoomStore::load(dir.path());
        assert_eq!(store.zoom(), Zoom::default());
        assert!(
            matches!(store.error(), Some(ZoomFileError::Read { .. })),
            "{:?}",
            store.error()
        );

        store.zoom_in();
        assert_eq!(store.zoom(), zoomed(1), "the level still changes");
        assert!(
            matches!(store.error(), Some(ZoomFileError::Write { .. })),
            "{:?}",
            store.error()
        );
    }

    #[test]
    fn in_memory_the_level_changes_and_is_saved_nowhere() {
        let mut store = ZoomStore::in_memory();
        store.zoom_in();
        assert_eq!(store.zoom(), zoomed(1));
        assert!(store.error().is_none());
    }
}
