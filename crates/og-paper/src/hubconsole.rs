// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! The page server's web console (`/console`): sign in, see the pages as a
//! tree of folders, and manage them by role:
//!
//! - admin: make, rename, move and delete pages (deleting tags the file
//!   `.ogp.deleted`; it can be restored), manage folders and users.
//! - subadmin: rename and move pages, make and rename folders.
//! - user: see the pages and open them (each page's own link).
//!
//! Users live in `DIR/users.json` (passwords salted and hashed with PBKDF2),
//! folders in `DIR/folders.json`. The first start makes `admin` with the
//! password `password` and asks for it to be changed. Sessions are cookies
//! (HttpOnly, SameSite=Strict) and every form carries the session's token.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::hub::{changed_ms, clean_name, make_page, page_path, read_index, write_index};
use crate::hubpage::{esc, url_encode, ICON, WEB_APP};
use crate::net::native::{WebReq, WebResp};

/// What a signed-in person may do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Admin,
    Subadmin,
    User,
    Viewer,
}

impl Role {
    pub fn key(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Subadmin => "subadmin",
            Role::User => "user",
            Role::Viewer => "viewer",
        }
    }
    fn from(s: &str) -> Option<Role> {
        Some(match s {
            "admin" => Role::Admin,
            "subadmin" => Role::Subadmin,
            "user" => Role::User,
            "viewer" => Role::Viewer,
            _ => return None,
        })
    }
    fn label(self) -> &'static str {
        match self {
            Role::Admin => "Admin",
            Role::Subadmin => "Subadmin",
            Role::User => "User",
            Role::Viewer => "Viewer",
        }
    }
    pub fn organizes(self) -> bool {
        matches!(self, Role::Admin | Role::Subadmin)
    }
    /// May draw on pages (everyone but viewers).
    pub fn can_draw(self) -> bool {
        self != Role::Viewer
    }
}

#[derive(Clone)]
struct User {
    name: String,
    role: Role,
    salt: String,
    hash: String,
    /// Still the default password: ask to change it.
    must_change: bool,
    /// The apps' remembered sign-ins (SHA-256 of each token), newest last.
    /// A new password clears them.
    tokens: Vec<String>,
}

struct Session {
    user: String,
    csrf: String,
    last: Instant,
}

/// Folders and which folder each page is in ("" is the top).
#[derive(Default)]
struct Folders {
    folders: Vec<String>,
    pages: HashMap<String, String>,
    /// Pages tagged for deletion: page id -> (name, folder).
    deleted: HashMap<String, (String, String)>,
}

/// Changes the hub must apply to pages it has open.
pub enum HubOp {
    /// Deleted: stop serving it.
    Unload(u128),
    /// Renamed.
    Renamed(u128, String),
}

/// A change to the workspace's pages or folders (see `Console::page_op`).
pub enum PageOp {
    New { name: String, folder: String },
    Rename(u128, String),
    Move(u128, String),
    Delete(u128),
    Restore(u128),
    NewFolder { parent: String, name: String },
    RenameFolder { folder: String, name: String },
    DeleteFolder(String),
}

pub struct Console {
    dir: PathBuf,
    users: Vec<User>,
    sessions: HashMap<String, Session>,
    folders: Folders,
    /// Failed sign-ins per user name: count, since.
    fails: HashMap<String, (u32, Instant)>,
    pub ops: Vec<HubOp>,
}

const ITERATIONS: u32 = 120_000;
const SESSION: Duration = Duration::from_secs(7 * 24 * 3600);
const COOKIE: &str = "ogp_session";

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hash_password(pw: &str, salt: &str) -> String {
    let mut out = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(pw.as_bytes(), salt.as_bytes(), ITERATIONS, &mut out);
    hex(&out)
}

/// Equal in constant time (for hashes).
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn new_user(name: &str, role: Role, pw: &str, must_change: bool) -> User {
    let salt = hex(&crate::seal::random_secret()[..16]);
    User {
        name: name.to_string(),
        role,
        hash: hash_password(pw, &salt),
        salt,
        must_change,
        tokens: vec![],
    }
}

/// A folder path from what was typed: segments without slashes, trimmed.
fn clean_folder(p: &str) -> String {
    p.split('/')
        .map(|s| s.trim().chars().take(60).collect::<String>())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

fn form(body: &[u8]) -> HashMap<String, String> {
    fn dec(s: &str) -> String {
        let b = s.replace('+', " ").into_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' && i + 2 < b.len() {
                if let Ok(v) = u8::from_str_radix(&String::from_utf8_lossy(&b[i + 1..i + 3]), 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
            out.push(b[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    String::from_utf8_lossy(body)
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (dec(k), dec(v)))
        .collect()
}

fn query(target: &str, name: &str) -> Option<String> {
    let q = target.split_once('?')?.1;
    q.split('&')
        .find_map(|kv| kv.strip_prefix(name).and_then(|v| v.strip_prefix('=')))
        .map(|v| {
            form(format!("x={v}").as_bytes())
                .remove("x")
                .unwrap_or_default()
        })
}

fn html(body: String) -> WebResp {
    WebResp {
        status: 200,
        ctype: "text/html; charset=utf-8",
        headers: vec![],
        body: body.into_bytes(),
    }
}

fn redirect(to: &str) -> WebResp {
    WebResp {
        status: 303,
        ctype: "text/plain; charset=utf-8",
        headers: vec![("Location".into(), to.to_string())],
        body: vec![],
    }
}

impl Console {
    pub fn load(dir: &Path) -> Console {
        let mut c = Console {
            dir: dir.to_path_buf(),
            users: vec![],
            sessions: HashMap::new(),
            folders: Folders::default(),
            fails: HashMap::new(),
            ops: vec![],
        };
        if let Some(v) = std::fs::read_to_string(dir.join("users.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        {
            for u in v["users"].as_array().into_iter().flatten() {
                if let (Some(name), Some(role), Some(salt), Some(hash)) = (
                    u["name"].as_str(),
                    u["role"].as_str().and_then(Role::from),
                    u["salt"].as_str(),
                    u["hash"].as_str(),
                ) {
                    c.users.push(User {
                        name: name.into(),
                        role,
                        salt: salt.into(),
                        hash: hash.into(),
                        must_change: u["must_change"].as_bool().unwrap_or(false),
                        tokens: u["tokens"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|t| t.as_str().map(str::to_string))
                            .collect(),
                    });
                }
            }
        }
        if !c.users.iter().any(|u| u.role == Role::Admin) {
            c.users.retain(|u| u.name != "admin");
            c.users
                .push(new_user("admin", Role::Admin, "password", true));
            c.save_users();
            println!("Console: sign in at /console as admin / password, then change the password.");
        }
        if let Some(v) = std::fs::read_to_string(dir.join("folders.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        {
            c.folders.folders = v["folders"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|f| f.as_str().map(clean_folder))
                .filter(|f| !f.is_empty())
                .collect();
            for (k, f) in v["pages"].as_object().into_iter().flatten() {
                if let Some(f) = f.as_str() {
                    c.folders.pages.insert(k.clone(), clean_folder(f));
                }
            }
            for (k, d) in v["deleted"].as_object().into_iter().flatten() {
                c.folders.deleted.insert(
                    k.clone(),
                    (
                        d["name"].as_str().unwrap_or("Page").into(),
                        d["folder"].as_str().unwrap_or("").into(),
                    ),
                );
            }
        }
        c
    }

    fn save_users(&self) {
        let users: Vec<Value> = self
            .users
            .iter()
            .map(|u| json!({"name": u.name, "role": u.role.key(), "salt": u.salt, "hash": u.hash, "must_change": u.must_change, "tokens": u.tokens}))
            .collect();
        let path = self.dir.join("users.json");
        let _ = std::fs::write(
            &path,
            serde_json::to_string_pretty(&json!({"users": users})).unwrap_or_default(),
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }

    fn save_folders(&self) {
        let deleted: serde_json::Map<String, Value> = self
            .folders
            .deleted
            .iter()
            .map(|(k, (n, f))| (k.clone(), json!({"name": n, "folder": f})))
            .collect();
        let v = json!({"folders": self.folders.folders, "pages": self.folders.pages, "deleted": deleted});
        let _ = std::fs::write(
            self.dir.join("folders.json"),
            serde_json::to_string_pretty(&v).unwrap_or_default(),
        );
    }

    fn session(&mut self, req: &WebReq) -> Option<(User, String)> {
        let token = req
            .header("cookie")?
            .split(';')
            .find_map(|c| c.trim().strip_prefix(&format!("{COOKIE}=")))?
            .to_string();
        self.sessions.retain(|_, s| s.last.elapsed() < SESSION);
        let s = self.sessions.get_mut(&token)?;
        s.last = Instant::now();
        let (name, csrf) = (s.user.clone(), s.csrf.clone());
        let user = self.users.iter().find(|u| u.name == name)?.clone();
        Some((user, csrf))
    }

    /// The tab bar for the home page: live when signed in, greyed (each tab
    /// leading to sign-in) when not.
    pub fn home_nav(&mut self, req: &WebReq) -> String {
        let s = self.session(req);
        nav(s.as_ref().map(|(u, c)| (u, c.as_str())), "/")
    }

    /// Answer a request under `/console` (or `/login`, `/logout`).
    pub fn handle(&mut self, req: &WebReq, server_name: &str) -> WebResp {
        let path = req.target.split('?').next().unwrap_or("/");
        let secure = req.base.starts_with("wss://");
        match (req.method.as_str(), path) {
            ("GET", "/login") => {
                if self.session(req).is_some() {
                    return redirect(&safe_next(query(&req.target, "next")));
                }
                html(self.login_page(server_name, None, &safe_next(query(&req.target, "next"))))
            }
            ("POST", "/login") => self.login(req, server_name, secure),
            ("POST", "/logout") => {
                if let Some((_, csrf)) = self.session(req) {
                    let f = form(&req.body);
                    if f.get("csrf") == Some(&csrf) {
                        let token = req
                            .header("cookie")
                            .and_then(|c| {
                                c.split(';')
                                    .find_map(|c| c.trim().strip_prefix(&format!("{COOKIE}=")))
                            })
                            .map(str::to_string);
                        if let Some(t) = token {
                            self.sessions.remove(&t);
                        }
                    }
                }
                let mut r = redirect("/login");
                r.headers.push((
                    "Set-Cookie".into(),
                    format!("{COOKIE}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict"),
                ));
                r
            }
            (_, p) if p == "/console" || p.starts_with("/console/") => {
                let Some((user, csrf)) = self.session(req) else {
                    return redirect(&format!("/login?next={}", url_encode(p)));
                };
                if req.method == "POST" {
                    let f = form(&req.body);
                    if f.get("csrf") != Some(&csrf) {
                        return WebResp {
                            status: 403,
                            ctype: "text/plain; charset=utf-8",
                            headers: vec![],
                            body: b"Expired form: reload the page and try again.\n".to_vec(),
                        };
                    }
                    let (back, msg) = self.act(&user, p, &f);
                    return redirect(&format!("{back}?m={}", url_encode(&msg)));
                }
                let msg = query(&req.target, "m");
                match p {
                    "/console/users" if user.role == Role::Admin => {
                        html(self.users_page(server_name, &user, &csrf, msg))
                    }
                    "/console/password" => html(self.password_page(server_name, &user, &csrf, msg)),
                    _ => html(self.pages_page(server_name, &user, &csrf, &req.base, msg)),
                }
            }
            _ => WebResp {
                status: 404,
                ctype: "text/plain; charset=utf-8",
                headers: vec![],
                body: b"not found\n".to_vec(),
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn add_user(&mut self, name: &str, role: Role, pw: &str) {
        self.users.push(new_user(name, role, pw, false));
    }

    /// An app signing in to the server's page list: by password (then a
    /// new token to remember) or by a token from before. The role and the
    /// token to keep.
    pub fn account(
        &mut self,
        name: &str,
        password: &str,
        token: &str,
    ) -> Result<(Role, String), &'static str> {
        use sha2::Digest;
        let digest = |t: &str| hex(&sha2::Sha256::digest(t.as_bytes()));
        if !token.is_empty() {
            let d = digest(token);
            return match self.users.iter().find(|u| u.name == name) {
                Some(u) if u.tokens.iter().any(|t| same(t, &d)) => Ok((u.role, token.to_string())),
                _ => Err("Signed out: sign in again"),
            };
        }
        let role = self.check(name, password)?;
        let token = hex(&crate::seal::random_secret());
        if let Some(u) = self.users.iter_mut().find(|u| u.name == name) {
            u.tokens.push(digest(&token));
            // Remember the last few devices.
            let extra = u.tokens.len().saturating_sub(20);
            u.tokens.drain(..extra);
        }
        self.save_users();
        Ok((role, token))
    }

    /// Check a user name and password (slowing down guessing): the role.
    pub fn check(&mut self, name: &str, pw: &str) -> Result<Role, &'static str> {
        if let Some((n, since)) = self.fails.get(name) {
            if *n >= 5 && since.elapsed() < Duration::from_secs(60) {
                return Err("Too many tries: wait a minute and try again.");
            }
        }
        let user = self
            .users
            .iter()
            .find(|u| u.name == name)
            .filter(|u| same(&hash_password(pw, &u.salt), &u.hash));
        match user {
            Some(u) => {
                let role = u.role;
                self.fails.remove(name);
                Ok(role)
            }
            None => {
                let e = self
                    .fails
                    .entry(name.to_string())
                    .or_insert((0, Instant::now()));
                if e.1.elapsed() > Duration::from_secs(60) {
                    *e = (0, Instant::now());
                }
                e.0 += 1;
                std::thread::sleep(Duration::from_millis(400));
                Err("That user name and password don't match.")
            }
        }
    }

    fn login(&mut self, req: &WebReq, server_name: &str, secure: bool) -> WebResp {
        let f = form(&req.body);
        let next = safe_next(f.get("next").cloned());
        let name = f
            .get("user")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let pw = f.get("password").cloned().unwrap_or_default();
        if let Err(e) = self.check(&name, &pw) {
            return html(self.login_page(server_name, Some(e), &next));
        }
        let token = hex(&crate::seal::random_secret());
        let csrf = hex(&crate::seal::random_secret()[..16]);
        self.sessions.insert(
            token.clone(),
            Session {
                user: name,
                csrf,
                last: Instant::now(),
            },
        );
        let mut r = redirect(&next);
        r.headers.push((
            "Set-Cookie".into(),
            format!(
                "{COOKIE}={token}; Path=/; Max-Age={}; HttpOnly; SameSite=Strict{}",
                SESSION.as_secs(),
                if secure { "; Secure" } else { "" }
            ),
        ));
        r
    }

    /// Do a console action; where to go back to and what to say.
    fn act(
        &mut self,
        user: &User,
        path: &str,
        f: &HashMap<String, String>,
    ) -> (&'static str, String) {
        let get = |k: &str| f.get(k).map(|s| s.trim().to_string()).unwrap_or_default();
        let role = user.role;
        let denied = || {
            (
                "/console",
                "You don't have permission to do that.".to_string(),
            )
        };
        if path == "/console/password" {
            let me = self.users.iter().position(|u| u.name == user.name);
            let Some(i) = me else {
                return ("/console", "Sign in again.".into());
            };
            let (old, new, again) = (
                get("old"),
                f.get("new").cloned().unwrap_or_default(),
                f.get("again").cloned().unwrap_or_default(),
            );
            if !same(
                &hash_password(&old, &self.users[i].salt),
                &self.users[i].hash,
            ) {
                return (
                    "/console/password",
                    "Your current password is wrong.".into(),
                );
            }
            if new.chars().count() < 8 {
                return ("/console/password", "Use at least 8 characters.".into());
            }
            if new != again {
                return ("/console/password", "The new passwords don't match.".into());
            }
            let n = new_user(&user.name, self.users[i].role, &new, false);
            self.users[i] = n;
            self.save_users();
            return ("/console", "Password changed.".into());
        }
        if path == "/console/users" {
            if role != Role::Admin {
                return denied();
            }
            let name = get("name");
            match get("action").as_str() {
                "add" => {
                    let name: String = name
                        .chars()
                        .filter(|c| c.is_alphanumeric() || "._-@".contains(*c))
                        .take(40)
                        .collect();
                    let pw = f.get("password").cloned().unwrap_or_default();
                    let Some(r) = Role::from(&get("role")) else {
                        return ("/console/users", "Pick a role.".into());
                    };
                    if name.is_empty() || self.users.iter().any(|u| u.name == name) {
                        return ("/console/users", "That user name is empty or taken.".into());
                    }
                    if pw.chars().count() < 8 {
                        return (
                            "/console/users",
                            "Give them a password of at least 8 characters.".into(),
                        );
                    }
                    self.users.push(new_user(&name, r, &pw, true));
                    self.save_users();
                    ("/console/users", format!("Added {name} as {}.", r.label()))
                }
                "role" => {
                    let Some(r) = Role::from(&get("role")) else {
                        return ("/console/users", "Pick a role.".into());
                    };
                    let admins = self.users.iter().filter(|u| u.role == Role::Admin).count();
                    let Some(u) = self.users.iter_mut().find(|u| u.name == name) else {
                        return ("/console/users", "No such user.".into());
                    };
                    if u.role == Role::Admin && r != Role::Admin && admins == 1 {
                        return ("/console/users", "There must be at least one admin.".into());
                    }
                    u.role = r;
                    self.save_users();
                    ("/console/users", format!("{name} is now {}.", r.label()))
                }
                "reset" => {
                    let pw = f.get("password").cloned().unwrap_or_default();
                    if pw.chars().count() < 8 {
                        return ("/console/users", "Use at least 8 characters.".into());
                    }
                    let Some(i) = self.users.iter().position(|u| u.name == name) else {
                        return ("/console/users", "No such user.".into());
                    };
                    let r = self.users[i].role;
                    self.users[i] = new_user(&name, r, &pw, true);
                    self.sessions.retain(|_, s| s.user != name);
                    self.save_users();
                    (
                        "/console/users",
                        format!("Password reset for {name}; they'll be asked to change it."),
                    )
                }
                "remove" => {
                    if name == user.name {
                        return ("/console/users", "You can't remove yourself.".into());
                    }
                    self.users.retain(|u| u.name != name);
                    self.sessions.retain(|_, s| s.user != name);
                    self.save_users();
                    ("/console/users", format!("Removed {name}."))
                }
                _ => ("/console/users", "Unknown action.".into()),
            }
        } else {
            let id = get("page");
            let canvas = u128::from_str_radix(&id, 16).ok();
            let op = match (get("action").as_str(), canvas) {
                ("new_page", _) => PageOp::New {
                    name: get("name"),
                    folder: get("folder"),
                },
                ("rename", Some(c)) => PageOp::Rename(c, get("name")),
                ("move", Some(c)) => PageOp::Move(c, get("folder")),
                ("delete", Some(c)) => PageOp::Delete(c),
                ("restore", Some(c)) => PageOp::Restore(c),
                ("new_folder", _) => PageOp::NewFolder {
                    parent: get("parent"),
                    name: get("name"),
                },
                ("rename_folder", _) => PageOp::RenameFolder {
                    folder: get("folder"),
                    name: get("name"),
                },
                ("delete_folder", _) => PageOp::DeleteFolder(get("folder")),
                _ => return ("/console", "No such page.".into()),
            };
            match self.page_op(role, op) {
                Ok((msg, _)) => ("/console", msg),
                Err(e) => ("/console", e),
            }
        }
    }

    /// The workspace's folders, and which folder each page is in.
    pub fn folder_list(&self) -> (Vec<String>, Vec<(u128, String)>) {
        let pages = self
            .folders
            .pages
            .iter()
            .filter_map(|(k, f)| Some((u128::from_str_radix(k, 16).ok()?, f.clone())))
            .collect();
        (self.folders.folders.clone(), pages)
    }

    /// Change the pages or folders as `role` may (the console and the apps
    /// both come here): what to say, and the page made, if one was.
    pub fn page_op(&mut self, role: Role, op: PageOp) -> Result<(String, Option<u128>), String> {
        let denied = || Err("You don't have permission to do that.".to_string());
        let no_page = || Err("No such page.".to_string());
        let admin = role == Role::Admin;
        match op {
            PageOp::New { name, folder } => {
                if !admin {
                    return denied();
                }
                let name = clean_name(&name);
                let folder = clean_folder(&folder);
                if !folder.is_empty() && !self.folders.folders.contains(&folder) {
                    return Err("No such folder.".into());
                }
                let Some(c) = make_page(&self.dir, &name) else {
                    return Err("Couldn't make the page.".into());
                };
                if !folder.is_empty() {
                    self.folders.pages.insert(format!("{c:032x}"), folder);
                    self.save_folders();
                }
                Ok((format!("Made {name}."), Some(c)))
            }
            PageOp::Rename(c, name) => {
                if !role.organizes() {
                    return denied();
                }
                let name = clean_name(&name);
                let mut all = read_index(&self.dir);
                let Some(p) = all.iter_mut().find(|p| p.0 == c) else {
                    return no_page();
                };
                p.1 = name.clone();
                write_index(&self.dir, &all);
                self.ops.push(HubOp::Renamed(c, name.clone()));
                Ok((format!("Renamed to {name}."), None))
            }
            PageOp::Move(c, folder) => {
                if !role.organizes() {
                    return denied();
                }
                let folder = clean_folder(&folder);
                if !folder.is_empty() && !self.folders.folders.contains(&folder) {
                    return Err("No such folder.".into());
                }
                if !read_index(&self.dir).iter().any(|p| p.0 == c) {
                    return no_page();
                }
                let key = format!("{c:032x}");
                if folder.is_empty() {
                    self.folders.pages.remove(&key);
                } else {
                    self.folders.pages.insert(key, folder.clone());
                }
                self.save_folders();
                Ok((
                    if folder.is_empty() {
                        "Moved to the top.".into()
                    } else {
                        format!("Moved to {folder}.")
                    },
                    None,
                ))
            }
            PageOp::Delete(c) => {
                if !admin {
                    return denied();
                }
                let mut all = read_index(&self.dir);
                let Some(i) = all.iter().position(|p| p.0 == c) else {
                    return no_page();
                };
                let (_, name) = all.remove(i);
                write_index(&self.dir, &all);
                let from = page_path(&self.dir, c);
                let _ = std::fs::rename(&from, from.with_extension("ogp.deleted"));
                let key = format!("{c:032x}");
                let folder = self.folders.pages.remove(&key).unwrap_or_default();
                self.folders.deleted.insert(key, (name.clone(), folder));
                self.save_folders();
                self.ops.push(HubOp::Unload(c));
                Ok((
                    format!(
                        "{name} is tagged for deletion (an admin can restore it in the console)."
                    ),
                    None,
                ))
            }
            PageOp::Restore(c) => {
                if !admin {
                    return denied();
                }
                let key = format!("{c:032x}");
                let Some((name, folder)) = self.folders.deleted.remove(&key) else {
                    return no_page();
                };
                let from = page_path(&self.dir, c).with_extension("ogp.deleted");
                if std::fs::rename(&from, page_path(&self.dir, c)).is_err() {
                    return Err("The page's file is gone.".into());
                }
                let mut all = read_index(&self.dir);
                all.push((c, name.clone()));
                write_index(&self.dir, &all);
                if !folder.is_empty() && self.folders.folders.contains(&folder) {
                    self.folders.pages.insert(key, folder);
                }
                self.save_folders();
                Ok((format!("Restored {name}."), None))
            }
            PageOp::NewFolder { parent, name } => {
                if !role.organizes() {
                    return denied();
                }
                let parent = clean_folder(&parent);
                let name = clean_folder(&name.replace('/', " "));
                if name.is_empty() {
                    return Err("Name the folder.".into());
                }
                if !parent.is_empty() && !self.folders.folders.contains(&parent) {
                    return Err("No such folder.".into());
                }
                let path = if parent.is_empty() {
                    name
                } else {
                    format!("{parent}/{name}")
                };
                if self.folders.folders.contains(&path) {
                    return Err("That folder exists.".into());
                }
                self.folders.folders.push(path.clone());
                self.folders.folders.sort();
                self.save_folders();
                Ok((format!("Made the folder {path}."), None))
            }
            PageOp::RenameFolder { folder, name } => {
                if !role.organizes() {
                    return denied();
                }
                let old = clean_folder(&folder);
                let name = clean_folder(&name.replace('/', " "));
                if old.is_empty() || name.is_empty() || !self.folders.folders.contains(&old) {
                    return Err("No such folder.".into());
                }
                let new = match old.rsplit_once('/') {
                    Some((p, _)) => format!("{p}/{name}"),
                    None => name,
                };
                let swap = |f: &str| -> String {
                    if f == old {
                        new.clone()
                    } else if let Some(rest) = f.strip_prefix(&format!("{old}/")) {
                        format!("{new}/{rest}")
                    } else {
                        f.to_string()
                    }
                };
                self.folders.folders = self.folders.folders.iter().map(|f| swap(f)).collect();
                self.folders.folders.sort();
                self.folders.folders.dedup();
                for f in self.folders.pages.values_mut() {
                    *f = swap(f);
                }
                self.save_folders();
                Ok((format!("Renamed the folder to {new}."), None))
            }
            PageOp::DeleteFolder(folder) => {
                if !admin {
                    return denied();
                }
                let old = clean_folder(&folder);
                if !self.folders.folders.contains(&old) {
                    return Err("No such folder.".into());
                }
                let parent = old
                    .rsplit_once('/')
                    .map(|(p, _)| p.to_string())
                    .unwrap_or_default();
                // Its pages and folders move up a level.
                let lift = |f: &str| -> String {
                    if f == old {
                        parent.clone()
                    } else if let Some(rest) = f.strip_prefix(&format!("{old}/")) {
                        if parent.is_empty() {
                            rest.to_string()
                        } else {
                            format!("{parent}/{rest}")
                        }
                    } else {
                        f.to_string()
                    }
                };
                self.folders.folders = self
                    .folders
                    .folders
                    .iter()
                    .filter(|f| **f != old)
                    .map(|f| lift(f))
                    .collect();
                self.folders.folders.sort();
                self.folders.folders.dedup();
                let pages: Vec<(String, String)> = self
                    .folders
                    .pages
                    .iter()
                    .map(|(k, f)| (k.clone(), lift(f)))
                    .collect();
                self.folders.pages = pages.into_iter().filter(|(_, f)| !f.is_empty()).collect();
                self.save_folders();
                Ok((
                    format!("Removed the folder {old}; what was in it moved up."),
                    None,
                ))
            }
        }
    }

    // ---- pages ---------------------------------------------------------------

    fn shell(
        &self,
        title: &str,
        active: &str,
        server: &str,
        user: Option<(&User, &str)>,
        msg: Option<String>,
        body: &str,
    ) -> String {
        let nav = nav(user, active);
        let warn = match user {
            Some((u, _)) if u.must_change => r#"<p class="flash warn">You're using a temporary password. <a href="/console/password">Change it now</a>.</p>"#.to_string(),
            _ => String::new(),
        };
        let flash = msg
            .filter(|m| !m.is_empty())
            .map(|m| format!(r#"<p class="flash">{}</p>"#, esc(&m)))
            .unwrap_or_default();
        format!(
            r##"<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex">
<title>{title} · {server}</title><link rel="icon" href="/icon.svg"><style>{CSS}{NAV_CSS}</style></head>
<body><main><header><a href="/" class="logo">{icon}</a><div><h1>{title}</h1><p>{server}</p></div></header>
{nav}{warn}{flash}{body}</main>
<script>
document.querySelectorAll('[data-copy]').forEach(b => b.onclick = async () => {{
  try {{ await navigator.clipboard.writeText(b.dataset.copy); }} catch (e) {{
    const t = document.createElement('textarea'); t.value = b.dataset.copy; document.body.append(t); t.select(); document.execCommand('copy'); t.remove(); }}
  const was = b.textContent; b.textContent = 'Copied'; setTimeout(() => b.textContent = was, 1400); }});
document.querySelectorAll('form[data-confirm]').forEach(f => f.onsubmit = e => {{ if (!confirm(f.dataset.confirm)) e.preventDefault(); }});
const ago = t => {{ if (!t) return ''; const s = (Date.now() - t) / 1000;
  return s < 60 ? 'just now' : s < 3600 ? Math.floor(s / 60) + ' min ago' : s < 86400 ? Math.floor(s / 3600) + ' h ago' : Math.floor(s / 86400) + ' days ago'; }};
document.querySelectorAll('time[data-t]').forEach(e => e.textContent = ago(+e.dataset.t));
</script></body></html>"##,
            title = esc(title),
            server = esc(server),
            icon = ICON.trim(),
        )
    }

    fn login_page(&self, server: &str, err: Option<&str>, next: &str) -> String {
        let body = format!(
            r#"<section class="card narrow"><h2>Sign in</h2>{err}
<form method="post" action="/login" class="stack"><input type="hidden" name="next" value="{next}">
<label>User name<input name="user" autocomplete="username" required autofocus></label>
<label>Password<input name="password" type="password" autocomplete="current-password" required></label>
<button class="btn primary">Sign in</button></form>
<p class="note">The server's first account is <b>admin</b> with the password <b>password</b>; change it after signing in.</p>
</section>"#,
            next = esc(next),
            err = err
                .map(|e| format!(r#"<p class="flash warn">{}</p>"#, esc(e)))
                .unwrap_or_default()
        );
        self.shell("Sign in", "", server, None, None, &body)
    }

    fn password_page(&self, server: &str, user: &User, csrf: &str, msg: Option<String>) -> String {
        let body = format!(
            r#"<section class="card narrow"><h2>Change your password</h2>
<form method="post" action="/console/password" class="stack"><input type="hidden" name="csrf" value="{csrf}">
<label>Current password<input name="old" type="password" autocomplete="current-password" required></label>
<label>New password (8 or more characters)<input name="new" type="password" autocomplete="new-password" minlength="8" required></label>
<label>New password again<input name="again" type="password" autocomplete="new-password" minlength="8" required></label>
<button class="btn primary">Change password</button></form></section>"#
        );
        self.shell(
            "Password",
            "/console/password",
            server,
            Some((user, csrf)),
            msg,
            &body,
        )
    }

    fn users_page(&self, server: &str, user: &User, csrf: &str, msg: Option<String>) -> String {
        let roles = |sel: Role| -> String {
            [Role::Admin, Role::Subadmin, Role::User, Role::Viewer]
                .iter()
                .map(|r| {
                    format!(
                        r#"<option value="{}"{}>{}</option>"#,
                        r.key(),
                        if *r == sel { " selected" } else { "" },
                        r.label()
                    )
                })
                .collect()
        };
        let mut rows = String::new();
        for u in &self.users {
            let me = u.name == user.name;
            rows += &format!(
                r#"<tr><td><b>{name}</b>{you}{temp}</td>
<td><form method="post" action="/console/users" class="inline"><input type="hidden" name="csrf" value="{csrf}"><input type="hidden" name="action" value="role"><input type="hidden" name="name" value="{name}"><select name="role">{roles}</select><button class="btn">Set</button></form></td>
<td><form method="post" action="/console/users" class="inline"><input type="hidden" name="csrf" value="{csrf}"><input type="hidden" name="action" value="reset"><input type="hidden" name="name" value="{name}"><input name="password" type="password" placeholder="New password" minlength="8" required autocomplete="new-password"><button class="btn">Reset</button></form></td>
<td>{remove}</td></tr>"#,
                name = esc(&u.name),
                you = if me { r#" <small>(you)</small>"# } else { "" },
                temp = if u.must_change {
                    r#" <small class="warnt">temporary password</small>"#
                } else {
                    ""
                },
                roles = roles(u.role),
                remove = if me {
                    String::new()
                } else {
                    format!(
                        r#"<form method="post" action="/console/users" class="inline" data-confirm="Remove {n}?"><input type="hidden" name="csrf" value="{csrf}"><input type="hidden" name="action" value="remove"><input type="hidden" name="name" value="{n}"><button class="btn danger">Remove</button></form>"#,
                        n = esc(&u.name)
                    )
                },
            );
        }
        let body = format!(
            r#"<section class="card"><h2>Users</h2><div class="scroll"><table><tr><th>User</th><th>Role</th><th>Password</th><th></th></tr>{rows}</table></div>
<p class="note"><b>Admin</b>: everything, including deleting pages and managing users. <b>Subadmin</b>: rename and move pages, organize folders. <b>User</b>: open and draw on pages. <b>Viewer</b>: open pages to look, never draw. In the apps, anyone can sign in with their account from <b>Share live</b> on a server page to get the right to draw.</p></section>
<section class="card"><h2>Add a user</h2><form method="post" action="/console/users" class="row"><input type="hidden" name="csrf" value="{csrf}"><input type="hidden" name="action" value="add">
<input name="name" placeholder="User name" required><input name="password" type="password" placeholder="Temporary password" minlength="8" required autocomplete="new-password"><select name="role">{r}</select><button class="btn primary">Add user</button></form>
<p class="note">They're asked to change the temporary password after signing in.</p></section>"#,
            r = roles(Role::User),
        );
        self.shell(
            "Users",
            "/console/users",
            server,
            Some((user, csrf)),
            msg,
            &body,
        )
    }

    fn pages_page(
        &self,
        server: &str,
        user: &User,
        csrf: &str,
        base: &str,
        msg: Option<String>,
    ) -> String {
        let base = base.trim_end_matches('/');
        let pages = read_index(&self.dir);
        let role = user.role;
        let folder_opts = |sel: &str| -> String {
            let mut o = format!(
                r#"<option value=""{}>(top)</option>"#,
                if sel.is_empty() { " selected" } else { "" }
            );
            for f in &self.folders.folders {
                o += &format!(
                    r#"<option value="{v}"{s}>{v}</option>"#,
                    v = esc(f),
                    s = if f == sel { " selected" } else { "" }
                );
            }
            o
        };
        let hidden = |action: &str| {
            format!(
                r#"<input type="hidden" name="csrf" value="{csrf}"><input type="hidden" name="action" value="{action}">"#
            )
        };
        let page_row = |c: u128, name: &str, folder: &str| -> String {
            let id = format!("{c:032x}");
            let (edit, keys) = crate::net::page_keys_for(c);
            // Viewers get the page's view key: their links can't draw.
            let edit = if role.can_draw() {
                edit
            } else {
                keys.view_token()
            };
            let app_link = format!("{base}/p/{id}?k={edit}");
            let web_link = format!("{WEB_APP}#join={base}/p/{id}&k={edit}");
            let mut tools = String::new();
            if role.organizes() {
                tools += &format!(
                    r#"<details class="tools"><summary>Edit</summary>
<form method="post" action="/console" class="row">{h}<input type="hidden" name="page" value="{id}"><input name="name" value="{n}" required><button class="btn">Rename</button></form>
<form method="post" action="/console" class="row">{m}<input type="hidden" name="page" value="{id}"><select name="folder">{opts}</select><button class="btn">Move</button></form>{del}</details>"#,
                    h = hidden("rename"),
                    m = hidden("move"),
                    n = esc(name),
                    opts = folder_opts(folder),
                    del = if role == Role::Admin {
                        format!(
                            r#"<form method="post" action="/console" data-confirm="Delete {n}? It's tagged for deletion and can be restored here.">{d}<input type="hidden" name="page" value="{id}"><button class="btn danger">Delete</button></form>"#,
                            d = hidden("delete"),
                            n = esc(name)
                        )
                    } else {
                        String::new()
                    },
                );
            }
            format!(
                r#"<li class="page"><div class="pline"><span class="ic">▤</span><b>{n}</b><time data-t="{t}"></time>
<span class="acts"><a class="btn primary" href="{web}" target="_blank" rel="noopener">Open</a><button class="btn" data-copy="{app}">Copy link</button></span></div>{tools}</li>"#,
                n = esc(name),
                t = changed_ms(&page_path(&self.dir, c)),
                web = esc(&web_link),
                app = esc(&app_link),
            )
        };
        // The tree: folders (sorted) with their pages, then top-level pages.
        let in_folder = |c: u128| {
            self.folders
                .pages
                .get(&format!("{c:032x}"))
                .cloned()
                .unwrap_or_default()
        };
        fn tree(
            prefix: &str,
            folders: &[String],
            pages: &[(u128, String)],
            in_folder: &dyn Fn(u128) -> String,
            page_row: &dyn Fn(u128, &str, &str) -> String,
            folder_tools: &dyn Fn(&str) -> String,
        ) -> String {
            let mut out = String::new();
            for f in folders.iter().filter(|f| match f.rsplit_once('/') {
                Some((p, _)) => p == prefix,
                None => prefix.is_empty(),
            }) {
                let label = f.rsplit('/').next().unwrap_or(f);
                let inner = tree(f, folders, pages, in_folder, page_row, folder_tools);
                out += &format!(
                    r#"<li class="folder"><details open><summary><span class="ic">▸</span><b>{l}</b></summary>{tools}<ul>{inner}</ul></details></li>"#,
                    l = esc(label),
                    tools = folder_tools(f),
                );
            }
            for (c, name) in pages.iter().filter(|(c, _)| in_folder(*c) == prefix) {
                out += &page_row(*c, name, prefix);
            }
            out
        }
        let folder_tools = |f: &str| -> String {
            if !role.organizes() {
                return String::new();
            }
            format!(
                r#"<details class="tools"><summary>Folder</summary><form method="post" action="/console" class="row">{h}<input type="hidden" name="folder" value="{v}"><input name="name" value="{l}" required><button class="btn">Rename folder</button></form>{del}</details>"#,
                h = hidden("rename_folder"),
                v = esc(f),
                l = esc(f.rsplit('/').next().unwrap_or(f)),
                del = if role == Role::Admin {
                    format!(
                        r#"<form method="post" action="/console" data-confirm="Remove the folder {v}? What's in it moves up a level.">{d}<input type="hidden" name="folder" value="{v}"><button class="btn danger">Remove folder</button></form>"#,
                        d = hidden("delete_folder"),
                        v = esc(f)
                    )
                } else {
                    String::new()
                },
            )
        };
        let list = tree(
            "",
            &self.folders.folders,
            &pages,
            &in_folder,
            &page_row,
            &folder_tools,
        );
        let list = if list.is_empty() {
            r#"<li class="page"><span>No pages yet.</span></li>"#.to_string()
        } else {
            list
        };
        let mut make = String::new();
        if role == Role::Admin {
            make += &format!(
                r#"<form method="post" action="/console" class="row">{h}<input name="name" placeholder="New page name" required><select name="folder">{o}</select><button class="btn primary">New page</button></form>"#,
                h = hidden("new_page"),
                o = folder_opts(""),
            );
        }
        if role.organizes() {
            make += &format!(
                r#"<form method="post" action="/console" class="row">{h}<input name="name" placeholder="New folder name" required><select name="parent">{o}</select><button class="btn">New folder</button></form>"#,
                h = hidden("new_folder"),
                o = folder_opts(""),
            );
        }
        let mut deleted = String::new();
        if role == Role::Admin && !self.folders.deleted.is_empty() {
            let mut rows = String::new();
            for (id, (name, folder)) in &self.folders.deleted {
                rows += &format!(
                    r#"<li class="page"><div class="pline"><span class="ic">✕</span><b>{n}</b><small>{f}</small><span class="acts"><form method="post" action="/console">{h}<input type="hidden" name="page" value="{id}"><button class="btn">Restore</button></form></span></div></li>"#,
                    n = esc(name),
                    f = if folder.is_empty() {
                        String::new()
                    } else {
                        esc(folder)
                    },
                    h = hidden("restore"),
                    id = esc(id),
                );
            }
            deleted = format!(
                r#"<section class="card"><h2>Tagged for deletion</h2><ul class="tree">{rows}</ul><p class="note">The files stay on the server as <code>.ogp.deleted</code> until someone removes them there.</p></section>"#
            );
        }
        let body = format!(
            r#"<section class="card"><h2>Pages</h2>{make}<ul class="tree">{list}</ul>
<p class="note"><b>Open</b> opens the page in the web app; <b>Copy link</b> copies the page's link for the desktop and mobile apps (Share live › Join). Anyone with a page's link can draw on it.</p></section>{deleted}"#
        );
        self.shell("Pages", "/console", server, Some((user, csrf)), msg, &body)
    }
}

/// Where to go after signing in: a path on this server, else the pages.
fn safe_next(next: Option<String>) -> String {
    match next {
        Some(n)
            if n.starts_with('/') && !n.starts_with("//") && !n.contains('\\') && n != "/login" =>
        {
            n
        }
        _ => "/console".into(),
    }
}

/// The tab bar: Home, Pages, Users (admins), Password. Signed out, the
/// console tabs are greyed and lead to sign-in, then back to the tab.
fn nav(user: Option<(&User, &str)>, active: &str) -> String {
    let tab = |href: &str, label: &str, live: bool| -> String {
        let on = if href == active { " on" } else { "" };
        if live {
            format!(r#"<a class="tab{on}" href="{href}">{label}</a>"#)
        } else {
            format!(
                r#"<a class="tab off{on}" href="/login?next={}" title="Sign in to use {label}">{label}</a>"#,
                url_encode(href)
            )
        }
    };
    let signed = user.is_some();
    let admin = user.is_some_and(|(u, _)| u.role == Role::Admin);
    let mut tabs = tab("/", "Home", true);
    tabs += &tab("/console", "Pages", signed);
    if admin || !signed {
        tabs += &tab("/console/users", "Users", admin);
    }
    tabs += &tab("/console/password", "Password", signed);
    let right = match user {
        Some((u, csrf)) => format!(
            r#"<span class="who">{name} · {role}</span>
<form method="post" action="/logout"><input type="hidden" name="csrf" value="{csrf}"><button class="btn">Sign out</button></form>"#,
            name = esc(&u.name),
            role = u.role.label(),
        ),
        None if active.is_empty() => String::new(),
        None => r#"<a class="btn primary who-btn" href="/login">Sign in</a>"#.into(),
    };
    format!(r#"<nav class="tabs">{tabs}{right}</nav>"#)
}

/// Styles for the tab bar (shared with the home page).
pub const NAV_CSS: &str = r#"
nav.tabs { display:flex; flex-wrap:wrap; align-items:center; gap:6px; margin:6px 0 16px; border-bottom:1px solid #2b2f37; padding-bottom:10px; }
nav.tabs .tab { color:#eef0e8; text-decoration:none; font-weight:600; padding:6px 12px; border-radius:10px; }
nav.tabs .tab:hover { background:#22262e; } nav.tabs .tab.on { background:#22262e; color:#e2457a; }
nav.tabs .tab.off { color:#5c6069; } nav.tabs .tab.off:hover { color:#a0a3ab; background:none; }
nav.tabs .who { margin-left:auto; color:#a0a3ab; font-size:.9rem; } nav.tabs .who-btn { margin-left:auto; }
nav.tabs form { margin:0; }
"#;

const CSS: &str = r#"
:root { --bg:#101216; --card:#191c22; --ink:#eef0e8; --muted:#a0a3ab; --edge:#2b2f37; --pink:#e2457a; --warn:#f2c46d; }
* { box-sizing:border-box; }
body { margin:0; background:var(--bg); color:var(--ink); font:16px/1.5 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif; }
main { max-width:960px; margin:0 auto; padding:26px 16px 60px; }
header { display:flex; align-items:center; gap:14px; margin-bottom:16px; }
header svg { width:52px; height:52px; } header .logo { display:flex; }
h1 { margin:0; font-size:1.5rem; } header p { margin:0; color:var(--muted); }
h2 { font-size:1.05rem; margin:0 0 12px; }
.card { background:var(--card); border:1px solid var(--edge); border-radius:16px; padding:16px 18px; margin:14px 0; }
.card.narrow { max-width:440px; }
.flash { background:#1f2a22; border:1px solid #2f4a36; color:#bfe8c6; border-radius:12px; padding:10px 14px; }
.flash.warn { background:#2c2618; border-color:#5a4a22; color:var(--warn); } .flash a { color:inherit; }
.btn { border:1px solid var(--edge); background:#22262e; color:var(--ink); border-radius:10px; padding:7px 12px; font:inherit; font-weight:600; cursor:pointer; text-decoration:none; white-space:nowrap; display:inline-block; }
.btn:hover { border-color:#454b56; } .btn.primary { background:var(--pink); border-color:var(--pink); color:#fff; }
.btn.danger { color:#ff9aa8; border-color:#5a2a33; }
input, select { background:#0c0e11; color:var(--ink); border:1px solid var(--edge); border-radius:10px; padding:8px 10px; font:inherit; min-width:0; }
.stack { display:flex; flex-direction:column; gap:12px; } .stack label { display:flex; flex-direction:column; gap:6px; font-weight:600; }
.row { display:flex; flex-wrap:wrap; gap:8px; align-items:center; margin:6px 0; } .row input { flex:1 1 180px; }
form.inline { display:flex; gap:6px; align-items:center; margin:0; }
ul.tree, ul.tree ul { list-style:none; margin:0; padding:0; } ul.tree ul { padding-left:20px; border-left:1px dashed var(--edge); margin-left:8px; }
li.folder > details > summary { cursor:pointer; padding:7px 4px; list-style:none; display:flex; gap:8px; align-items:center; }
li.folder > details[open] > summary .ic { transform:rotate(90deg); display:inline-block; }
li.page { padding:6px 4px; border-top:1px solid var(--edge); } ul.tree > li.page:first-child { border-top:0; }
.pline { display:flex; flex-wrap:wrap; gap:10px; align-items:center; }
.pline time, .pline small { color:var(--muted); font-size:.85rem; } .pline .acts { margin-left:auto; display:flex; gap:6px; align-items:center; }
.pline .acts form { margin:0; }
.ic { color:var(--muted); width:16px; text-align:center; }
details.tools { margin:4px 0 2px 26px; } details.tools > summary { cursor:pointer; color:var(--muted); font-size:.88rem; }
table { width:100%; border-collapse:collapse; } th, td { text-align:left; padding:8px 6px; border-top:1px solid var(--edge); vertical-align:middle; }
th { color:var(--muted); font-weight:600; font-size:.88rem; border-top:0; } .scroll { overflow-x:auto; }
small { color:var(--muted); } .warnt { color:var(--warn); }
.note { color:var(--muted); font-size:.9rem; margin:12px 0 0; } a { color:var(--ink); } code { color:var(--ink); }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, target: &str, cookie: Option<&str>, body: &str) -> WebReq {
        let mut headers = vec![];
        if let Some(c) = cookie {
            headers.push(("cookie".to_string(), c.to_string()));
        }
        WebReq {
            method: method.into(),
            target: target.into(),
            base: "ws://test:8991".into(),
            headers,
            body: body.as_bytes().to_vec(),
        }
    }

    fn sign_in(c: &mut Console, user: &str, pw: &str) -> (String, String) {
        let r = c.handle(
            &req(
                "POST",
                "/login",
                None,
                &format!("user={user}&password={pw}"),
            ),
            "T",
        );
        assert_eq!(r.status, 303, "sign in as {user}");
        let cookie = r
            .headers
            .iter()
            .find(|(k, _)| k == "Set-Cookie")
            .unwrap()
            .1
            .split(';')
            .next()
            .unwrap()
            .to_string();
        let token = cookie.split('=').nth(1).unwrap().to_string();
        let csrf = c.sessions[&token].csrf.clone();
        (cookie, csrf)
    }

    #[test]
    fn roles_decide_what_the_console_allows() {
        let dir = std::env::temp_dir().join(format!("ogp-console-{}", crate::uid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = Console::load(&dir);
        // The default admin, then a wrong password.
        assert_eq!(
            c.handle(
                &req("POST", "/login", None, "user=admin&password=nope"),
                "T"
            )
            .status,
            200
        );
        let (admin, acsrf) = sign_in(&mut c, "admin", "password");
        assert_eq!(
            c.handle(&req("GET", "/console", None, ""), "T").status,
            303,
            "not signed in"
        );
        let page = c.handle(&req("GET", "/console", Some(&admin), ""), "T");
        assert!(String::from_utf8(page.body)
            .unwrap()
            .contains("temporary password"));
        // A form without the session's token is refused.
        assert_eq!(
            c.handle(
                &req("POST", "/console", Some(&admin), "action=new_folder&name=X"),
                "T"
            )
            .status,
            403
        );
        // Admin makes a folder and a page in it, and two users.
        let post = |c: &mut Console, cookie: &str, path: &str, body: String| {
            c.handle(&req("POST", path, Some(cookie), &body), "T")
        };
        post(
            &mut c,
            &admin,
            "/console",
            format!("csrf={acsrf}&action=new_folder&name=Art&parent="),
        );
        post(
            &mut c,
            &admin,
            "/console",
            format!("csrf={acsrf}&action=new_page&name=Mural&folder=Art"),
        );
        let (mural, _) = read_index(&dir)
            .into_iter()
            .find(|p| p.1 == "Mural")
            .unwrap();
        assert_eq!(c.folders.pages[&format!("{mural:032x}")], "Art");
        post(
            &mut c,
            &admin,
            "/console/users",
            format!("csrf={acsrf}&action=add&name=sam&password=secret123&role=subadmin"),
        );
        post(
            &mut c,
            &admin,
            "/console/users",
            format!("csrf={acsrf}&action=add&name=uma&password=secret123&role=user"),
        );
        // The subadmin renames and moves, but can't delete.
        let (sam, scsrf) = sign_in(&mut c, "sam", "secret123");
        let id = format!("{mural:032x}");
        post(
            &mut c,
            &sam,
            "/console",
            format!("csrf={scsrf}&action=rename&page={id}&name=Big+mural"),
        );
        assert!(read_index(&dir).iter().any(|p| p.1 == "Big mural"));
        post(
            &mut c,
            &sam,
            "/console",
            format!("csrf={scsrf}&action=move&page={id}&folder="),
        );
        assert!(!c.folders.pages.contains_key(&id));
        post(
            &mut c,
            &sam,
            "/console",
            format!("csrf={scsrf}&action=delete&page={id}"),
        );
        assert!(
            read_index(&dir).iter().any(|p| p.0 == mural),
            "subadmin can't delete"
        );
        assert_eq!(
            c.handle(&req("GET", "/console/users", Some(&sam), ""), "T")
                .status,
            200
        );
        // The user only sees pages; their actions are refused.
        let (uma, ucsrf) = sign_in(&mut c, "uma", "secret123");
        post(
            &mut c,
            &uma,
            "/console",
            format!("csrf={ucsrf}&action=rename&page={id}&name=Nope"),
        );
        assert!(read_index(&dir).iter().any(|p| p.1 == "Big mural"));
        let view =
            String::from_utf8(c.handle(&req("GET", "/console", Some(&uma), ""), "T").body).unwrap();
        assert!(view.contains("Big mural") && !view.contains("Rename"));
        // Admin deletes (tagged), then restores.
        post(
            &mut c,
            &admin,
            "/console",
            format!("csrf={acsrf}&action=delete&page={id}"),
        );
        assert!(!read_index(&dir).iter().any(|p| p.0 == mural));
        assert!(page_path(&dir, mural)
            .with_extension("ogp.deleted")
            .exists());
        post(
            &mut c,
            &admin,
            "/console",
            format!("csrf={acsrf}&action=restore&page={id}"),
        );
        assert!(read_index(&dir).iter().any(|p| p.0 == mural) && page_path(&dir, mural).exists());
        // Changing the password clears the warning; users survive a reload.
        post(
            &mut c,
            &admin,
            "/console/password",
            format!("csrf={acsrf}&old=password&new=correct-horse&again=correct-horse"),
        );
        let c2 = Console::load(&dir);
        assert!(c2.users.iter().any(|u| u.name == "admin" && !u.must_change));
        assert!(c2
            .users
            .iter()
            .any(|u| u.name == "sam" && u.role == Role::Subadmin));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
