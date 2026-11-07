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
/// Servers whose set the person turned down (space-separated link URLs).
const DECLINED_KEY: &str = "profile.declined";

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

/// The set in use: "" this device's, else a server's link URL. (Kept in
/// memory once read: the settings menu asks every frame.)
pub fn in_use() -> String {
    IN_USE.with(|c| {
        c.borrow_mut()
            .get_or_insert_with(|| {
                crate::prefs::load()
                    .get(USE_KEY)
                    .cloned()
                    .unwrap_or_default()
            })
            .clone()
    })
}

thread_local! {
    // (Per thread: the app is on one; tests each have their own prefs.)
    static IN_USE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn set_in_use(url: &str) {
    let mut p = crate::prefs::load();
    p.insert(USE_KEY.into(), url.into());
    crate::prefs::save(&p);
    IN_USE.with(|c| *c.borrow_mut() = Some(url.into()));
}

/// Servers whose set the person turned down for this device's, so a later
/// sign-in with a saved token doesn't switch back by itself.
fn declined(url: &str) -> bool {
    crate::prefs::load()
        .get(DECLINED_KEY)
        .is_some_and(|d| d.split(' ').any(|u| u == url))
}

fn set_declined(url: &str, on: bool) {
    let mut p = crate::prefs::load();
    let mut d: Vec<String> = p
        .get(DECLINED_KEY)
        .map(|d| {
            d.split(' ')
                .filter(|u| !u.is_empty() && *u != url)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    if on {
        d.push(url.into());
    }
    p.insert(DECLINED_KEY.into(), d.join(" "));
    crate::prefs::save(&p);
}

/// Keep this device's own set aside (it is the one in use now).
fn keep_local() {
    let mut p = crate::prefs::load();
    p.insert(LOCAL_KEY.into(), escape(&current()));
    crate::prefs::save(&p);
}

impl App {
    /// The tools and settings on this device were replaced: show them.
    fn profile_reload(&mut self) {
        self.ui.load_saved(crate::hotbar::load());
        crate::apply_prefs(&mut self.ui, &crate::prefs::load());
        self.redraw();
    }

    /// Changes made here that haven't gone to the server yet.
    fn profile_unsent(&self) -> bool {
        self.profile_sent.as_deref().is_some_and(|s| s != current())
    }

    /// Server `url`'s account set arrived (empty: it keeps none yet).
    pub(crate) fn profile_arrived(&mut self, i: usize, text: String) {
        let Some(s) = self.servers.get(i) else { return };
        let url = s.link.url.clone();
        let here = in_use();
        let empty = text.trim().is_empty();
        if here == url {
            if empty || self.profile_unsent() {
                // It has none, or changes made here haven't reached it
                // yet: send ours (by `profile_tick` once this connection
                // is done: a second one now would trip over it).
                self.profile_sent = None;
                self.profile_checked = web_time::Instant::now() - std::time::Duration::from_secs(2);
            } else if text != current() && write(&text) {
                // Changed on another device: take it.
                self.profile_sent = Some(current());
                self.profile_reload();
            }
            return;
        }
        // Switch when asked to (the menu, or signing in with a password
        // just now), or on a sign-in with a saved token to an account that
        // has a set, unless the person chose this device's over it.
        let asked = self.profile_want.as_deref() == Some(url.as_str());
        if !asked && (!here.is_empty() || empty || declined(&url)) {
            return;
        }
        self.profile_want = None;
        set_declined(&url, false);
        if here.is_empty() {
            keep_local();
        }
        if empty {
            // A new account: it takes the set in use, and uses it.
            set_in_use(&url);
            self.profile_sent = None;
            self.profile_checked = web_time::Instant::now() - std::time::Duration::from_secs(2);
            return;
        }
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
            // Back to this device's own set (kept aside when it was left).
            let mut p = crate::prefs::load();
            if let Some(t) = p.remove(LOCAL_KEY).map(|s| unescape(&s)) {
                crate::prefs::save(&p);
                write(&t);
            }
            set_in_use("");
            // Not switched back by itself at the next sign-in.
            set_declined(&here, true);
            self.profile_sent = None;
            self.profile_want = None;
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
        if self.servers[i].busy() {
            self.say("Busy with that server: try again in a moment");
            return;
        }
        // Switched when its set arrives (`profile_arrived`).
        self.profile_want = Some(url.to_string());
        self.server_send(
            i,
            vec![crate::wire::Msg::GetSettings, crate::wire::Msg::ListPages],
        );
    }

    /// Signed out of (or forgot) server `url`: if its set is in use, back
    /// to this device's.
    pub(crate) fn profile_signed_out(&mut self, url: &str) {
        if self.profile_want.as_deref() == Some(url) {
            self.profile_want = None;
        }
        if in_use() == url {
            self.profile_use("");
            // Signing out isn't turning the account's set down.
            set_declined(url, false);
        }
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

    fn dark() -> String {
        crate::prefs::load().get("dark").cloned().unwrap_or_default()
    }

    fn set_dark(v: &str) {
        let mut p = crate::prefs::load();
        p.insert("dark".into(), v.into());
        crate::prefs::save(&p);
    }

    /// A set like this device's, with dark mode `v`.
    fn set_with_dark(v: &str) -> String {
        let now = current();
        let mut out = String::new();
        for l in now.lines() {
            if !l.starts_with("pref dark=") {
                out += l;
                out.push('\n');
            }
            if l == HEAD {
                out += &format!("pref dark={v}\n");
            }
        }
        out
    }

    const A: &str = "ws://127.0.0.1:1/a";
    const B: &str = "ws://127.0.0.1:1/b";

    fn app() -> App {
        let mut app = App::new(None);
        app.servers = vec![
            crate::pages::ServerConn::for_test(A, Some("ann")),
            crate::pages::ServerConn::for_test(B, Some("ann")),
        ];
        app
    }

    #[test]
    fn choosing_this_device_lasts_and_gets_its_own_set_back() {
        set_dark("on");
        let mut app = app();
        // Signed in with a saved token to an account that has a set.
        app.profile_arrived(0, set_with_dark("off"));
        assert_eq!((in_use().as_str(), dark().as_str()), (A, "off"));
        // This device's, please: its own set comes back, kept no more.
        app.profile_use("");
        assert_eq!((in_use().as_str(), dark().as_str()), ("", "on"));
        assert!(!crate::prefs::load().contains_key(LOCAL_KEY));
        // The next sign-in (launch, Pages) doesn't switch by itself.
        app.profile_arrived(0, set_with_dark("off"));
        assert_eq!((in_use().as_str(), dark().as_str()), ("", "on"));
        // Chosen again in the menu: it switches.
        app.profile_use(A);
        app.profile_arrived(0, set_with_dark("off"));
        assert_eq!((in_use().as_str(), dark().as_str()), (A, "off"));
    }

    #[test]
    fn going_from_one_account_to_another_keeps_this_devices_set() {
        set_dark("on");
        let mut app = app();
        app.profile_arrived(0, set_with_dark("off"));
        assert_eq!(in_use(), A);
        app.servers[0].state.clear();
        app.profile_use(B);
        // Until B answers, A's set stays.
        assert_eq!(in_use(), A);
        app.profile_arrived(1, set_with_dark("auto"));
        assert_eq!((in_use().as_str(), dark().as_str()), (B, "auto"));
        app.profile_use("");
        assert_eq!(dark(), "on", "this device's own set, not A's");
    }

    #[test]
    fn a_new_account_takes_the_set_in_use_and_an_old_aside_is_not_restored() {
        set_dark("on");
        let mut app = app();
        // An old set kept aside from long ago.
        let mut p = crate::prefs::load();
        p.insert(LOCAL_KEY.into(), escape(&set_with_dark("off")));
        crate::prefs::save(&p);
        // A token sign-in to an empty account: nothing changes.
        app.profile_arrived(0, String::new());
        assert_eq!(in_use(), "");
        // Signing in on purpose: it takes this device's set.
        app.profile_want = Some(A.into());
        app.profile_arrived(0, String::new());
        assert_eq!(in_use(), A);
        app.profile_use("");
        assert_eq!(dark(), "on", "the set from just now, not the old one");
    }

    #[test]
    fn a_change_not_sent_yet_is_not_undone_by_the_server() {
        set_dark("on");
        let mut app = app();
        app.profile_arrived(0, set_with_dark("off"));
        set_dark("on"); // changed here; not sent yet
        app.profile_arrived(0, set_with_dark("off"));
        assert_eq!(dark(), "on");
        assert!(app.profile_sent.is_none(), "it goes to the server next");
    }

    #[test]
    fn signing_out_goes_back_to_this_device() {
        set_dark("on");
        let mut app = app();
        app.profile_arrived(0, set_with_dark("off"));
        app.server_account_out(0);
        assert_eq!((in_use().as_str(), dark().as_str()), ("", "on"));
        assert!(!declined(A), "signing out isn't turning it down");
    }
}
