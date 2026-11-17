// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! An account in the native apps: optional, so pages can be kept on a server
//! and recovered on any device, with a copy on this one as well. Two kinds:
//!
//! - **OG Studio** (ogstudio.org). Signing in happens on the website (see
//!   `hosted/src/oauth.js` and docs/NATIVE-SIGN-IN.md). The app makes a PKCE
//!   pair, opens the system browser and keeps asking the server until the
//!   person has agreed there. App and web page show the same short code.
//! - **A vault** (OG Vault, og-vault docs/APP-LOGIN.md, PROFILES.md and
//!   PAPER.md). The person enters the vault's address or connect link; the
//!   app probes it, opens the vault's own sign-in page with PKCE, and the
//!   vault sends the browser back to a one-time listener on 127.0.0.1 (RFC
//!   8252 7.3) with a code the app swaps for tokens. One profile per vault,
//!   one in use at a time, switchable.
//!
//! Either way the app never sees a password. Tokens stay in the Keychain on
//! Apple systems and in files only this user (or app) can read elsewhere.
//! An account page is a room that speaks the relay protocol, so opening one
//! is the same as joining any relay link and the copy on this device works
//! as usual. The web app has its own account panel (og-hosted.js, og-vault.js).

use crate::App;

/// What the UI asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Sign in to OG Studio.
    SignIn,
    /// Sign in to the vault at the address typed in the account view.
    VaultSignIn,
    Cancel,
    SignOut,
    Refresh,
    Open(usize),
    /// Put the open canvas into the account.
    Keep,
    /// Delete the account, on the website.
    Delete,
    NotNow,
    /// Use saved account `i` of `AccountView::profiles`.
    Switch(usize),
}

#[cfg(target_arch = "wasm32")]
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

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::*;
    use crate::seal::{b64, random_secret, Keys};
    use crate::ui::{AccountPage, AccountProfile};
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const SITE: &str = "https://ogstudio.org";
    const API: &str = "/paper/api";
    const VAULT_API: &str = "/api/v1/paper";
    const CLIENT: &str = "og-paper-app";
    /// The vault's app id for OG Paper (APP-LOGIN.md section 2).
    const VAULT_CLIENT: &str = "og-paper";
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
        } else if cfg!(target_os = "android") {
            "Android"
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

    // ---- vault addresses and profiles --------------------------------------

    /// Percent-encode a query value.
    pub(super) fn enc(s: &str) -> String {
        let mut o = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                o.push(b as char);
            } else {
                o.push_str(&format!("%{b:02X}"));
            }
        }
        o
    }

    pub(super) fn dec(s: &str) -> String {
        let b = s.as_bytes();
        let mut o = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'+' => o.push(b' '),
                b'%' if i + 2 < b.len() => {
                    match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                        Ok(v) => {
                            o.push(v);
                            i += 2;
                        }
                        Err(_) => o.push(b'%'),
                    }
                }
                c => o.push(c),
            }
            i += 1;
        }
        String::from_utf8_lossy(&o).into_owned()
    }

    /// The query of a URL or request target as pairs.
    pub(super) fn query(s: &str) -> Vec<(String, String)> {
        let q = s.split_once('?').map_or("", |x| x.1);
        let q = q.split('#').next().unwrap_or("");
        q.split('&')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (k, v) = p.split_once('=').unwrap_or((p, ""));
                (dec(k), dec(v))
            })
            .collect()
    }

    /// `"vault.ts.net/x"` → `"https://vault.ts.net"`: an origin, https unless
    /// http is written out.
    pub(super) fn normalize(s: &str) -> Option<String> {
        let t = s.trim().trim_end_matches('/');
        if t.is_empty() || t.chars().any(char::is_whitespace) {
            return None;
        }
        let (scheme, rest) = match t.split_once("://") {
            Some((s, r)) if s.eq_ignore_ascii_case("https") || s.eq_ignore_ascii_case("http") => (s.to_ascii_lowercase(), r),
            Some(_) => return None,
            None => ("https".to_string(), t),
        };
        let host = rest.split(['/', '?', '#']).next()?;
        if host.is_empty() || host.contains('@') {
            return None;
        }
        Some(format!("{scheme}://{}", host.to_ascii_lowercase()))
    }

    /// A connect link (`ogvault://connect?u=&id=&n=`, the same on an https
    /// `/connect` URL) or a plain address → (address, vault id if given).
    pub(super) fn parse_connect(text: &str) -> Option<(String, Option<String>)> {
        let t = text.trim();
        let low = t.to_ascii_lowercase();
        let path = t.split('?').next().unwrap_or("");
        if low.starts_with("ogvault://connect") || (low.starts_with("http") && path.trim_end_matches('/').ends_with("/connect") && t.contains("u=")) {
            let q = query(t);
            let get = |k: &str| q.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
            return Some((normalize(&get("u")?)?, get("id").filter(|s| !s.is_empty())));
        }
        if low.starts_with("ogvault:") {
            return None;
        }
        Some((normalize(t)?, None))
    }

    /// A vault this device signed in to (PROFILES.md section 2). Kept in
    /// `vaults.json`; its tokens in the secure store under `vault-<id>`.
    #[derive(Clone, Debug, PartialEq, Default)]
    pub(super) struct Vault {
        pub id: String,
        pub name: String,
        /// Every known origin, the one that answered last first.
        pub addresses: Vec<String>,
        pub user_id: String,
        pub user_name: String,
        pub last_used: u64,
    }

    impl Vault {
        fn label(&self) -> String {
            let who = if self.user_name.is_empty() { "me" } else { &self.user_name };
            let at = if self.name.is_empty() { self.addresses.first().map_or("vault", |a| a.as_str()) } else { &self.name };
            format!("{who} @ {at}")
        }
        fn address(&self) -> String {
            self.addresses.first().cloned().unwrap_or_default()
        }
        fn to_json(&self) -> Value {
            json!({"vault_id": self.id, "name": self.name, "addresses": self.addresses,
                   "user": {"id": self.user_id, "name": self.user_name}, "last_used_at": self.last_used})
        }
        fn from_json(v: &Value) -> Option<Vault> {
            Some(Vault {
                id: v["vault_id"].as_str()?.to_string(),
                name: v["name"].as_str().unwrap_or("").to_string(),
                addresses: v["addresses"].as_array()?.iter().filter_map(|a| a.as_str().map(String::from)).collect(),
                user_id: v["user"]["id"].as_str().unwrap_or("").to_string(),
                user_name: v["user"]["name"].as_str().unwrap_or("").to_string(),
                last_used: v["last_used_at"].as_u64().unwrap_or(0),
            })
        }
    }

    fn vaults() -> Vec<Vault> {
        let text = store::load_file("vaults.json").unwrap_or_default();
        serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v.as_array().map(|a| a.iter().filter_map(Vault::from_json).collect()))
            .unwrap_or_default()
    }

    fn save_vaults(list: &[Vault]) {
        store::save_file("vaults.json", &Value::Array(list.iter().map(Vault::to_json).collect()).to_string());
    }

    fn save_vault(v: &Vault) {
        let mut list = vaults();
        match list.iter_mut().find(|x| x.id == v.id) {
            Some(x) => {
                let mut addrs = v.addresses.clone();
                addrs.extend(x.addresses.iter().filter(|a| !v.addresses.contains(a)).cloned());
                *x = Vault { addresses: addrs, ..v.clone() };
            }
            None => list.push(v.clone()),
        }
        save_vaults(&list);
    }

    /// Which account is in use: OG Studio, or a vault profile.
    #[derive(Clone, Debug, PartialEq)]
    pub(super) enum Backend {
        Studio,
        Vault(Vault),
    }

    impl Backend {
        fn base(&self) -> String {
            match self {
                Backend::Studio => site(),
                Backend::Vault(v) => v.address(),
            }
        }
        fn api(&self) -> &'static str {
            match self {
                Backend::Studio => API,
                Backend::Vault(_) => VAULT_API,
            }
        }
        /// The secure-store name of its tokens.
        fn slot(&self) -> String {
            match self {
                Backend::Studio => "tokens".into(),
                Backend::Vault(v) => format!("vault-{}", v.id.replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "_")),
            }
        }
        fn pref(&self) -> String {
            match self {
                Backend::Studio => "studio".into(),
                Backend::Vault(v) => format!("vault:{}", v.id),
            }
        }
    }

    pub(super) static BACKEND: Mutex<Option<Backend>> = Mutex::new(None);

    fn backend() -> Backend {
        let mut b = BACKEND.lock().unwrap_or_else(|e| e.into_inner());
        if b.is_none() {
            let pref = crate::prefs::load().get("account.backend").cloned().unwrap_or_default();
            *b = Some(match pref.strip_prefix("vault:") {
                Some(id) => vaults().into_iter().find(|v| v.id == id).map_or(Backend::Studio, Backend::Vault),
                None => Backend::Studio,
            });
        }
        b.clone().expect("just set")
    }

    fn use_backend(next: Backend) {
        let mut prefs = crate::prefs::load();
        prefs.insert("account.backend".into(), next.pref());
        crate::prefs::save(&prefs);
        if let Ok(mut t) = TOKENS.lock() {
            *t = None;
        }
        *BACKEND.lock().unwrap_or_else(|e| e.into_inner()) = Some(next);
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

        /// A vault's answer (`/auth/token`, `/auth/refresh`): `access_expires`
        /// in seconds since 1970.
        pub(super) fn from_vault(v: &Value, name: String, old_refresh: &str) -> Option<Tokens> {
            let exp = v["access_expires"].as_u64().map_or(now_ms() + 15 * 60_000, |s| if s < 1_000_000_000_000 { s * 1000 } else { s });
            Some(Tokens {
                access: v["access_token"].as_str()?.to_string(),
                refresh: v["refresh_token"].as_str().unwrap_or(old_refresh).to_string(),
                expires: exp,
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

        pub fn load(name: &str) -> Option<String> {
            String::from_utf8(get_generic_password(SERVICE, name).ok()?).ok()
        }
        pub fn save(name: &str, text: &str) {
            if let Err(e) = set_generic_password(SERVICE, name, text.as_bytes()) {
                log::warn!("keychain: {e}");
            }
        }
        pub fn clear(name: &str) {
            let _ = delete_generic_password(SERVICE, name);
        }
        pub use super::files::{load_file, save_file};
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    mod store {
        // Files only this user can read (on Android, the app's private storage).
        pub fn load(name: &str) -> Option<String> {
            super::files::load_file(&file(name))
        }
        pub fn save(name: &str, text: &str) {
            super::files::save_file(&file(name), text)
        }
        pub fn clear(name: &str) {
            super::files::remove_file(&file(name))
        }
        /// OG Studio's tokens keep their old file name.
        fn file(name: &str) -> String {
            if name == "tokens" { "account.txt".into() } else { format!("{name}.txt") }
        }
        pub use super::files::{load_file, save_file};
    }

    mod files {
        use std::path::PathBuf;

        fn path(name: &str) -> Option<PathBuf> {
            #[cfg(test)]
            return Some(std::env::temp_dir().join(format!(
                "ogp-test-{}-{}-{name}",
                std::process::id(),
                std::thread::current()
                    .name()
                    .unwrap_or("other")
                    .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
            )));
            #[cfg(not(test))]
            Some(crate::fonts_dir()?.parent()?.join(name))
        }

        pub fn load_file(name: &str) -> Option<String> {
            std::fs::read_to_string(path(name)?).ok()
        }

        pub fn save_file(name: &str, text: &str) {
            let Some(p) = path(name) else { return };
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

        #[allow(dead_code)]
        pub fn remove_file(name: &str) {
            if let Some(p) = path(name) {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    /// The tokens in use. Held while refreshing, so only one refresh runs.
    pub(super) static TOKENS: Mutex<Option<Tokens>> = Mutex::new(None);

    fn signed_in() -> Option<Tokens> {
        let slot = backend().slot();
        let mut t = TOKENS.lock().ok()?;
        if t.is_none() {
            *t = store::load(&slot).as_deref().and_then(Tokens::decode);
        }
        t.clone()
    }

    fn remember(t: Option<Tokens>) {
        let slot = backend().slot();
        match &t {
            Some(t) => store::save(&slot, &t.encode()),
            None => store::clear(&slot),
        }
        if let Ok(mut g) = TOKENS.lock() {
            *g = t;
        }
    }

    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(20)))
            .build()
            .into()
    }

    /// An HTTP answer: status and body.
    fn http(method: &str, url: &str, bearer: Option<&str>, json: Option<&Value>, form: Option<&[(&str, &str)]>) -> Result<(u16, Value), String> {
        let agent = agent();
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
                "PATCH" => agent.patch(url),
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

    /// `GET <address>/.well-known/og-vault`: (vault id, name, its own url)
    /// if the address is an OG Vault (PROFILES.md section 1).
    pub(super) fn probe(address: &str) -> Option<(String, String, Option<String>)> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(4)))
            .build()
            .into();
        let mut r = agent.get(format!("{address}/.well-known/og-vault")).call().ok()?;
        if r.status().as_u16() / 100 != 2 {
            return None;
        }
        let v: Value = serde_json::from_str(&r.body_mut().read_to_string().ok()?).ok()?;
        if v["server"].as_str() != Some("og-vault") {
            return None;
        }
        let id = v["vault_id"].as_str().filter(|s| !s.is_empty())?.to_string();
        Some((id, v["name"].as_str().unwrap_or("").to_string(), v["url"].as_str().and_then(normalize)))
    }

    /// A vault profile in use: the first of its addresses that answers as it
    /// (tokens never go to an address that didn't).
    fn reach(v: &Vault) -> Option<Vault> {
        for a in &v.addresses {
            if let Some((id, name, url)) = probe(a) {
                if id == v.id {
                    let mut addrs = vec![a.clone()];
                    addrs.extend(v.addresses.iter().filter(|x| *x != a).cloned());
                    if let Some(u) = url.filter(|u| !addrs.contains(u)) {
                        addrs.push(u);
                    }
                    return Some(Vault { name: if name.is_empty() { v.name.clone() } else { name }, addresses: addrs, ..v.clone() });
                }
            }
        }
        None
    }

    /// A fresh access token, refreshing it if needed. Err(None): signed out.
    fn fresh(force: bool) -> Result<String, Option<String>> {
        let Some(t) = signed_in() else { return Err(None) };
        if !force && t.expires > now_ms() + EARLY_MS {
            return Ok(t.access);
        }
        let b = backend();
        let mut guard = TOKENS.lock().map_err(|_| None)?;
        // Another thread may have done it while this one waited.
        if let Some(now) = guard.as_ref() {
            if now.refresh != t.refresh {
                return Ok(now.access.clone());
            }
        }
        let (status, v) = match &b {
            Backend::Studio => {
                let url = format!("{}{API}/oauth/token", site());
                let form = [("client_id", CLIENT), ("grant_type", "refresh_token"), ("refresh_token", t.refresh.as_str())];
                http("POST", &url, None, None, Some(&form)).map_err(Some)?
            }
            Backend::Vault(vault) => {
                let url = format!("{}/auth/refresh", vault.address());
                http("POST", &url, None, Some(&json!({"refresh_token": t.refresh})), None).map_err(Some)?
            }
        };
        if status == 200 {
            let new = match &b {
                Backend::Studio => Tokens::from_answer(&v, t.name),
                Backend::Vault(_) => Tokens::from_vault(&v, t.name, &t.refresh),
            }
            .ok_or(Some("Bad answer from the server".to_string()))?;
            store::save(&b.slot(), &new.encode());
            let access = new.access.clone();
            *guard = Some(new);
            return Ok(access);
        }
        if status >= 500 {
            return Err(Some("The server is having trouble".into()));
        }
        *guard = None;
        store::clear(&b.slot());
        Err(None)
    }

    /// A call to the API as this account; one retry on an expired token.
    fn api(method: &str, path: &str, body: Option<&Value>) -> Result<Value, Done> {
        let b = backend();
        let url = format!("{}{}{path}", b.base(), b.api());
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

    /// `scheme://host[:port]` of a URL, lower case, with ws(s) as http(s).
    fn origin_of(url: &str) -> Option<String> {
        let (scheme, rest) = url.split_once("://")?;
        let scheme = match scheme.to_ascii_lowercase().as_str() {
            "wss" | "https" => "https",
            "ws" | "http" => "http",
            _ => return None,
        };
        Some(format!("{scheme}://{}", rest.split(['/', '?', '#']).next()?.to_ascii_lowercase()))
    }

    /// The token to send when connecting to this address, if it is the account's.
    pub fn bearer_for(ws_url: &str) -> Option<String> {
        let base = origin_of(&backend().base())?;
        if origin_of(ws_url)? == base {
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
        /// The vault in use answered at this address (or not: offline).
        Reached(Option<Vault>),
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

    /// A page's relay link from an answer; a path is made absolute (wss).
    fn link_of(v: &Value) -> Option<String> {
        let l = v["open"]["link"].as_str().or(v["link"].as_str())?;
        let b = backend();
        let ws = b.base().replacen("https://", "wss://", 1).replacen("http://", "ws://", 1);
        if l.starts_with('/') {
            return Some(format!("{ws}{l}"));
        }
        // A vault names its own public address; use the one it answered on
        // here (another network may not reach that name).
        if let (Backend::Vault(_), Some(i)) = (&b, l.find(&format!("{VAULT_API}/pages/"))) {
            if l.starts_with("ws") {
                return Some(format!("{ws}{}", &l[i..]));
            }
        }
        Some(l.to_string())
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

    /// The vault's sign-in page for this app (APP-LOGIN.md section 3).
    pub(super) fn vault_start_url(address: &str, redirect: &str, challenge: &str, state: &str, device: &str) -> String {
        format!(
            "{address}/auth/vault/start?client_id={VAULT_CLIENT}&redirect_uri={}&code_challenge={challenge}\
             &code_challenge_method=S256&state={state}&device={}",
            enc(redirect),
            enc(&format!("OG Paper · {device}"))
        )
    }

    /// The page the browser lands on after the vault (the app is in front
    /// again, or one tap away).
    const BACK: &str = "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'>\
        <title>OG Paper</title><body style='font:18px system-ui;margin:3em auto;max-width:28em;padding:0 1em'>\
        <p><b>OG Paper</b></p><p>MSG</p></body>";

    /// Wait for the vault to send the browser back to `listener`: the code,
    /// or why not. Requests that aren't the answer (a favicon) get a 404.
    pub(super) fn wait_for_vault(listener: std::net::TcpListener, state: &str, stop: &AtomicBool) -> Result<String, String> {
        use std::io::{BufRead, BufReader, Write};
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let until = now_ms() + ASK_FOR.as_millis() as u64;
        while now_ms() < until {
            if stop.load(Ordering::Relaxed) {
                return Err(String::new());
            }
            let (mut s, _) = match listener.accept() {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(150));
                    continue;
                }
                Err(e) => return Err(e.to_string()),
            };
            let _ = s.set_nonblocking(false);
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            let mut line = String::new();
            if BufReader::new(&s).read_line(&mut line).is_err() {
                continue;
            }
            let target = line.split_whitespace().nth(1).unwrap_or("").to_string();
            let q = query(&target);
            let get = |k: &str| q.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
            if get("state").is_none() && get("code").is_none() {
                let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue;
            }
            let ok = get("state").as_deref() == Some(state) && get("code").is_some();
            let msg = if ok { "Signed in. Go back to OG Paper." } else { "Sign-in didn't finish. Go back to OG Paper and try again." };
            let page = BACK.replace("MSG", msg);
            let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len()).as_bytes());
            if get("state").as_deref() != Some(state) {
                return Err("This sign-in didn't match. Try again.".into());
            }
            return match get("code") {
                Some(c) => Ok(c),
                None => Err(get("login_error").unwrap_or_else(|| "Sign-in was cancelled".into())),
            };
        }
        Err("Sign-in timed out. Try again.".into())
    }

    /// Swap the vault's one-time code for tokens and save the profile.
    pub(super) fn vault_finish(address: &str, vault_id: &str, name: &str, code: &str, verifier: &str, redirect: &str) -> Done {
        let url = format!("{address}/auth/token");
        let body = json!({"code": code, "verifier": verifier, "redirect_uri": redirect});
        let (status, v) = match http("POST", &url, None, Some(&body), None) {
            Ok(a) => a,
            Err(_) => return Done::Failed("The vault didn't answer. Try again.".into()),
        };
        if status != 200 || v["access_token"].as_str().is_none() {
            return Done::Failed(v["message"].as_str().or(v["error"].as_str()).unwrap_or("The vault refused the sign-in. Try again.").to_string());
        }
        // Still the vault that was probed (PROFILES.md: never adopt another).
        match probe(address) {
            Some((id, ..)) if id == vault_id => {}
            _ => return Done::Failed("That address now answers as another vault. Try again.".into()),
        }
        let user = &v["user"];
        let user_name = user["name"].as_str().or(user["email"].as_str()).unwrap_or("").to_string();
        let vault = Vault {
            id: vault_id.to_string(),
            name: name.to_string(),
            addresses: vec![address.to_string()],
            user_id: user["id"].as_str().map(String::from).or(user["id"].as_i64().map(|i| i.to_string())).unwrap_or_default(),
            user_name: user_name.clone(),
            last_used: now_ms(),
        };
        let Some(t) = Tokens::from_vault(&v, vault.label(), "") else {
            return Done::Failed("Bad answer from the vault".into());
        };
        save_vault(&vault);
        let saved = vaults().into_iter().find(|x| x.id == vault.id).unwrap_or(vault);
        use_backend(Backend::Vault(saved.clone()));
        remember(Some(t));
        Done::SignedIn(saved.label())
    }

    /// Sign in to the vault at `text` (an address or connect link): probe,
    /// browser to its sign-in page, wait for it to come back here.
    pub(super) fn vault_sign_in(text: String, stop: Arc<AtomicBool>, open: impl FnOnce(&str) -> Result<(), String>) -> Done {
        let Some((address, want)) = parse_connect(&text) else {
            return Done::Failed("Enter a vault address".into());
        };
        let Some((id, name, _)) = probe(&address) else {
            return Done::Failed("No OG Vault answers at that address".into());
        };
        if want.is_some_and(|w| w != id) {
            return Done::Failed("That address answers as another vault".into());
        }
        let listener = match std::net::TcpListener::bind("127.0.0.1:0") {
            Ok(l) => l,
            Err(e) => return Done::Failed(format!("Couldn't start signing in: {e}")),
        };
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        let redirect = format!("http://127.0.0.1:{port}/");
        let verifier = b64(&random_secret()) + &b64(&random_secret()[..16]);
        let challenge = challenge_of(&verifier);
        let state = b64(&random_secret()[..24]);
        if let Err(e) = open(&vault_start_url(&address, &redirect, &challenge, &state, device())) {
            return Done::Failed(format!("Couldn't open the browser: {e}"));
        }
        match wait_for_vault(listener, &state, &stop) {
            Ok(code) => vault_finish(&address, &id, &name, &code, &verifier, &redirect),
            Err(why) => Done::Failed(why),
        }
    }

    fn open_browser(url: &str) -> Result<(), String> {
        webbrowser::open(url).map_err(|e| e.to_string())
    }

    impl App {
        fn account_stop_waiting(&mut self) {
            if let Some(s) = self.account.waiting.take() {
                s.store(true, Ordering::Relaxed);
            }
            self.ui.account.pair = None;
        }

        /// Fill the view's list of saved accounts (OG Studio when signed in
        /// there, then each vault), the one in use marked.
        fn account_profiles(&mut self) {
            let cur = backend();
            let mut list = vec![];
            if store::load("tokens").is_some() {
                list.push(AccountProfile { label: "OG Studio".into(), current: cur == Backend::Studio });
            }
            for v in vaults() {
                list.push(AccountProfile { label: v.label(), current: matches!(&cur, Backend::Vault(c) if c.id == v.id) });
            }
            self.ui.account.profiles = list;
            self.ui.account.vault = matches!(cur, Backend::Vault(_));
        }

        fn account_signed_out_here(&mut self) {
            self.ui.account.name = None;
            self.ui.account.pages.clear();
            self.account.ids.clear();
            self.account_profiles();
        }

        pub(crate) fn account_op(&mut self, op: Op) {
            match op {
                Op::SignIn => {
                    self.account_stop_waiting();
                    use_backend(Backend::Studio);
                    self.account_signed_out_here();
                    let verifier = b64(&random_secret());
                    let challenge = challenge_of(&verifier);
                    let url = authorize_url(&site(), &challenge, device());
                    self.ui.account.pair = Some(pair_code(&challenge));
                    self.ui.account.prompt = false;
                    self.ui.account.state.clear();
                    self.account_not_now();
                    if let Err(e) = open_browser(&url) {
                        self.account_stop_waiting();
                        self.say(format!("Couldn't open the browser: {e}"));
                        return;
                    }
                    let stop = Arc::new(AtomicBool::new(false));
                    self.account.waiting = Some(stop.clone());
                    self.account.run(move || wait_for_browser(verifier, stop));
                }
                Op::VaultSignIn => {
                    self.account_stop_waiting();
                    let text = self.ui.account.vault_input.trim().to_string();
                    if text.is_empty() {
                        return;
                    }
                    // Waiting for the browser (no code to compare: the vault's
                    // page names the app and where it goes back to).
                    self.ui.account.pair = Some(String::new());
                    self.ui.account.prompt = false;
                    self.ui.account.state.clear();
                    self.account_not_now();
                    let stop = Arc::new(AtomicBool::new(false));
                    self.account.waiting = Some(stop.clone());
                    self.account.run(move || vault_sign_in(text, stop, open_browser));
                }
                Op::Cancel => self.account_stop_waiting(),
                Op::SignOut => {
                    let b = backend();
                    let token = signed_in().map(|t| t.access);
                    remember(None);
                    if let Some(token) = token {
                        let url = match &b {
                            Backend::Studio => format!("{}{API}/oauth/revoke", site()),
                            Backend::Vault(v) => format!("{}/auth/logout", v.address()),
                        };
                        std::thread::spawn(move || {
                            let _ = http("POST", &url, Some(&token), None, None);
                        });
                    }
                    // A vault profile goes from the list (PROFILES.md section 3);
                    // the next saved account, if any, is used.
                    if let Backend::Vault(v) = &b {
                        save_vaults(&vaults().into_iter().filter(|x| x.id != v.id).collect::<Vec<_>>());
                        let next = vaults().into_iter().max_by_key(|x| x.last_used).map_or(Backend::Studio, Backend::Vault);
                        use_backend(next);
                    }
                    self.account_signed_out_here();
                    if let Some(t) = signed_in() {
                        self.ui.account.name = Some(t.name);
                        self.account.run(page_list);
                    }
                    self.say("Signed out. Pages stay on this device.");
                }
                Op::Switch(i) => {
                    self.account_profiles();
                    let has_studio = store::load("tokens").is_some();
                    let next = if has_studio && i == 0 {
                        Backend::Studio
                    } else {
                        match vaults().into_iter().nth(i - usize::from(has_studio)) {
                            Some(mut v) => {
                                v.last_used = now_ms();
                                save_vault(&v);
                                Backend::Vault(v)
                            }
                            None => return,
                        }
                    };
                    if next == backend() {
                        return;
                    }
                    use_backend(next);
                    self.account_signed_out_here();
                    if let Some(t) = signed_in() {
                        self.say(format!("Using {}", t.name));
                        self.ui.account.name = Some(t.name);
                        self.account_reach();
                    }
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
                    if let Err(e) = open_browser(&url) {
                        self.say(format!("Couldn't open the browser: {e}"));
                    }
                }
                Op::NotNow => {
                    self.ui.account.prompt = false;
                    self.account_not_now();
                }
            }
        }

        /// The vault in use: find the address that answers as it, then list.
        fn account_reach(&mut self) {
            match backend() {
                Backend::Vault(v) => self.account.run(move || Done::Reached(reach(&v))),
                Backend::Studio => self.account.run(page_list),
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
                self.account_profiles();
                match signed_in() {
                    Some(t) => {
                        self.ui.account.name = Some(t.name);
                        self.account_reach();
                    }
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
                        self.ui.account.vault_input.clear();
                        self.say(format!("Signed in as {name}"));
                        self.ui.account.name = Some(name);
                        self.account_profiles();
                        self.account.run(page_list);
                        // Saving what's open is what signing in was for.
                        if self.net.is_none() && !self.scene.strokes.is_empty() {
                            self.account_op(Op::Keep);
                        }
                    }
                    Done::SignedOut => {
                        self.account_signed_out_here();
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
                        self.ui.account.state.clear();
                    }
                    Done::Reached(Some(v)) => {
                        save_vault(&v);
                        if matches!(backend(), Backend::Vault(c) if c.id == v.id) {
                            *BACKEND.lock().unwrap_or_else(|e| e.into_inner()) = Some(Backend::Vault(v));
                        }
                        self.account_profiles();
                        self.account.run(page_list);
                    }
                    Done::Reached(None) => {
                        // Offline, or another network: pages here stay usable and
                        // their changes go up once the vault answers again.
                        self.ui.account.state = "Not reachable now".into();
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

        /// `OGP_TEST_ACCOUNT=pairing|pages|signed-in|vault`, `OGP_TEST_SIZE=390x844`:
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
                Ok("vault") => {
                    self.ui.pages_open = true;
                    self.ui.account.prompt = false;
                    self.ui.account.vault_form = true;
                }
                Ok("signed-in") => {
                    self.ui.pages_open = true;
                    self.ui.account.prompt = false;
                    self.ui.account.name = Some("alice @ Home".into());
                    self.ui.account.vault = true;
                    self.ui.account.profiles = vec![
                        AccountProfile { label: "OG Studio".into(), current: false },
                        AccountProfile { label: "alice @ Home".into(), current: true },
                    ];
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
#[cfg(not(target_arch = "wasm32"))]
pub use imp::bearer_for;

#[cfg(all(test, not(target_arch = "wasm32")))]
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

    #[test]
    fn vault_addresses_and_connect_links() {
        assert_eq!(normalize("vault.ts.net/"), Some("https://vault.ts.net".into()));
        assert_eq!(normalize("HTTP://Host:8610/x?y"), Some("http://host:8610".into()));
        assert_eq!(normalize("ftp://x"), None);
        assert_eq!(normalize("a b"), None);
        assert_eq!(
            parse_connect("ogvault://connect?u=https%3A%2F%2Fog-host1.ts.net%3A8610&id=6f1c&n=Home"),
            Some(("https://og-host1.ts.net:8610".into(), Some("6f1c".into())))
        );
        assert_eq!(
            parse_connect("https://v.example/connect?u=https%3A%2F%2Fv.example&id="),
            Some(("https://v.example".into(), None))
        );
        assert_eq!(parse_connect("og-host1.ts.net:8610"), Some(("https://og-host1.ts.net:8610".into(), None)));
        assert_eq!(parse_connect("ogvault://other?u=x"), None);
    }

    #[test]
    fn the_vault_start_url_carries_pkce_and_the_loopback() {
        let u = vault_start_url("https://v.example", "http://127.0.0.1:5123/", "chal", "st8", "Linux");
        assert!(u.starts_with("https://v.example/auth/vault/start?client_id=og-paper&"));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5123%2F"));
        assert!(u.contains("code_challenge=chal&code_challenge_method=S256&state=st8"));
        let q = query(&u);
        assert!(q.contains(&("device".into(), "OG Paper · Linux".into())));
    }

    #[test]
    fn vault_tokens_expire_in_seconds() {
        let v = serde_json::json!({"access_token": "ogv_a_x", "access_expires": 1_900_000_000u64, "refresh_token": "ogv_r_y"});
        let t = Tokens::from_vault(&v, "n".into(), "old").unwrap();
        assert_eq!(t.expires, 1_900_000_000_000);
        let v = serde_json::json!({"access_token": "a2", "access_expires": 5u64});
        assert_eq!(Tokens::from_vault(&v, "n".into(), "old").unwrap().refresh, "old");
    }

    #[test]
    fn the_browser_coming_back_hands_over_the_code() {
        use std::io::{Read, Write};
        use std::sync::atomic::AtomicBool;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let browser = std::thread::spawn(move || {
            let get = |path: &str| {
                let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
                let mut out = String::new();
                let _ = s.read_to_string(&mut out);
                out
            };
            assert!(get("/favicon.ico").starts_with("HTTP/1.1 404"));
            assert!(get("/?code=c%2B1&state=abc").contains("Signed in"));
        });
        assert_eq!(wait_for_vault(l, "abc", &AtomicBool::new(false)), Ok("c+1".into()));
        browser.join().unwrap();

        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(s, "GET /?error=access_denied&login_error=Cancelled%20here&state=abc HTTP/1.1\r\n\r\n").unwrap();
            let _ = s.read_to_string(&mut String::new());
        });
        assert_eq!(wait_for_vault(l, "abc", &AtomicBool::new(false)), Err("Cancelled here".into()));
    }

    /// The vault's sign-in as a browser already signed in there would do it:
    /// start → "Continue as …" → back to the app's loopback with the code.
    fn browser_at_vault(session: String) -> impl FnOnce(&str) -> Result<(), String> {
        move |start: &str| {
            let start = start.to_string();
            std::thread::spawn(move || {
                let agent: ureq::Agent = ureq::Agent::config_builder().max_redirects(0).http_status_as_error(false).build().into();
                let cookie = format!("ogv_session={session}");
                let r = agent.get(&start).header("Cookie", &cookie).call().expect("start");
                let cont = r.headers().get("location").expect("to the sign-in page").to_str().unwrap().to_string();
                let origin = start.split("/auth/").next().unwrap().to_string();
                let cont = if cont.starts_with('/') { format!("{origin}{cont}") } else { cont };
                let r = agent
                    .post(&cont)
                    .header("Cookie", &cookie)
                    .header("Origin", &origin)
                    .send_form([("confirm", "yes")])
                    .expect("continue as");
                let back = r.headers().get("location").expect("back to the app").to_str().unwrap().to_string();
                assert!(back.starts_with("http://127.0.0.1:"), "{back}");
                ureq::get(&back).call().expect("the app's loopback");
            });
            Ok(())
        }
    }

    /// A vault end to end (tests/vault/run-native.sh, a throwaway vault): sign
    /// in through the vault's page, keep a page, get it back on a second
    /// device that signs in too, and draw on it live from both.
    #[test]
    #[ignore = "needs a throwaway vault"]
    fn a_page_kept_in_a_vault_comes_back_on_a_new_device() {
        use crate::App;
        use serde_json::json;
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        let vault = std::env::var("OGP_VAULT").expect("OGP_VAULT");
        let session = std::env::var("OGP_VAULT_SESSION").expect("OGP_VAULT_SESSION");
        let pump = |apps: &mut [&mut App], secs: u64, done: &dyn Fn(&[&mut App]) -> bool| {
            let until = Instant::now() + Duration::from_secs(secs);
            while Instant::now() < until && !done(apps) {
                for a in apps.iter_mut() {
                    a.account_tick();
                    a.net_tick();
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            done(apps)
        };
        let home = |n: &str| {
            let d = std::env::temp_dir().join(format!("ogp-vault-e2e-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&d).unwrap();
            std::env::set_var("HOME", &d);
            crate::prefs::save(&Default::default());
        };
        let has = |a: &App, t: &str| a.texts_at(&a.cam).iter().any(|x| x.to_string().contains(t));

        // Device 1.
        home("one");
        let label = match vault_sign_in(vault.clone(), Arc::new(AtomicBool::new(false)), browser_at_vault(session.clone())) {
            Done::SignedIn(l) => l,
            Done::Failed(why) => panic!("sign-in failed: {why}"),
            _ => panic!("sign-in failed"),
        };
        assert!(label.contains(" @ "), "{label}");
        let mut one = App::new(None);
        one.account_tick();
        assert_eq!(one.ui.account.name.as_deref(), Some(label.as_str()));
        assert!(one.ui.account.vault);
        let r = one.run_commands(&json!([{"add": "text", "x": 0, "y": 0, "text": "from-native-one", "size": 30}]));
        assert_eq!(r[0]["ok"], true, "{r}");
        one.account_op(crate::account::Op::Keep);
        assert!(pump(&mut [&mut one], 30, &|a| !a[0].ui.account.pages.is_empty()), "kept: {}", one.ui.account.state);
        pump(&mut [&mut one], 4, &|_| false);

        // Device 2: nothing on it; signs in to the same vault.
        let saved_tokens = TOKENS.lock().unwrap().take();
        *BACKEND.lock().unwrap() = None;
        home("two");
        let label2 = match vault_sign_in(vault.clone(), Arc::new(AtomicBool::new(false)), browser_at_vault(session)) {
            Done::SignedIn(l) => l,
            Done::Failed(why) => panic!("second sign-in failed: {why}"),
            _ => panic!("second sign-in failed"),
        };
        assert_eq!(label, label2);
        let mut two = App::new(None);
        assert!(!has(&two, "from-native-one"));
        two.account_tick();
        assert!(pump(&mut [&mut two], 30, &|a| !a[0].ui.account.pages.is_empty()), "listed: {}", two.ui.account.state);
        two.account_op(crate::account::Op::Open(0));
        assert!(pump(&mut [&mut two], 30, &|a| has(&a[0], "from-native-one")), "the page came back");
        // Both live on it (one process, one account: the same tokens serve both).
        drop(saved_tokens);
        let r = two.run_commands(&json!([{"add": "text", "x": 0, "y": 60, "text": "from-native-two", "size": 30}]));
        assert_eq!(r[0]["ok"], true, "{r}");
        assert!(pump(&mut [&mut one, &mut two], 30, &|a| has(&a[0], "from-native-two")), "live to device one");
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
