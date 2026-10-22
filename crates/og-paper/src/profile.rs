// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Tools and settings that follow a person's account: the toolbars,
//! inventory and saved views (`hotbar`), and the settings that are about
//! how they like to work (grid, rulers, dark mode, hints, diagram mode,
//! hotkeys, swatches…), but not the ones about this screen (where the
//! buttons sit, the interface size).
//!
//! Signed in to a server's account (Pages), the app asks it for the
//! account's set and switches to it, keeping this device's own set aside;
//! a new account takes this device's set. While a server's set is in use,
//! changes go back to it, so the next browser or device signing in gets
//! them. UI settings › Tools & settings switches between this device's
//! set and each server's.

use crate::App;

/// Most a set may be (a server refuses bigger).
pub const MAX_BYTES: usize = 1 << 20;

/// The settings that travel (the rest stay with the device).
const SYNCED: [&str; 12] = [
    "basictools",
    "dark",
    "diagram",
    "grid",
    "hints",
    "invviews",
    "keys",
    "radialbar",
    "rulers",
    "spell",
    "spellwords",
    "swatches",
];

/// Which set is in use: "" this device's, else a server's link URL.
const USE_KEY: &str = "profile.use";
/// This device's own set while a server's is in use.
const LOCAL_KEY: &str = "profile.local";

const HEAD: &str = "og-paper-profile 1";

/// The set as it is now (prefs and toolbars on this device).
pub fn current() -> String {
    let p = crate::prefs::load();
    let mut out = format!("{HEAD}\n");
    for k in SYNCED {
        if let Some(v) = p.get(k) {
            out += &format!("pref {k}={v}\n");
        }
    }
    out += "hotbar\n";
    out += &crate::hotbar::encode(&crate::hotbar::load());
    out
}

/// Make `text` the set on this device (prefs and toolbars). False when it
/// isn't a set.
pub fn write(text: &str) -> bool {
    let Some(rest) = text.strip_prefix(HEAD) else {
        return false;
    };
    let (prefs, hotbar) = match rest.split_once("\nhotbar\n") {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    let mut p = crate::prefs::load();
    for k in SYNCED {
        p.remove(k);
    }
    for line in prefs.lines() {
        if let Some((k, v)) = line.strip_prefix("pref ").and_then(|l| l.split_once('=')) {
            if SYNCED.contains(&k) {
                p.insert(k.into(), v.into());
            }
        }
    }
    crate::prefs::save(&p);
    if let Some(saved) = hotbar.and_then(crate::hotbar::decode) {
        crate::hotbar::save(&saved);
    }
    crate::swatches::forget();
    true
}

/// Prefs hold one line per value: a set's lines escaped.
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut c = s.chars();
    while let Some(ch) = c.next() {
        if ch == '\\' {
            match c.next() {
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// The set in use: "" this device's, else a server's link URL.
pub fn in_use() -> String {
    crate::prefs::load()
        .get(USE_KEY)
        .cloned()
        .unwrap_or_default()
}

fn set_in_use(url: &str) {
    let mut p = crate::prefs::load();
    p.insert(USE_KEY.into(), url.into());
    crate::prefs::save(&p);
}

impl App {
    /// The tools and settings on this device were replaced: show them.
    fn profile_reload(&mut self) {
        self.ui.load_saved(crate::hotbar::load());
        crate::apply_prefs(&mut self.ui, &crate::prefs::load());
        self.redraw();
    }

    /// Server `url`'s account set arrived (empty: it keeps none yet).
    pub(crate) fn profile_arrived(&mut self, i: usize, text: String) {
        let Some(s) = self.servers.get(i) else { return };
        let url = s.link.url.clone();
        let here = in_use();
        if text.trim().is_empty() {
            // A new account: it takes this device's set, and uses it.
            // (Sent by `profile_tick` once this connection is done: a
            // second one now would trip over it.)
            if here.is_empty() {
                set_in_use(&url);
                self.profile_sent = None;
                self.profile_checked = web_time::Instant::now() - std::time::Duration::from_secs(2);
            }
            return;
        }
        if here == url {
            // Changed on another device: take it.
            if text != current() && write(&text) {
                self.profile_sent = Some(current());
                self.profile_reload();
            }
            return;
        }
        if !here.is_empty() {
            // Another server's set is in use: leave it (the menu switches).
            return;
        }
        // First sign-in here: keep this device's set aside, take theirs.
        let mut p = crate::prefs::load();
        p.insert(LOCAL_KEY.into(), escape(&current()));
        crate::prefs::save(&p);
        if write(&text) {
            set_in_use(&url);
            self.profile_sent = Some(current());
            self.profile_reload();
            self.say(format!(
                "Your tools and settings from {}",
                crate::pages::host_label(&url)
            ));
        }
    }

    /// Send `text` to server `i` to keep for the signed-in account.
    fn profile_push_to(&mut self, i: usize, text: String) {
        if text.len() > MAX_BYTES {
            return;
        }
        self.server_send(
            i,
            vec![
                crate::wire::Msg::Settings(text),
                crate::wire::Msg::ListPages,
            ],
        );
    }

    /// Now and then: a server's set in use and changed here goes back to it.
    pub(crate) fn profile_tick(&mut self) {
        // (Reading the prefs is a file read: not every frame.)
        if self.headless || self.profile_checked.elapsed() < std::time::Duration::from_secs(3) {
            return;
        }
        self.profile_checked = web_time::Instant::now();
        let url = in_use();
        if url.is_empty() {
            return;
        }
        let now = current();
        if self.profile_sent.as_deref() == Some(now.as_str()) {
            return;
        }
        let Some(i) = self
            .servers
            .iter()
            .position(|s| s.link.url == url && s.signed_in())
        else {
            return;
        };
        if self.servers[i].busy() {
            return;
        }
        self.profile_sent = Some(now.clone());
        self.profile_push_to(i, now);
    }

    /// The sets to choose from: this device's, and each signed-in server's.
    pub(crate) fn profile_choices(&self) -> Vec<(String, String)> {
        let mut v = vec![(String::new(), "This device".to_string())];
        for s in self.servers.iter().filter(|s| s.signed_in()) {
            v.push((
                s.link.url.clone(),
                format!(
                    "{} on {}",
                    s.account_user(),
                    crate::pages::host_label(&s.link.url)
                ),
            ));
        }
        v
    }

    /// Switch to set `url` ("" this device's).
    pub(crate) fn profile_use(&mut self, url: &str) {
        let here = in_use();
        if here == url {
            return;
        }
        if url.is_empty() {
            let local = crate::prefs::load().get(LOCAL_KEY).map(|s| unescape(s));
            if let Some(t) = local.filter(|t| write(t)) {
                let _ = t;
            }
            set_in_use("");
            self.profile_sent = None;
            self.profile_reload();
            self.say("Using this device's tools and settings");
            return;
        }
        let Some(i) = self
            .servers
            .iter()
            .position(|s| s.link.url == url && s.signed_in())
        else {
            return;
        };
        if here.is_empty() {
            let mut p = crate::prefs::load();
            p.insert(LOCAL_KEY.into(), escape(&current()));
            crate::prefs::save(&p);
        }
        // Clear the choice so the server's answer is taken as a first sign-in.
        set_in_use("");
        self.server_send(
            i,
            vec![crate::wire::Msg::GetSettings, crate::wire::Msg::ListPages],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_round_trips_and_keeps_device_settings_out() {
        let mut p = crate::prefs::load();
        p.insert("dark".into(), "on".into());
        p.insert("uiscale".into(), "1.4".into());
        crate::prefs::save(&p);
        let text = current();
        assert!(text.contains("pref dark=on"));
        assert!(
            !text.contains("uiscale"),
            "the interface size stays with the device"
        );
        // Another set: dark off; this device's interface size kept.
        let other = text.replace("pref dark=on\n", "pref dark=off\n");
        assert!(write(&other));
        let p = crate::prefs::load();
        assert_eq!(p.get("dark").map(String::as_str), Some("off"));
        assert_eq!(p.get("uiscale").map(String::as_str), Some("1.4"));
        assert!(!write("not a set"));
        assert_eq!(unescape(&escape("a\\b\nc")), "a\\b\nc");
    }
}
