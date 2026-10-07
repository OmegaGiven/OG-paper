// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Public views: a saved view (see `timeline::Bookmark`) marked public goes
//! to everyone on the same page, under its author's name, for them to fly
//! to or copy. Each view travels by its id (`Msg::Views`): made private or
//! deleted, it goes out once more marked removed. The host passes them on
//! to the other guests, gives a newcomer all it knows, and a page server
//! keeps them in the page file for people who come later.

use ogpaper_core::Camera;

use crate::timeline::Bookmark;
use crate::wire::Msg;
use crate::App;

/// Someone's public view (or the word that it is gone).
#[derive(Clone, Debug)]
pub struct SharedView {
    pub id: u128,
    pub name: String,
    pub author: String,
    pub cam: Camera,
    pub view_px: f64,
    pub when: Option<(i64, i64)>,
    pub removed: bool,
}

impl PartialEq for SharedView {
    fn eq(&self, o: &Self) -> bool {
        self.id == o.id
            && self.name == o.name
            && self.author == o.author
            && self.cam.cell == o.cam.cell
            && self.cam.off == o.cam.off
            && self.cam.scale == o.cam.scale
            && self.view_px == o.view_px
            && self.when == o.when
            && self.removed == o.removed
    }
}

impl SharedView {
    pub fn of(b: &Bookmark, author: &str) -> Self {
        SharedView {
            id: b.id,
            name: b.name.clone(),
            author: author.to_string(),
            cam: b.cam.clone(),
            view_px: b.view_px,
            when: b.when,
            removed: !b.public,
        }
    }

    /// A copy to keep as one's own (private) view.
    pub fn to_bookmark(&self) -> Bookmark {
        Bookmark {
            name: self.name.clone(),
            cam: self.cam.clone(),
            view_px: self.view_px,
            when: self.when,
            public: false,
            id: 0,
        }
    }
}

/// The page file's key for the public views it keeps (a page server's).
#[cfg(not(target_arch = "wasm32"))]
const FILE_KEY: &str = "public_views";

impl App {
    /// My views that are public, as others see them.
    fn my_public_views(&self) -> Vec<SharedView> {
        let me = crate::presence::my_name();
        self.bookmarks
            .iter()
            .filter(|b| b.public && b.id != 0)
            .map(|b| SharedView::of(b, &me))
            .collect()
    }

    /// Everything public this side knows: mine, and (a host's) everyone's.
    fn all_public_views(&self) -> Vec<SharedView> {
        let mut v = self.my_public_views();
        v.extend(self.shared_views.iter().cloned());
        v
    }

    /// Tell the others about `views` (mine just changed: made public,
    /// renamed, made private or deleted).
    pub(crate) fn send_views(&mut self, views: Vec<SharedView>) {
        if self.net.is_none() || views.is_empty() {
            return;
        }
        let m = Msg::Views(crate::snapshot::encode_views(&views));
        self.broadcast(&m, None);
    }

    /// A view of mine changed: send it (or its removal).
    pub(crate) fn public_view_changed(&mut self, i: usize) {
        let me = crate::presence::my_name();
        if let Some(b) = self.bookmarks.get(i) {
            if b.id != 0 {
                let v = SharedView::of(b, &me);
                self.send_views(vec![v]);
            }
        }
    }

    /// Make view `i` public or private.
    pub(crate) fn view_set_public(&mut self, i: usize, public: bool) {
        let Some(b) = self.bookmarks.get_mut(i) else {
            return;
        };
        if b.id == 0 {
            b.id = crate::uid::new();
        }
        b.public = public;
        let name = b.name.clone();
        self.public_view_changed(i);
        self.say(if public {
            if self.net.is_some() {
                format!("\"{name}\" is public: everyone on this page can fly to it")
            } else {
                format!("\"{name}\" is public: people on this page will see it once it is shared")
            }
        } else {
            format!("\"{name}\" is private again")
        });
        #[cfg(target_arch = "wasm32")]
        crate::web::touch();
    }

    /// A view of mine is being deleted: if it was public, say it is gone.
    pub(crate) fn view_deleting(&mut self, i: usize) {
        if let Some(b) = self.bookmarks.get(i) {
            if b.public && b.id != 0 {
                let mut v = SharedView::of(b, &crate::presence::my_name());
                v.removed = true;
                self.send_views(vec![v]);
            }
        }
    }

    /// Fly to someone's public view.
    pub(crate) fn shared_view_go(&mut self, i: usize) {
        let Some(v) = self.shared_views.get(i).cloned() else {
            return;
        };
        let b = v.to_bookmark();
        if let Some(when) = b.when {
            if let Some(span) = self.timeline.span_of(when) {
                self.timeline_range(Some(span));
            }
        }
        self.fly_to(b.cam, b.view_px);
    }

    /// Keep a copy of someone's public view as one's own.
    pub(crate) fn shared_view_copy(&mut self, i: usize) {
        let Some(v) = self.shared_views.get(i) else {
            return;
        };
        let b = v.to_bookmark();
        self.say(format!("Saved a copy of {}'s \"{}\"", v.author, b.name));
        self.bookmarks.push(b);
        #[cfg(target_arch = "wasm32")]
        crate::web::touch();
    }

    /// Views arrived from connection `from`: keep them (a host also passes
    /// them on to the other guests and keeps them in its page file).
    pub(crate) fn views_arrived(&mut self, from: u64, bytes: &[u8]) {
        let Ok(views) = crate::snapshot::decode_views(bytes, crate::BASE_PX) else {
            return;
        };
        let mine: Vec<u128> = self
            .bookmarks
            .iter()
            .map(|b| b.id)
            .filter(|&id| id != 0)
            .collect();
        let mut changed = false;
        for v in views {
            if v.id == 0 || mine.contains(&v.id) {
                continue;
            }
            let at = self.shared_views.iter().position(|s| s.id == v.id);
            match (at, v.removed) {
                (Some(k), true) => {
                    self.shared_views.remove(k);
                    changed = true;
                }
                (Some(k), false) if self.shared_views[k] != v => {
                    self.shared_views[k] = v;
                    changed = true;
                }
                (None, false) => {
                    self.shared_views.push(v);
                    changed = true;
                }
                _ => {}
            }
        }
        if !changed {
            return;
        }
        if self.net.as_ref().is_some_and(|n| n.is_host()) {
            self.broadcast(&Msg::Views(bytes.to_vec()), Some(from));
            self.save_shared_views();
        }
        self.redraw();
    }

    /// A guest has just joined (host): send it every public view known.
    pub(crate) fn views_for_newcomer(&mut self, id: u64) {
        let all = self.all_public_views();
        if !all.is_empty() {
            self.send(id, &Msg::Views(crate::snapshot::encode_views(&all)));
        }
    }

    /// Joined (guest): share my public views with the page.
    pub(crate) fn views_on_join(&mut self) {
        let mine = self.my_public_views();
        self.send_views(mine);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_shared_views(&self) {
        if let Some(f) = &self.file {
            let _ = f.put_text(
                FILE_KEY,
                &hex(&crate::snapshot::encode_views(&self.shared_views)),
            );
        }
    }
    #[cfg(target_arch = "wasm32")]
    fn save_shared_views(&self) {}

    /// The public views a page file keeps (a page server's).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn load_shared_views(&mut self) {
        self.shared_views = self
            .file
            .as_ref()
            .and_then(|f| f.get_text(FILE_KEY).ok().flatten())
            .and_then(|h| unhex(&h))
            .and_then(|b| crate::snapshot::decode_views(&b, crate::BASE_PX).ok())
            .unwrap_or_default();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn unhex(s: &str) -> Option<Vec<u8>> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(id: u128, name: &str, removed: bool) -> SharedView {
        SharedView {
            id,
            name: name.into(),
            author: "Ann".into(),
            cam: crate::home_camera(),
            view_px: 800.0,
            when: Some((i64::MIN, 1_000)),
            removed,
        }
    }

    #[test]
    fn views_merge_by_id() {
        // The codec round trips.
        let vs = vec![view(7, "Tower", false), view(9, "Bridge", true)];
        let back =
            crate::snapshot::decode_views(&crate::snapshot::encode_views(&vs), crate::BASE_PX)
                .unwrap();
        assert_eq!(back, vs);

        let mut app = App::new(None);
        let send = |app: &mut App, v: Vec<SharedView>| {
            app.views_arrived(0, &crate::snapshot::encode_views(&v));
        };
        send(
            &mut app,
            vec![view(7, "Tower", false), view(8, "Hall", false)],
        );
        assert_eq!(app.shared_views.len(), 2);
        // Renamed: replaced in place, not added.
        send(&mut app, vec![view(7, "Tall tower", false)]);
        assert_eq!(app.shared_views.len(), 2);
        assert_eq!(app.shared_views[0].name, "Tall tower");
        // Removed (made private): gone.
        send(&mut app, vec![view(8, "Hall", true)]);
        assert_eq!(app.shared_views.len(), 1);
        // My own views never show up as someone else's.
        let mut mine = view(42, "Mine", false).to_bookmark();
        mine.id = 42;
        mine.public = true;
        app.bookmarks.push(mine);
        send(&mut app, vec![view(42, "Mine", false)]);
        assert_eq!(app.shared_views.len(), 1);
    }
}
