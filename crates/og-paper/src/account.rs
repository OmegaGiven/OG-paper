// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! An OG Studio account in the native apps: optional, so pages can be kept on
//! the server and recovered on any device, with a copy on this one as well.
//!
//! Signing in happens on the website (see `hosted/src/oauth.js` and
//! docs/NATIVE-SIGN-IN.md). The app makes a PKCE pair, opens the system browser
//! and keeps asking the server until the person has agreed there; it never sees
//! their password or their Google, GitHub or Apple sign-in. App and web page
//! show the same short code, so nobody can get a person to approve someone
//! else's sign-in. Tokens stay in the Keychain on Apple systems and in a file
//! only this user can read elsewhere.
//!
//! An account page is a hosted room, which is a relay, so opening one is the
//! same as joining any relay link and the copy on this device works as usual.
//! The web app has its own account panel, and the Android build has no TLS, so
//! neither has this.

use crate::App;

/// What the UI asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    SignIn,
    Cancel,
    SignOut,
    Refresh,
    Open(usize),
    /// Put the open canvas into the account.
    Keep,
    /// Delete the account, on the website.
    Delete,
    NotNow,
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Account;

    impl App {
        pub(crate) fn account_op(&mut self, _: Op) {}
        pub(crate) fn account_tick(&mut self) {}
        pub(crate) fn account_waiting(&self) -> bool {
            false
        }
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
mod imp {
    use super::*;
    use crate::seal::{b64, random_secret, Keys};
    use crate::ui::AccountPage;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const SITE: &str = "https://ogstudio.org";
    const API: &str = "/paper/api";
    const CLIENT: &str = "og-paper-app";
    const ASK_EVERY: Duration = Duration::from_secs(3);
    const ASK_FOR: Duration = Duration::from_secs(600);
    /// Refresh this long before an access token runs out.
    const EARLY_MS: u64 = 60_000;

    fn site() -> String {
        std::env::var("OGP_ACCOUNT_SITE").unwrap_or_else(|_| SITE.to_string())
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64)
    }

    fn device() -> &'static str {
        if cfg!(target_os = "ios") {
            "iPhone or iPad"
        } else if cfg!(target_os = "macos") {
            "Mac"
        } else if cfg!(target_os = "windows") {
            "Windows"
        } else {
            "Linux"
        }
    }

    /// What app and sign-in page both show (`pairCode` in oauth.js).
    pub(super) fn pair_code(challenge: &str) -> String {
        challenge
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(6)
            .collect::<String>()
            .to_ascii_uppercase()
    }

    pub(super) fn challenge_of(verifier: &str) -> String {
        b64(&Sha256::digest(verifier.as_bytes()))
    }

    /// Where the browser goes to sign in.
    pub(super) fn authorize_url(site: &str, challenge: &str, device: &str) -> String {
        format!(
            "{site}{API}/oauth/authorize?client_id={CLIENT}&code_challenge={challenge}\
             &code_challenge_method=S256&device={}",
            device.replace(' ', "%20")
        )
    }

    #[derive(Clone, Debug, PartialEq)]
    pub(super) struct Tokens {
        pub access: String,
        pub refresh: String,
        /// When the access token runs out (ms since 1970).
        pub expires: u64,
        pub name: String,
    }

    impl Tokens {
        pub(super) fn encode(&self) -> String {
            format!(
                "{}\n{}\n{}\n{}",
                self.access, self.refresh, self.expires, self.name
            )
        }

        pub(super) fn decode(text: &str) -> Option<Tokens> {
            let mut l = text.splitn(4, '\n');
            Some(Tokens {
                access: l.next()?.to_string(),
                refresh: l.next()?.to_string(),
                expires: l.next()?.parse().ok()?,
                name: l.next().unwrap_or("").to_string(),
            })
        }

        fn from_answer(v: &Value, name: String) -> Option<Tokens> {
            Some(Tokens {
                access: v["access_token"].as_str()?.to_string(),
                refresh: v["refresh_token"].as_str()?.to_string(),
                expires: now_ms() + v["expires_in"].as_u64()? * 1000,
                name,
            })
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    mod store {
        use security_framework::passwords::{
            delete_generic_password, get_generic_password, set_generic_password,
        };
        const SERVICE: &str = "com.omegagiven.ogpaper.account";
        const NAME: &str = "tokens";

        pub fn load() -> Option<String> {
            String::from_utf8(get_generic_password(SERVICE, NAME).ok()?).ok()
        }
        pub fn save(text: &str) {
            if let Err(e) = set_generic_password(SERVICE, NAME, text.as_bytes()) {
                log::warn!("keychain: {e}");
            }
        }
        pub fn clear() {
            let _ = delete_generic_password(SERVICE, NAME);
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    mod store {
        use std::path::PathBuf;

        fn path() -> Option<PathBuf> {
            #[cfg(test)]
            return Some(std::env::temp_dir().join(format!(
                "ogp-test-account-{}-{}.txt",
                std::process::id(),
                std::thread::current()
                    .name()
                    .unwrap_or("other")
                    .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
            )));
            #[cfg(not(test))]
            Some(crate::fonts_dir()?.parent()?.join("account.txt"))
        }

        pub fn load() -> Option<String> {
            std::fs::read_to_string(path()?).ok()
        }

        pub fn save(text: &str) {
            let Some(p) = path() else { return };
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::remove_file(&p);
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
            if let Ok(mut f) = o.open(&p) {
                let _ = std::io::Write::write_all(&mut f, text.as_bytes());
            }
        }

        pub fn clear() {
            if let Some(p) = path() {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    /// The tokens in use. Held while refreshing, so only one refresh runs.
    static TOKENS: Mutex<Option<Tokens>> = Mutex::new(None);

    fn signed_in() -> Option<Tokens> {
        let mut t = TOKENS.lock().ok()?;
        if t.is_none() {
            *t = store::load().as_deref().and_then(Tokens::decode);
        }
        t.clone()
    }

    fn remember(t: Option<Tokens>) {
        match &t {
            Some(t) => store::save(&t.encode()),
            None => store::clear(),
        }
        if let Ok(mut g) = TOKENS.lock() {
            *g = t;
        }
    }

    /// An HTTP answer: status and body.
    fn http(method: &str, url: &str, bearer: Option<&str>, json: Option<&Value>, form: Option<&[(&str, &str)]>) -> Result<(u16, Value), String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(20)))
            .build()
            .into();
        let auth = bearer.map(|t| format!("Bearer {t}"));
        let answer = if method == "GET" {
            let mut r = agent.get(url);
            if let Some(a) = &auth {
                r = r.header("Authorization", a);
            }
            r.call()
        } else {
            let mut r = match method {
                "DELETE" => agent.delete(url).force_send_body(),
                _ => agent.post(url),
            };
            if let Some(a) = &auth {
                r = r.header("Authorization", a);
            }
            match (json, form) {
                (_, Some(f)) => r.send_form(f.iter().copied()),
                (Some(j), _) => r.content_type("application/json").send(j.to_string()),
                _ => r.send_empty(),
            }
        };
        let mut res = answer.map_err(|e| e.to_string())?;
        let status = res.status().as_u16();
        let text = res.body_mut().read_to_string().unwrap_or_default();
        Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
    }

    /// A fresh access token, refreshing it if needed. Err(None): signed out.
    fn fresh(force: bool) -> Result<String, Option<String>> {
        let Some(t) = signed_in() else { return Err(None) };
        if !force && t.expires > now_ms() + EARLY_MS {
            return Ok(t.access);
        }
        let mut guard = TOKENS.lock().map_err(|_| None)?;
        // Another thread may have done it while this one waited.
        if let Some(now) = guard.as_ref() {
            if now.refresh != t.refresh {
                return Ok(now.access.clone());
            }
        }
        let url = format!("{}{API}/oauth/token", site());
        let form = [("client_id", CLIENT), ("grant_type", "refresh_token"), ("refresh_token", t.refresh.as_str())];
        let (status, v) = http("POST", &url, None, None, Some(&form)).map_err(Some)?;
        if status == 200 {
            let new = Tokens::from_answer(&v, t.name).ok_or(Some("Bad answer from the server".to_string()))?;
            store::save(&new.encode());
            let access = new.access.clone();
            *guard = Some(new);
            return Ok(access);
        }
        if status >= 500 {
            return Err(Some("The server is having trouble".into()));
        }
        *guard = None;
        store::clear();
        Err(None)
    }

    /// A call to the API as this account; one retry on an expired token.
    fn api(method: &str, path: &str, body: Option<&Value>) -> Result<Value, Done> {
        let url = format!("{}{API}{path}", site());
        let lost = |e: Option<String>| e.map_or(Done::SignedOut, Done::Failed);
        let mut token = fresh(false).map_err(lost)?;
        for last in [false, true] {
            let (status, v) = http(method, &url, Some(&token), body, None).map_err(Done::Failed)?;
            if status == 401 && !last {
                token = fresh(true).map_err(lost)?;
                continue;
            }
            if status == 401 {
                remember(None);
                return Err(Done::SignedOut);
            }
            if status >= 400 {
                let why = v["error"].as_str().unwrap_or("That didn't work").to_string();
                return Err(Done::Failed(why));
            }
            return Ok(v);
        }
        Err(Done::SignedOut)
    }

    /// The token to send when connecting to this address, if it is the account's.
    pub fn bearer_for(ws_url: &str) -> Option<String> {
        let base = site();
        let host = base.split_once("://")?.1;
        let target = ws_url.split_once("://")?.1;
        if target.starts_with(&format!("{host}/")) {
            fresh(false).ok()
        } else {
            None
        }
    }

    pub enum Done {
        SignedIn(String),
        SignedOut,
        Failed(String),
        Pages(Vec<(String, AccountPage)>),
        /// A page's link, and whether the open canvas goes up into it.
        Link(String, bool),
    }

    #[derive(Default)]
    pub struct Account {
        rx: Option<Receiver<Done>>,
        tx: Option<Sender<Done>>,
        waiting: Option<Arc<AtomicBool>>,
        jobs: usize,
        started: bool,
        ids: Vec<String>,
    }

    impl Account {
        fn tx(&mut self) -> Sender<Done> {
            if self.tx.is_none() {
                let (tx, rx) = channel();
                self.tx = Some(tx);
                self.rx = Some(rx);
            }
            self.tx.clone().expect("just made")
        }

        fn run(&mut self, job: impl FnOnce() -> Done + Send + 'static) {
            let tx = self.tx();
            self.jobs += 1;
            std::thread::spawn(move || {
                let _ = tx.send(job());
            });
        }
    }

    pub(super) fn page_list() -> Done {
        let v = match api("GET", "/pages", None) {
            Ok(v) => v,
            Err(d) => return d,
        };
        let mut out = vec![];
        for p in ["mine", "shared"].iter().filter_map(|k| v[*k].as_array()).flatten() {
            let (Some(id), Some(name)) = (p["id"].as_str(), p["name"].as_str()) else {
                continue;
            };
            out.push((
                id.to_string(),
                AccountPage {
                    name: name.to_string(),
                    role: p["role"].as_str().unwrap_or("").to_string(),
                },
            ));
        }
        Done::Pages(out)
    }

    fn link_of(v: &Value) -> Option<String> {
        v["open"]["link"].as_str().or(v["link"].as_str()).map(String::from)
    }

    /// Ask the server until the person has agreed in the browser.
    pub(super) fn wait_for_browser(verifier: String, stop: Arc<AtomicBool>) -> Done {
        let url = format!("{}{API}/oauth/token", site());
        let until = now_ms() + ASK_FOR.as_millis() as u64;
        while now_ms() < until {
            std::thread::sleep(ASK_EVERY);
            if stop.load(Ordering::Relaxed) {
                return Done::Failed(String::new());
            }
            let form = [("client_id", CLIENT), ("grant_type", "poll"), ("code_verifier", verifier.as_str())];
            // The network coming and going is normal here: a phone was just in Safari.
            let Ok((status, v)) = http("POST", &url, None, None, Some(&form)) else {
                continue;
            };
            if status == 200 {
                let Some(t) = Tokens::from_answer(&v, String::new()) else {
                    return Done::Failed("Bad answer from the server".into());
                };
                remember(Some(t.clone()));
                let name = api("GET", "/me", None)
                    .ok()
                    .and_then(|me| me["user"]["name"].as_str().map(String::from))
                    .unwrap_or_default();
                remember(Some(Tokens { name: name.clone(), ..t }));
                return Done::SignedIn(name);
            }
            match v["error"].as_str() {
                Some("authorization_pending") | None => {}
                Some("access_denied") => return Done::Failed("Sign-in was cancelled".into()),
                Some(_) => return Done::Failed("Sign-in didn't finish. Try again.".into()),
            }
        }
        Done::Failed("Sign-in timed out. Try again.".into())
    }

    impl App {
        fn account_stop_waiting(&mut self) {
            if let Some(s) = self.account.waiting.take() {
                s.store(true, Ordering::Relaxed);
            }
            self.ui.account.pair = None;
        }

        pub(crate) fn account_op(&mut self, op: Op) {
            match op {
                Op::SignIn => {
                    self.account_stop_waiting();
                    let verifier = b64(&random_secret());
                    let challenge = challenge_of(&verifier);
                    let url = authorize_url(&site(), &challenge, device());
                    self.ui.account.pair = Some(pair_code(&challenge));
                    self.ui.account.prompt = false;
                    self.ui.account.state.clear();
                    self.account_not_now();
                    if let Err(e) = webbrowser::open(&url) {
                        self.account_stop_waiting();
                        self.say(format!("Couldn't open the browser: {e}"));
                        return;
                    }
                    let stop = Arc::new(AtomicBool::new(false));
                    self.account.waiting = Some(stop.clone());
                    self.account.run(move || wait_for_browser(verifier, stop));
                }
                Op::Cancel => self.account_stop_waiting(),
                Op::SignOut => {
                    let token = signed_in().map(|t| t.access);
                    remember(None);
                    self.ui.account.name = None;
                    self.ui.account.pages.clear();
                    self.account.ids.clear();
                    if let Some(token) = token {
                        let url = format!("{}{API}/oauth/revoke", site());
                        std::thread::spawn(move || {
                            let _ = http("POST", &url, Some(&token), None, None);
                        });
                    }
                    self.say("Signed out. Pages stay on this device.");
                }
                Op::Refresh => {
                    if self.ui.account.name.is_some() {
                        self.account.run(page_list);
                    }
                }
                Op::Open(i) => {
                    let Some(id) = self.account.ids.get(i).cloned() else { return };
                    self.account.run(move || match api("POST", &format!("/pages/{id}/open"), None) {
                        Ok(v) => link_of(&v).map_or(Done::Failed("Bad answer from the server".into()), |l| Done::Link(l, false)),
                        Err(d) => d,
                    });
                }
                Op::Keep => {
                    let name = self.ui.local_pages.iter().find(|p| p.current).map_or("Untitled".to_string(), |p| p.name.clone());
                    let edit = Keys::edit_token(&random_secret());
                    let Some(keys) = Keys::parse(&edit) else { return };
                    let canvas: String = random_secret()[..16].iter().map(|b| format!("{b:02x}")).collect();
                    let room: String = keys.room().iter().map(|b| format!("{b:02x}")).collect();
                    let body = json!({"name": name, "canvas": canvas, "edit": edit, "view": keys.view_token(), "room": room});
                    self.account.run(move || match api("POST", "/pages", Some(&body)) {
                        Ok(v) => link_of(&v).map_or(Done::Failed("Bad answer from the server".into()), |l| Done::Link(l, true)),
                        Err(d) => d,
                    });
                }
                Op::Delete => {
                    let url = format!("{}/account/?delete=1", site());
                    if let Err(e) = webbrowser::open(&url) {
                        self.say(format!("Couldn't open the browser: {e}"));
                    }
                }
                Op::NotNow => {
                    self.ui.account.prompt = false;
                    self.account_not_now();
                }
            }
        }

        fn account_not_now(&mut self) {
            let mut prefs = crate::prefs::load();
            prefs.insert("account.prompted".into(), "1".into());
            crate::prefs::save(&prefs);
        }

        pub(crate) fn account_tick(&mut self) {
            if !self.account.started {
                self.account.started = true;
                self.ui.account.available = true;
                match signed_in() {
                    Some(t) => self.ui.account.name = Some(t.name),
                    None => self.ui.account.prompt = !crate::prefs::load().contains_key("account.prompted"),
                }
                self.account_test_scene();
            }
            let done: Vec<Done> = self.account.rx.as_ref().map_or(vec![], |r| r.try_iter().collect());
            for d in done {
                self.account.jobs = self.account.jobs.saturating_sub(1);
                match d {
                    Done::SignedIn(name) => {
                        self.account_stop_waiting();
                        self.say(format!("Signed in as {name}"));
                        self.ui.account.name = Some(name);
                        self.account.run(page_list);
                        // Saving what's open is what signing in was for.
                        if self.net.is_none() && !self.scene.strokes.is_empty() {
                            self.account_op(Op::Keep);
                        }
                    }
                    Done::SignedOut => {
                        self.ui.account.name = None;
                        self.ui.account.pages.clear();
                        self.account.ids.clear();
                        self.say("Signed out of your account. Sign in again to reach its pages.");
                    }
                    Done::Failed(why) => {
                        self.account.waiting = None;
                        self.ui.account.pair = None;
                        if !why.is_empty() {
                            self.ui.account.state = why.clone();
                            self.say(why);
                        }
                    }
                    Done::Pages(list) => {
                        let (ids, pages) = list.into_iter().unzip();
                        self.account.ids = ids;
                        self.ui.account.pages = pages;
                    }
                    Done::Link(link, false) => self.join(&link),
                    Done::Link(link, true) => {
                        self.join_upload(&link);
                        self.say("Saved to your account, and kept on this device");
                        self.account.run(page_list);
                    }
                }
            }
        }

        pub(crate) fn account_waiting(&self) -> bool {
            self.account.jobs > 0
        }

        /// `OGP_TEST_ACCOUNT=pairing|pages|signed-in`, `OGP_TEST_SIZE=390x844`:
        /// for screenshots of these states (tests/account/shots.sh).
        fn account_test_scene(&mut self) {
            if let Some((w, h)) = std::env::var("OGP_TEST_SIZE").ok().as_deref().and_then(|s| s.split_once('x')) {
                if let (Some(win), Ok(w), Ok(h)) = (&self.window, w.parse::<u32>(), h.parse::<u32>()) {
                    let _ = win.request_inner_size(winit::dpi::PhysicalSize::new(w, h));
                }
            }
            match std::env::var("OGP_TEST_ACCOUNT").as_deref() {
                Ok("pairing") => self.ui.account.pair = Some("K7QX2M".into()),
                Ok("pages") => self.ui.pages_open = true,
                Ok("signed-in") => {
                    self.ui.pages_open = true;
                    self.ui.account.prompt = false;
                    self.ui.account.name = Some("alice".into());
                    self.ui.account.pages = ["Trip notes", "Kitchen plan", "Team board"]
                        .iter()
                        .enumerate()
                        .map(|(i, n)| AccountPage { name: n.to_string(), role: if i == 2 { "editor" } else { "owner" }.into() })
                        .collect();
                }
                _ => {}
            }
        }
    }
}

pub use imp::Account;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub use imp::bearer_for;

#[cfg(all(test, not(any(target_arch = "wasm32", target_os = "android"))))]
mod tests {
    use super::imp::*;

    #[test]
    fn pair_code_matches_the_server() {
        // Same vector as hosted/test/appsignin.test.js.
        assert_eq!(pair_code("a-b_c1d2e3f4"), "ABC1D2");
    }

    #[test]
    fn challenge_is_base64url_sha256() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge_of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn the_sign_in_link_stays_under_the_sites_limit() {
        let url = authorize_url("https://ogstudio.org", &"c".repeat(43), "iPhone or iPad");
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("device=iPhone%20or%20iPad"));
        let next = url.trim_start_matches("https://ogstudio.org");
        assert!(next.len() < 300);
    }

    #[test]
    fn tokens_survive_being_stored() {
        let t = Tokens { access: "a".into(), refresh: "r".into(), expires: 42, name: "sam two".into() };
        assert_eq!(Tokens::decode(&t.encode()), Some(t));
        assert_eq!(Tokens::decode("only one line"), None);
    }

    /// The whole thing against a real hosted server: tests/account/run-e2e.sh.
    /// Sign in the way a browser would, keep a page, then get it back on a
    /// second device that has nothing on it.
    #[test]
    #[ignore = "needs a hosted server"]
    fn a_page_kept_in_the_account_comes_back_on_a_new_device() {
        use crate::App;
        use serde_json::json;
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        let site = std::env::var("OGP_ACCOUNT_SITE").expect("OGP_ACCOUNT_SITE");
        let pump = |app: &mut App, secs: u64, done: &dyn Fn(&App) -> bool| {
            let until = Instant::now() + Duration::from_secs(secs);
            while Instant::now() < until && !done(app) {
                app.account_tick();
                app.net_tick();
                std::thread::sleep(Duration::from_millis(30));
            }
            done(app)
        };

        // The browser, signed in on the site as alice.
        let verifier = crate::seal::b64(&crate::seal::random_secret());
        let challenge = challenge_of(&verifier);
        let url = authorize_url(&site, &challenge, "Test device");
        let code = pair_code(&challenge);
        let origin = site.clone();
        let browser = std::thread::spawn(move || {
            let page = ureq::get(&url)
                .header("Cookie", "dev_user=alice")
                .call()
                .expect("the sign-in page")
                .body_mut()
                .read_to_string()
                .unwrap();
            assert!(page.contains(&code), "the page shows the app's code");
            let form = [
                ("client_id", "og-paper-app"),
                ("code_challenge", challenge.as_str()),
                ("code_challenge_method", "S256"),
                ("device", "Test device"),
                ("choice", "allow"),
            ];
            let done = ureq::post(format!("{origin}/paper/api/oauth/authorize"))
                .header("Cookie", "dev_user=alice")
                .header("Origin", &origin)
                .send_form(form)
                .expect("agreeing")
                .body_mut()
                .read_to_string()
                .unwrap();
            assert!(done.contains("signed in"));
        });
        let name = match wait_for_browser(verifier, Arc::new(AtomicBool::new(false))) {
            Done::SignedIn(n) => n,
            Done::Failed(why) => panic!("sign-in failed: {why}"),
            _ => panic!("sign-in failed"),
        };
        browser.join().unwrap();
        assert_eq!(name, "alice");

        // This device: draw, and keep the page.
        let mut phone = App::new(None);
        phone.account_tick();
        assert_eq!(phone.ui.account.name.as_deref(), Some("alice"));
        assert!(!phone.ui.account.prompt);
        let r = phone.run_commands(&json!([{"add": "text", "x": 0, "y": 0, "text": "Recover me", "size": 30}]));
        assert_eq!(r[0]["ok"], true, "{r}");
        phone.account_op(crate::account::Op::Keep);
        assert!(
            pump(&mut phone, 20, &|a: &App| !a.ui.account.pages.is_empty()),
            "the kept page is listed: {}",
            phone.ui.account.state
        );
        // Long enough for the change to reach the room.
        pump(&mut phone, 5, &|_| false);
        assert_eq!(phone.ui.account.pages[0].role, "owner");
        drop(phone);

        // A new device with nothing on it: no files, no prefs, so the page can
        // only come from the server.
        let empty = std::env::temp_dir().join(format!("ogp-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        std::env::set_var("HOME", &empty);
        crate::prefs::save(&Default::default());
        let mut tablet = App::new(None);
        assert!(tablet.texts_at(&tablet.cam).is_empty());
        tablet.account_tick();
        tablet.account_op(crate::account::Op::Refresh);
        assert!(pump(&mut tablet, 20, &|a: &App| !a.ui.account.pages.is_empty()), "pages listed");
        tablet.account_op(crate::account::Op::Open(0));
        let has = |a: &App| a.texts_at(&a.cam).iter().any(|t| t.to_string().contains("Recover me"));
        assert!(pump(&mut tablet, 30, &has), "the page came back");
    }
}
