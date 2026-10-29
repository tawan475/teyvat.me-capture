// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The window's place and size, as eframe keeps them in `app.ron`
//! (`persist_window`).
//!
//! eframe saves the window as it is at that moment, every 30 s and on exit,
//! minimized too. A minimized window on Windows has a 0×0 client area and sits
//! at the minimized caption's spot above the taskbar, so a session that ended
//! in the tray (Quit from the tray menu, or the startup task's "Start
//! minimized" run) came back as a tiny window in the bottom-left corner,
//! eframe's 64-point floor. [`TeyvatmeApp`](crate::app::TeyvatmeApp) keeps the
//! window as last [`Seen`] in place of such a one, and [`forget_tiny`] drops
//! one an earlier build saved before eframe reads it.

use std::collections::HashMap;
use std::path::Path;

/// eframe's key for the window record.
pub const KEY: &str = "window";

/// eframe's settings file in a storage directory.
const SETTINGS_FILE: &str = "app.ron";

/// Smaller than this (either side, in points) is not a window anyone sized:
/// the window's own minimum is 380×560, a minimized one reports 0×0.
const MIN_SIDE: f32 = 128.0;

/// The window size a record holds, in points.
fn size(record: &str) -> Option<egui::Vec2> {
    ron::from_str::<egui_winit::WindowSettings>(record)
        .ok()
        .and_then(|settings| settings.inner_size_points())
}

/// Whether a window record describes a window someone could use. One eframe
/// can't read (an older format) is not.
pub fn usable(record: &str) -> bool {
    size(record).is_some_and(|size| size.x >= MIN_SIDE && size.y >= MIN_SIDE)
}

/// Whether a record would open the window too small to use. One without a
/// size (or eframe can't read) only leaves eframe's default.
fn tiny(record: &str) -> bool {
    size(record).is_some_and(|size| size.x < MIN_SIDE || size.y < MIN_SIDE)
}

/// No record: eframe opens the window at its default size and place.
pub fn none() -> String {
    ron::to_string(&egui_winit::WindowSettings::default()).unwrap_or_default()
}

/// The window as a frame in sight sees it. eframe saves only on a frame, and
/// an idle window draws none, so its own record can be older than the last
/// place and size the window had in sight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seen {
    /// In points.
    inner: egui::Rect,
    outer: egui::Rect,
    pixels_per_point: f32,
    maximized: bool,
    fullscreen: bool,
}

impl Seen {
    pub fn of(ctx: &egui::Context) -> Option<Self> {
        ctx.input(|input| {
            let viewport = input.viewport();
            Some(Self {
                inner: viewport.inner_rect?,
                outer: viewport.outer_rect?,
                pixels_per_point: input.pixels_per_point,
                maximized: viewport.maximized.unwrap_or(false),
                fullscreen: viewport.fullscreen.unwrap_or(false),
            })
        })
    }

    /// The record eframe would save for it (`WindowSettings::from_window`):
    /// positions in physical pixels, the size in points.
    pub fn record(&self) -> String {
        #[derive(serde::Serialize)]
        struct Record {
            inner_position_pixels: Option<egui::Pos2>,
            outer_position_pixels: Option<egui::Pos2>,
            fullscreen: bool,
            maximized: bool,
            inner_size_points: Option<egui::Vec2>,
        }
        let pixels = |point: egui::Pos2| (point * self.pixels_per_point).round();
        ron::to_string(&Record {
            inner_position_pixels: Some(pixels(self.inner.min)),
            outer_position_pixels: Some(pixels(self.outer.min)),
            fullscreen: self.fullscreen,
            maximized: self.maximized,
            inner_size_points: Some(self.inner.size()),
        })
        .unwrap_or_default()
    }
}

/// Drop a window record too small to use from `dir`'s `app.ron`, before
/// eframe reads it. Returns whether one was dropped; failures are logged and
/// change nothing.
pub fn forget_tiny(dir: Option<&Path>) -> bool {
    let Some(dir) = dir else {
        return false;
    };
    let path = dir.join(SETTINGS_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(mut settings) = ron::from_str::<HashMap<String, String>>(&text) else {
        return false;
    };
    if !settings.get(KEY).is_some_and(|record| tiny(record)) {
        return false;
    }
    settings.remove(KEY);
    let written = ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::default())
        .map_err(std::io::Error::other)
        .and_then(|text| {
            let temp = path.with_extension("ron.tmp");
            std::fs::write(&temp, text)?;
            std::fs::rename(&temp, &path)
        });
    match written {
        Ok(()) => {
            tracing::info!(
                "dropped a minimized window's saved size; the window opens at its default"
            );
            true
        }
        Err(e) => {
            tracing::warn!("could not drop the minimized window's saved size from {path:?}: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What eframe 0.32 wrote for a window minimized to the tray (inner size
    /// 0×0, at the minimized caption's spot), and for a window in use.
    const MINIMIZED: &str = "(inner_position_pixels:Some((x:160.0,y:1364.0)),outer_position_pixels:Some((x:160.0,y:1364.0)),fullscreen:false,maximized:false,inner_size_points:Some((x:0.0,y:0.0)))";
    const IN_USE: &str = "(inner_position_pixels:Some((x:248.0,y:291.0)),outer_position_pixels:Some((x:240.0,y:260.0)),fullscreen:false,maximized:false,inner_size_points:Some((x:960.0,y:600.0)))";

    #[test]
    fn a_minimized_windows_record_is_not_usable() {
        assert!(!usable(MINIMIZED));
        // What the next launch saved after opening from it: eframe's floor.
        assert!(!usable(
            &MINIMIZED.replace("(x:0.0,y:0.0)", "(x:64.0,y:64.0)")
        ));
        assert!(usable(IN_USE));
        assert!(!usable("not ron"));
        assert!(!usable(&none()));

        assert!(tiny(MINIMIZED));
        assert!(!tiny(IN_USE));
        assert!(!tiny(&none()));
        assert!(!tiny("not ron"));
    }

    #[test]
    fn no_record_reads_back_as_eframes_default() {
        let settings: egui_winit::WindowSettings = ron::from_str(&none()).unwrap();
        assert_eq!(settings.inner_size_points(), None);
    }

    #[test]
    fn a_seen_window_is_recorded_as_eframe_reads_it() {
        // 960×600 points at 150 %, its frame 8 px left and 31 px above.
        let seen = Seen {
            inner: egui::Rect::from_min_size(egui::pos2(165.25, 194.0), egui::vec2(960.0, 600.0)),
            outer: egui::Rect::from_min_size(egui::pos2(160.0, 173.25), egui::vec2(970.0, 625.0)),
            pixels_per_point: 1.5,
            maximized: false,
            fullscreen: false,
        };
        let record = seen.record();
        let settings: egui_winit::WindowSettings = ron::from_str(&record).unwrap();
        assert_eq!(settings.inner_size_points(), Some(egui::vec2(960.0, 600.0)));
        // Every field read back: a name eframe doesn't know would be dropped.
        assert_eq!(ron::to_string(&settings).unwrap(), record);
        assert!(record.contains("inner_position_pixels:Some((x:248.0,y:291.0))"));
        assert!(record.contains("outer_position_pixels:Some((x:240.0,y:260.0))"));
        assert!(usable(&record));
    }

    fn settings_file(window: &str) -> String {
        let settings = HashMap::from([
            (KEY.to_owned(), window.to_owned()),
            ("app".to_owned(), "(theme:Dark)".to_owned()),
        ]);
        ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::default()).unwrap()
    }

    fn read(dir: &Path) -> HashMap<String, String> {
        ron::from_str(&std::fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap()).unwrap()
    }

    #[test]
    fn a_tiny_record_is_dropped_and_the_rest_kept() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(SETTINGS_FILE), settings_file(MINIMIZED)).unwrap();

        assert!(forget_tiny(Some(dir.path())));
        let settings = read(dir.path());
        assert_eq!(settings.get(KEY), None);
        assert_eq!(
            settings.get("app").map(String::as_str),
            Some("(theme:Dark)")
        );
        assert!(!dir.path().join("app.ron.tmp").exists());
    }

    #[test]
    fn a_usable_record_or_no_record_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let file = settings_file(IN_USE);
        std::fs::write(dir.path().join(SETTINGS_FILE), &file).unwrap();
        assert!(!forget_tiny(Some(dir.path())));
        assert_eq!(
            std::fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap(),
            file
        );

        let file = settings_file(&none());
        std::fs::write(dir.path().join(SETTINGS_FILE), &file).unwrap();
        assert!(!forget_tiny(Some(dir.path())));
        assert_eq!(
            std::fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap(),
            file
        );

        assert!(!forget_tiny(Some(&dir.path().join("missing"))));
        assert!(!forget_tiny(None));
    }
}
