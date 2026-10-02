// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Time stamps for ink: every time a stroke appears (drawn, undo of an erase)
//! or disappears (erased, undo of a draw) is logged with the wall-clock time,
//! so the canvas can be shown as it was at any earlier moment.

use ogpaper_core::{Camera, Scene};

/// One change: at `t` (ms since the Unix epoch) stroke `id` became visible
/// (`alive`) or hidden.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Event {
    pub t: i64,
    pub id: u32,
    pub alive: bool,
}

#[derive(Default)]
pub struct Timeline {
    /// Sorted by time (appends come from the clock; loads are sorted).
    pub events: Vec<Event>,
}

pub fn now_ms() -> i64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Creation time hidden in a UUIDv7 stroke id, if it is one.
pub fn uid_ms(uid: u128) -> Option<i64> {
    ((uid >> 76) & 0xF == 7).then_some((uid >> 80) as i64)
}

impl Timeline {
    pub fn record(&mut self, id: u32, alive: bool) {
        // Never step backwards, even if the clock does.
        let t = now_ms().max(self.events.last().map_or(0, |e| e.t));
        self.events.push(Event { t, id, alive });
    }

    /// A log for a scene loaded without one: each stroke appears when its id
    /// says it was made (or at `fallback`), and deleted ones vanish at `fallback`.
    pub fn from_scene(scene: &Scene, fallback: i64) -> Self {
        let mut events = Vec::new();
        for (i, s) in scene.strokes.iter().enumerate() {
            let t = uid_ms(s.uid).unwrap_or(fallback);
            events.push(Event {
                t,
                id: i as u32,
                alive: true,
            });
            if s.deleted {
                events.push(Event {
                    t: t.max(fallback),
                    id: i as u32,
                    alive: false,
                });
            }
        }
        Self::from_events(events)
    }

    pub fn from_events(mut events: Vec<Event>) -> Self {
        events.sort_by_key(|e| e.t);
        Self { events }
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    /// Which strokes were visible just after event `upto` (inclusive).
    pub fn visible_after(&self, upto: usize, strokes: usize) -> Vec<bool> {
        let mut v = vec![false; strokes];
        for e in self.events.iter().take(upto + 1) {
            if let Some(s) = v.get_mut(e.id as usize) {
                *s = e.alive;
            }
        }
        v
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    /// A window of time: strokes visible just after event `upto` that were
    /// first drawn at or after event `from`. Older ink is left out, so one
    /// stretch of work can be looked at on its own.
    pub fn visible_between(&self, from: usize, upto: usize, strokes: usize) -> Vec<bool> {
        let mut v = self.visible_after(upto, strokes);
        if from == 0 {
            return v;
        }
        // When each stroke first appeared (its creation).
        let mut born = vec![usize::MAX; strokes];
        for (i, e) in self.events.iter().enumerate().take(upto + 1) {
            if e.alive {
                if let Some(b) = born.get_mut(e.id as usize) {
                    if *b == usize::MAX {
                        *b = i;
                    }
                }
            }
        }
        for (vis, &b) in v.iter_mut().zip(&born) {
            if b < from {
                *vis = false;
            }
        }
        v
    }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
/// Make the scene's deleted flags match `visible` (keeps subtree counts right).
pub fn apply(scene: &mut Scene, visible: &[bool]) {
    for (id, &vis) in visible.iter().enumerate() {
        if vis {
            scene.restore(id as u32);
        } else {
            scene.delete(id as u32);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
/// A saved view to fly back to.
#[derive(Clone)]
pub struct Bookmark {
    pub name: String,
    pub cam: Camera,
    /// Smaller side of the viewport (px) when saved, so the same area is
    /// framed on a phone and on a monitor.
    pub view_px: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ogpaper_core::CellAddr;

    #[test]
    fn replays_draws_and_erases() {
        let mut s = Scene::new();
        let a = s.add_stroke(&CellAddr::new(0, 0, 0), &[[0.1, 0.1], [0.2, 0.2]], 0.01, 0);
        let b = s.add_stroke(&CellAddr::new(0, 0, 0), &[[0.3, 0.3], [0.4, 0.4]], 0.01, 0);
        let tl = Timeline::from_events(vec![
            Event {
                t: 1,
                id: a,
                alive: true,
            },
            Event {
                t: 2,
                id: b,
                alive: true,
            },
            Event {
                t: 3,
                id: a,
                alive: false,
            },
        ]);
        assert_eq!(tl.visible_after(0, 2), vec![true, false]);
        assert_eq!(tl.visible_after(1, 2), vec![true, true]);
        assert_eq!(tl.visible_after(2, 2), vec![false, true]);
        apply(&mut s, &tl.visible_after(2, 2));
        assert!(s.strokes[a as usize].deleted && !s.strokes[b as usize].deleted);
        assert_eq!(s.node(s.roots[0]).subtree, 1);
    }

    #[test]
    fn window_leaves_out_older_ink() {
        let mut s = Scene::new();
        let a = s.add_stroke(&CellAddr::new(0, 0, 0), &[[0.1, 0.1], [0.2, 0.2]], 0.01, 0);
        let b = s.add_stroke(&CellAddr::new(0, 0, 0), &[[0.3, 0.3], [0.4, 0.4]], 0.01, 0);
        let c = s.add_stroke(&CellAddr::new(0, 0, 0), &[[0.5, 0.5], [0.6, 0.6]], 0.01, 0);
        let ev = |t, id, alive| Event { t, id, alive };
        // a drawn, b drawn, a erased, c drawn, a restored (undo of the erase).
        let tl = Timeline::from_events(vec![
            ev(1, a, true),
            ev(2, b, true),
            ev(3, a, false),
            ev(4, c, true),
            ev(5, a, true),
        ]);
        // From the start: same as the single-handle view.
        assert_eq!(tl.visible_between(0, 4, 3), tl.visible_after(4, 3));
        // From event 1 (b drawn): a was born before the window, so it stays
        // out even though it came back inside it.
        assert_eq!(tl.visible_between(1, 4, 3), vec![false, true, true]);
        // Window [3, 3]: only c.
        assert_eq!(tl.visible_between(3, 3, 3), vec![false, false, true]);
    }

    #[test]
    fn reads_v7_time() {
        let id = crate::uid::new();
        let t = uid_ms(id).unwrap();
        assert!((t - now_ms()).abs() < 5_000);
        assert_eq!(uid_ms(0), None);
    }
}
