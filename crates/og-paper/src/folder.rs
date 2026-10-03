// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Sync through a shared folder (Syncthing, Dropbox, Drive, a USB stick):
//! no server, works offline, merges whenever the folder catches up.
//!
//! Inside the chosen folder each canvas has a subfolder named by its id,
//! and in it each device keeps one file, `<peer>.ogpt`: a full copy of the
//! canvas as that device has it. A device writes only its own file
//! (written aside, then renamed, so others never read half of it) and
//! merges every other file whenever it changes. No two devices ever write
//! the same file, so the folder tool never has a conflict to resolve.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use web_time::{Duration, Instant};

use crate::App;

/// How long after a change our file is written (changes in between join).
const WRITE_AFTER: Duration = Duration::from_secs(2);
/// How often the folder is checked for others' changes.
const POLL_EVERY: Duration = Duration::from_secs(3);

pub struct Folder {
    /// The canvas's subfolder.
    pub dir: PathBuf,
    /// When each other device's file was last merged (its modified time).
    seen: HashMap<PathBuf, SystemTime>,
    /// Something changed here since our file was last written.
    dirty_since: Option<Instant>,
    last_poll: Option<Instant>,
}

/// This canvas's subfolder in `root`.
pub fn canvas_dir(root: &Path, canvas: u128) -> PathBuf {
    root.join(format!("og-paper-{canvas:032x}"))
}

/// Write `bytes` as `path` without anyone seeing half of it.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("ogpt.part");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Other devices' copies in `dir` that changed since `seen`, with their
/// modified times (our own file `own` left out).
pub fn changed_files(
    dir: &Path,
    own: &Path,
    seen: &HashMap<PathBuf, SystemTime>,
) -> Vec<(PathBuf, SystemTime)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p == own || p.extension().is_none_or(|x| x != "ogpt") {
            continue;
        }
        let Ok(t) = e.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if seen.get(&p).is_none_or(|old| t > *old) {
            out.push((p, t));
        }
    }
    out.sort();
    out
}

impl Folder {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            seen: HashMap::new(),
            dirty_since: Some(Instant::now() - WRITE_AFTER),
            last_poll: None,
        }
    }
}

impl App {
    fn folder_key(&self) -> String {
        format!("folder.{:032x}", self.share.canvas)
    }

    /// Start syncing this canvas through `root` (remembered for it).
    pub(crate) fn folder_start(&mut self, root: PathBuf) {
        let dir = canvas_dir(&root, self.share.canvas);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.say(format!("Could not use that folder: {e}"));
            return;
        }
        let mut p = crate::prefs::load();
        p.insert(self.folder_key(), root.display().to_string());
        crate::prefs::save(&p);
        self.folder = Some(Folder::new(dir));
        self.say(format!(
            "Syncing through {}: other devices pick the same folder",
            root.display()
        ));
        self.folder_tick();
    }

    pub(crate) fn folder_stop(&mut self) {
        self.folder = None;
        let mut p = crate::prefs::load();
        p.remove(&self.folder_key());
        crate::prefs::save(&p);
        self.say("Stopped syncing through the folder");
    }

    /// Resume the folder remembered for the canvas just opened, if any.
    pub(crate) fn folder_resume(&mut self) {
        self.folder = None;
        let root = crate::prefs::load().get(&self.folder_key()).cloned();
        if let Some(root) = root.map(PathBuf::from).filter(|r| r.is_dir()) {
            self.folder = Some(Folder::new(canvas_dir(&root, self.share.canvas)));
        }
    }

    /// Something changed here: write our copy soon.
    pub(crate) fn folder_touch(&mut self) {
        if let Some(f) = self.folder.as_mut() {
            f.dirty_since.get_or_insert_with(Instant::now);
        }
    }

    /// Write our copy when due; merge others' when they changed. Called
    /// every frame and on a timer while a folder is set.
    pub(crate) fn folder_tick(&mut self) {
        let Some(f) = self.folder.as_ref() else {
            return;
        };
        // Not while a stroke or drag is in progress.
        if self.busy() {
            return;
        }
        let own = f.dir.join(format!("{:016x}.ogpt", self.share.clock.peer()));
        let poll = f.last_poll.is_none_or(|t| t.elapsed() >= POLL_EVERY);
        if poll {
            let changed = changed_files(&f.dir, &own, &f.seen);
            if let Some(f) = self.folder.as_mut() {
                f.last_poll = Some(Instant::now());
            }
            for (path, t) in changed {
                match std::fs::read(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|b| crate::snapshot::decode(&b, crate::BASE_PX))
                {
                    Ok(s) => {
                        self.merge_quiet(s.scene, s.objs, s.share);
                        if let Some(f) = self.folder.as_mut() {
                            f.seen.insert(path, t);
                        }
                    }
                    // Half-synced by the folder tool: try again next poll.
                    Err(e) => log::debug!("folder copy {}: {e}", path.display()),
                }
            }
        }
        let Some(f) = self.folder.as_ref() else {
            return;
        };
        if f.dirty_since.is_some_and(|t| t.elapsed() >= WRITE_AFTER) {
            let bytes = crate::snapshot::encode(
                &self.scene,
                &self.cam,
                &self.timeline,
                &self.bookmarks,
                &self.objs,
                &self.share.copy_log(),
            );
            match write_atomic(&own, &bytes) {
                Ok(()) => {
                    if let Some(f) = self.folder.as_mut() {
                        f.dirty_since = None;
                    }
                }
                Err(e) => self.say(format!("Could not write to the sync folder: {e}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_others_changed_files_are_picked_up() {
        let tmp = std::env::temp_dir().join(format!("ogp-folder-{}", crate::uid::new()));
        let dir = canvas_dir(&tmp, 42);
        std::fs::create_dir_all(&dir).unwrap();
        let own = dir.join("0000000000000001.ogpt");
        write_atomic(&own, b"mine").unwrap();
        write_atomic(&dir.join("0000000000000002.ogpt"), b"theirs").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let mut seen = HashMap::new();
        let got = changed_files(&dir, &own, &seen);
        assert_eq!(got.len(), 1);
        assert!(got[0].0.ends_with("0000000000000002.ogpt"));
        assert!(!dir.join("0000000000000002.ogpt.part").exists());
        seen.insert(got[0].0.clone(), got[0].1);
        assert!(changed_files(&dir, &own, &seen).is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
