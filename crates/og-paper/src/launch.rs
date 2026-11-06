// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! How far the app got while starting, written to `launch.log` in the
//! canvas folder and to the log, and a message on the screen when it can't
//! start. An app that fails silently shows an empty window and nobody can
//! say why; here it says so, and the launch test in the Mac App Store
//! workflow reads the same file.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Where people get help (the listing's Support URL).
#[cfg(desktop)]
const SUPPORT: &str = "https://omegagiven.github.io/OG-paper/support/";
/// How long a window may stay undrawn before the app says so.
const PATIENCE: Duration = Duration::from_secs(15);
/// Frames lost before the first one shows: this many are written down.
const DROPS_NOTED: u32 = 8;

static STARTED: OnceLock<Instant> = OnceLock::new();
static STAGE: Mutex<&str> = Mutex::new("not started");
static PRESENTED: AtomicBool = AtomicBool::new(false);
static UI_SHOWN: AtomicBool = AtomicBool::new(false);
#[cfg(desktop)]
static WARNED: AtomicBool = AtomicBool::new(false);
static DROPPED: AtomicU32 = AtomicU32::new(0);

fn log_path() -> Option<PathBuf> {
    Some(crate::canvas_dir()?.join("launch.log"))
}

fn elapsed() -> f32 {
    STARTED.get().map_or(0.0, |t| t.elapsed().as_secs_f32())
}

/// A line in `launch.log`. Failing to write it never matters.
pub(crate) fn note(line: &str) {
    log::info!("launch: {line}");
    let Some(path) = log_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{:7.3}s {line}", elapsed());
    }
}

/// Start a new `launch.log` and catch panics. Call once, first thing.
pub(crate) fn begin() {
    let _ = STARTED.set(Instant::now());
    if let Some(p) = log_path() {
        let _ = std::fs::remove_file(p);
    }
    note(concat!("OG Paper ", env!("CARGO_PKG_VERSION")));
    note(&format!(
        "{} {}, home {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        crate::canvas_dir().map_or("(none)".into(), |d| d.display().to_string())
    ));
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let stage = *STAGE.lock().unwrap_or_else(|e| e.into_inner());
        note(&format!("panic during \"{stage}\": {info}"));
        before(info);
        // The window's thread can't draw any more: say so on the screen.
        #[cfg(desktop)]
        if std::thread::current().name() == Some("main") {
            alert(
                "OG Paper stopped",
                &format!("Something went wrong during \"{stage}\".\n\n{}", help()),
            );
        }
    }));
    // Stuck on the window's thread nothing else can report it, so a thread
    // of its own writes down where it got to.
    let _ = std::thread::Builder::new()
        .name("launch watch".into())
        .spawn(|| {
            std::thread::sleep(PATIENCE);
            if !PRESENTED.load(Ordering::Relaxed) {
                let stage = *STAGE.lock().unwrap_or_else(|e| e.into_inner());
                note(&format!(
                    "no frame after {}s: still at \"{stage}\", {} frames lost",
                    PATIENCE.as_secs(),
                    DROPPED.load(Ordering::Relaxed)
                ));
            }
        });
}

/// Starting has reached this point.
pub(crate) fn stage(name: &'static str) {
    *STAGE.lock().unwrap_or_else(|e| e.into_inner()) = name;
    note(name);
}

/// A frame the system didn't give us, before the first one showed.
pub(crate) fn dropped(why: &str) {
    if PRESENTED.load(Ordering::Relaxed) {
        return;
    }
    let n = DROPPED.fetch_add(1, Ordering::Relaxed) + 1;
    if n <= DROPS_NOTED {
        note(&format!("frame {n} lost: {why}"));
    }
}

/// A frame reached the screen, with this many UI meshes and UI textures.
pub(crate) fn presented(meshes: usize, textures: usize) {
    if !PRESENTED.swap(true, Ordering::Relaxed) {
        stage("first frame on screen");
    }
    if meshes > 0 && !UI_SHOWN.swap(true, Ordering::Relaxed) {
        note(&format!(
            "ui on screen: {meshes} meshes, {textures} textures, {} frames lost first",
            DROPPED.load(Ordering::Relaxed)
        ));
    }
}

/// The window has gone a long time with nothing drawn, and nobody has been
/// told yet. True once.
#[cfg(desktop)]
pub(crate) fn overdue() -> bool {
    !PRESENTED.load(Ordering::Relaxed)
        && !WARNED.load(Ordering::Relaxed)
        && elapsed() > PATIENCE.as_secs_f32()
        && !WARNED.swap(true, Ordering::Relaxed)
}

/// Tell the person the window can't be drawn. The app keeps running: a frame
/// may yet come.
#[cfg(desktop)]
pub(crate) fn warn_no_frame() {
    note("told the user no frame came");
    alert(
        "OG Paper can't draw its window",
        &format!(
            "This Mac or PC hasn't let OG Paper draw anything yet. Your canvases are safe.\n\n\
             Try quitting and opening it again.\n\n{}",
            help()
        ),
    );
}

/// Starting can't go on: say why, then stop.
pub(crate) fn fatal(what: &str, detail: &str) -> ! {
    note(&format!("failed: {what}: {detail}"));
    #[cfg(desktop)]
    {
        alert(
            "OG Paper can't start",
            &format!("{what}.\n\n{detail}\n\n{}", help()),
        );
        std::process::exit(1);
    }
    #[cfg(not(desktop))]
    panic!("{what}: {detail}");
}

#[cfg(desktop)]
fn help() -> String {
    let log = log_path().map_or(String::new(), |p| {
        format!("\n\nDetails are in {}", p.display())
    });
    format!("For help: {SUPPORT}{log}")
}

#[cfg(desktop)]
fn alert(title: &str, body: &str) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(title)
        .set_description(body)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}
