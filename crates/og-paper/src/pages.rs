// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Pages: the canvases on this device and on the servers you added.
//!
//! A server (see `hub`) is added once by its server link and kept in the
//! prefs (`servers`, space separated). Its page list is fetched over a
//! short sealed connection to its directory. Opening a server page keeps a
//! copy on this device that remembers its link (`link.<canvas>` in the
//! prefs), so opening that copy later reconnects by itself and your
//! offline work goes up.
//!
//! On desktop the pages on this device are the canvas files in the OG Paper
//! folder; the web page keeps one copy per canvas in browser storage and
//! hands the list over (`og_set_pages`).

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use crate::net::{parse_link, Ev, Link};
use crate::seal::Keys;
use crate::ui::{LocalPage, ServerView};
use crate::wire::{Msg, PageInfo};
use crate::App;

/// The open page on its way into a workspace (see `workspace_upload`).
pub struct Upload {
    server: usize,
    bytes: Vec<u8>,
    /// The page the workspace made for it, once it answers.
    canvas: u128,
}

/// Connection ids for directory requests (server i uses DIR_CONN + i).
pub const DIR_CONN: u64 = 1000;

pub struct ServerConn {
    pub link: Link,
    keys: Keys,
    pub pages: Vec<PageInfo>,
    pub state: String,
    /// The requests to send once connected.
    pending: Vec<Msg>,
    /// Signed in to an account there: name, role, token.
    account: Option<(String, String, String)>,
    /// The workspace's folders, and the folder of each page in one.
    folders: Vec<String>,
    in_folder: std::collections::HashMap<u128, String>,
    #[cfg(not(target_arch = "wasm32"))]
    client: Option<crate::net::native::Client>,
}

/// The servers kept in the prefs.
pub fn load_servers() -> Vec<ServerConn> {
    crate::prefs::load()
        .get("servers")
        .map(|s| {
            s.split_whitespace()
                .filter_map(|l| {
                    let link = parse_link(l)?;
                    let keys = Keys::parse(&link.key)?;
                    let account = load_account(&link.url);
                    Some(ServerConn {
                        link,
                        keys,
                        pages: Vec::new(),
                        state: "Not checked yet".into(),
                        pending: Vec::new(),
                        account,
                        folders: Vec::new(),
                        in_folder: Default::default(),
                        #[cfg(not(target_arch = "wasm32"))]
                        client: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The account remembered for a server (name, role, token).
fn load_account(url: &str) -> Option<(String, String, String)> {
    let v = crate::prefs::load().get(&format!("account.{url}"))?.clone();
    let mut f = v.split('\t');
    Some((f.next()?.into(), f.next()?.into(), f.next()?.into()))
}

fn save_account(url: &str, a: Option<&(String, String, String)>) {
    let mut p = crate::prefs::load();
    let k = format!("account.{url}");
    match a {
        Some((u, r, t)) => p.insert(k, format!("{u}\t{r}\t{t}")),
        None => p.remove(&k),
    };
    crate::prefs::save(&p);
}

/// A role's name to show.
fn role_label(r: &str) -> &'static str {
    match r {
        "admin" => "Admin",
        "subadmin" => "Subadmin",
        "viewer" => "Viewer",
        _ => "User",
    }
}

fn save_servers(all: &[ServerConn]) {
    let mut p = crate::prefs::load();
    let v: Vec<String> = all
        .iter()
        .map(|s| format!("{}/?k={}", s.link.url, s.link.key))
        .collect();
    p.insert("servers".into(), v.join(" "));
    crate::prefs::save(&p);
}

/// A server's short name: its host.
fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

/// Canvas files in the OG Paper folder, newest first (desktop).
#[cfg(not(target_arch = "wasm32"))]
pub fn local_files() -> Vec<(PathBuf, String, u64)> {
    let Some(dir) = crate::canvas_dir() else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, String, u64)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "ogp"))
                .map(|p| {
                    let t = std::fs::metadata(&p)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_millis() as u64);
                    let name = p
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    (p, name, t)
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.2.cmp(&a.2));
    out
}

impl App {
    /// Open or close the Pages panel (listing what is here, and checking
    /// the servers).
    pub(crate) fn pages_toggle(&mut self) {
        self.ui.pages_open = !self.ui.pages_open;
        self.ui.menu = crate::ui::Menu::None;
        if self.ui.pages_open {
            if self.servers.is_empty() {
                self.servers = load_servers();
            }
            #[cfg(not(target_arch = "wasm32"))]
            self.refresh_local();
            for i in 0..self.servers.len() {
                self.server_request(i, Msg::ListPages);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn refresh_local(&mut self) {
        let current = self.file.as_ref().map(|f| f.path().to_path_buf());
        self.local_paths.clear();
        self.ui.local_pages = local_files()
            .into_iter()
            .map(|(p, name, t)| {
                let page = LocalPage {
                    key: String::new(),
                    name,
                    changed: t,
                    current: current.as_ref() == Some(&p),
                };
                self.local_paths.push(p);
                page
            })
            .collect();
    }

    /// Rename page `i` on this device: its file (desktop; the open one is
    /// closed, renamed and reopened, keeping the canvas and its undo).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn rename_local(&mut self, i: usize, name: String) {
        let Some(from) = self.local_paths.get(i).cloned() else {
            return;
        };
        let Some(dir) = from.parent() else { return };
        let safe: String = name
            .chars()
            .map(|c| {
                if "/\\:*?\"<>|".contains(c) || c.is_control() {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let to = dir.join(format!("{}.ogp", safe.trim_end_matches('.')));
        if to == from {
            return;
        }
        if to.exists() {
            return self.say(format!("There is already a page called {name} here"));
        }
        let current = self.file.as_ref().is_some_and(|f| f.path() == from);
        if current {
            // Closed first: some systems will not rename an open file.
            self.file = None;
        }
        let r = std::fs::rename(&from, &to);
        if current {
            let at = if r.is_ok() { &to } else { &from };
            match ogpaper_file::OgpFile::open(at) {
                Ok((f, ..)) => {
                    self.file = Some(f);
                    self.remember_file();
                    if r.is_ok() {
                        self.ui.file_name = crate::file_label(&to);
                    }
                }
                Err(e) => self.say(format!("Could not reopen the page: {e}")),
            }
        }
        match r {
            Ok(()) => self.say(format!("Renamed to {name}")),
            Err(e) => self.say(format!("Could not rename it: {e}")),
        }
        self.refresh_local();
    }

    /// Rename page `i` kept in this browser (its name in the page's list).
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn rename_local(&mut self, i: usize, name: String) {
        let Some(p) = self.ui.local_pages.get(i) else {
            return;
        };
        if p.current {
            self.ui.file_name = name.clone();
        }
        let key = p.key.clone();
        crate::web::page_request("rename-page", &format!("{key}\t{name}"));
    }

    /// What the panel shows of the servers.
    pub(crate) fn server_views(&self) -> Vec<ServerView> {
        self.servers
            .iter()
            .map(|s| ServerView {
                name: host_of(&s.link.url),
                state: s.state.clone(),
                role: if s.keys.can_edit() {
                    "admin".into()
                } else {
                    s.account.as_ref().map(|a| a.1.clone()).unwrap_or_default()
                },
                folders: s.folders.clone(),
                account: s
                    .account
                    .as_ref()
                    .map(|a| (a.0.clone(), role_label(&a.1).to_string())),
                pages: s
                    .pages
                    .iter()
                    .map(|p| {
                        (
                            p.name.clone(),
                            p.changed,
                            p.canvas == self.share.canvas,
                            s.in_folder.get(&p.canvas).cloned().unwrap_or_default(),
                        )
                    })
                    .collect(),
            })
            .collect()
    }

    /// Add a server by its link and list its pages.
    pub(crate) fn add_server(&mut self, link: &str) {
        let Some(l) = parse_link(link) else {
            self.say("That is not a server link (it starts with ws:// or wss://)");
            return;
        };
        let Some(keys) = Keys::parse(&l.key) else {
            self.say("That link's key is incomplete: copy the whole server link");
            return;
        };
        if self.servers.iter().any(|s| s.link.url == l.url) {
            self.say("That server is already in the list");
            return;
        }
        let account = load_account(&l.url);
        self.servers.push(ServerConn {
            link: l,
            keys,
            pages: Vec::new(),
            state: "Checking…".into(),
            pending: Vec::new(),
            account,
            folders: Vec::new(),
            in_folder: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            client: None,
        });
        save_servers(&self.servers);
        self.ui.add_server_text.clear();
        self.server_request(self.servers.len() - 1, Msg::ListPages);
    }

    pub(crate) fn remove_server(&mut self, i: usize) {
        if i < self.servers.len() {
            self.servers.remove(i);
            save_servers(&self.servers);
        }
    }

    /// Send `m` to server `i`'s directory (it answers with the page list).
    pub(crate) fn server_request(&mut self, i: usize, m: Msg) {
        let Some(s) = self.servers.get_mut(i) else {
            return;
        };
        s.pending = vec![m];
        s.state = "Checking…".into();
        self.dir_connect(i);
    }

    /// Sign in to an account on server `i` (from the Pages panel).
    pub(crate) fn server_account_in(&mut self, i: usize, user: &str, password: &str) {
        let Some(s) = self.servers.get_mut(i) else {
            return;
        };
        s.account = None;
        s.pending = vec![
            Msg::AccountIn {
                user: user.into(),
                password: password.into(),
                token: String::new(),
            },
            Msg::ListPages,
        ];
        s.state = "Signing in…".into();
        self.dir_connect(i);
    }

    /// Change workspace `i`'s pages or folders (see `Msg::Change`); page
    /// `p` is an index into its page list, when the change is to a page.
    pub(crate) fn workspace_change(
        &mut self,
        i: usize,
        op: &str,
        p: Option<usize>,
        name: &str,
        folder: &str,
    ) {
        let page = p
            .and_then(|p| self.servers.get(i)?.pages.get(p))
            .map_or(0, |p| p.canvas);
        self.server_request(
            i,
            Msg::Change {
                op: op.into(),
                page,
                name: name.into(),
                folder: folder.into(),
            },
        );
    }

    /// Put the open page into workspace `i` (in `folder`): a new page
    /// there, opened here, with this page's ink and objects brought in.
    pub(crate) fn workspace_upload(&mut self, i: usize, folder: &str) {
        let bytes = crate::snapshot::encode(
            &self.scene,
            &self.cam,
            &self.timeline,
            &self.bookmarks,
            &self.objs,
            &self.share.copy_log(),
        );
        let name = self.ui.file_name.clone();
        self.upload = Some(Upload {
            server: i,
            bytes,
            canvas: 0,
        });
        self.say(format!("Uploading {name}…"));
        self.workspace_change(i, "new_page", None, &name, folder);
    }

    /// After joining a page: if it is the one an upload made, bring the
    /// uploaded page's content in.
    pub(crate) fn upload_arrived(&mut self, canvas: u128) {
        if !self
            .upload
            .as_ref()
            .is_some_and(|u| u.canvas == canvas && canvas != 0)
        {
            return;
        }
        let Some(up) = self.upload.take() else { return };
        match crate::snapshot::decode(&up.bytes, crate::BASE_PX) {
            Ok(s) => {
                self.import_begin(s.scene, s.objs, "that page");
                self.import_finish(true);
                self.cam = s.cam;
                self.say("Uploaded: it's in the workspace now");
            }
            Err(e) => self.say(format!("Could not upload it: {e}")),
        }
        self.redraw();
    }

    /// Forget the account on server `i`: back to what its link allows.
    pub(crate) fn server_account_out(&mut self, i: usize) {
        if let Some(s) = self.servers.get_mut(i) {
            s.account = None;
            save_account(&s.link.url, None);
            self.server_request(i, Msg::ListPages);
        }
    }

    fn dir_connect(&mut self, i: usize) {
        let Some(s) = self.servers.get_mut(i) else {
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            s.client = Some(crate::net::native::Client::connect(s.link.url.clone()));
        }
        #[cfg(target_arch = "wasm32")]
        crate::web::dir_connect(DIR_CONN + i as u64, &s.link.url);
    }

    /// Directory connections: take what came in (desktop polls here; the
    /// web page's events come through `dir_event`).
    pub(crate) fn pages_tick(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        for i in 0..self.servers.len() {
            let evs = self.servers[i]
                .client
                .as_ref()
                .map(|c| c.poll())
                .unwrap_or_default();
            for e in evs {
                let e = match e {
                    Ev::Open(_) => Ev::Open(DIR_CONN + i as u64),
                    Ev::Data(_, b) => Ev::Data(DIR_CONN + i as u64, b),
                    Ev::Closed(_, w) => Ev::Closed(DIR_CONN + i as u64, w),
                };
                self.dir_event(e);
            }
        }
    }

    /// An event on a directory connection.
    pub(crate) fn dir_event(&mut self, e: Ev) {
        let conn = match &e {
            Ev::Open(c) | Ev::Data(c, _) | Ev::Closed(c, _) => *c,
        };
        let i = (conn - DIR_CONN) as usize;
        let Some(s) = self.servers.get_mut(i) else {
            return;
        };
        match e {
            Ev::Open(_) => {
                let mut out = std::mem::take(&mut s.pending);
                // A remembered account signs in ahead of the request.
                if let Some((user, _, token)) = &s.account {
                    if !matches!(out.first(), Some(Msg::AccountIn { .. })) {
                        out.insert(
                            0,
                            Msg::AccountIn {
                                user: user.clone(),
                                password: String::new(),
                                token: token.clone(),
                            },
                        );
                    }
                }
                let sealed: Vec<Vec<u8>> =
                    out.iter().map(|m| s.keys.seal(&m.encode(), true)).collect();
                for b in sealed {
                    self.dir_send(i, b);
                }
            }
            Ev::Data(_, b) => {
                if b == b"OG-PAPER:BAD-KEY" {
                    s.state = "The server did not accept this link".into();
                    return;
                }
                let msg = s.keys.open(&b).map(|(p, _)| Msg::decode(&p));
                if let Some(Ok(Msg::Account {
                    user,
                    role,
                    token,
                    note,
                })) = msg
                {
                    let say = if token.is_empty() {
                        s.account = None;
                        save_account(&s.link.url, None);
                        Some(note)
                    } else {
                        let a = (user, role, token);
                        save_account(&s.link.url, Some(&a));
                        let first = s.account.is_none();
                        let msg = format!("{note} ({})", role_label(&a.1));
                        s.account = Some(a);
                        first.then_some(msg)
                    };
                    if let Some(m) = say {
                        self.say(m);
                    }
                    self.redraw();
                    return;
                }
                if let Some(Ok(Msg::Folders { folders, pages })) = msg {
                    s.folders = folders;
                    s.in_folder = pages.into_iter().collect();
                    return;
                }
                if let Some(Ok(Msg::Changed { note, ok, made })) = msg {
                    if let Some(up) = self
                        .upload
                        .as_mut()
                        .filter(|u| u.server == i && u.canvas == 0)
                    {
                        if ok && made != 0 {
                            up.canvas = made;
                        } else {
                            self.upload = None;
                        }
                    }
                    if !ok || self.upload.is_none() {
                        self.say(note);
                    }
                    self.redraw();
                    return;
                }
                if let Some(Ok(Msg::Pages(list))) = msg {
                    s.state = format!(
                        "{} page{}",
                        list.len(),
                        if list.len() == 1 { "" } else { "s" }
                    );
                    s.pages = list;
                    self.dir_close(i);
                    let made = self
                        .upload
                        .as_ref()
                        .filter(|u| u.server == i)
                        .map(|u| u.canvas);
                    if let Some(c) = made.filter(|c| *c != 0) {
                        if let Some(p) = self.servers[i].pages.iter().position(|p| p.canvas == c) {
                            self.open_server_page(i, p);
                        }
                    }
                }
            }
            Ev::Closed(_, why) => {
                if s.state == "Checking…" || s.state == "Signing in…" {
                    s.state = format!("Could not reach it ({why})");
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    s.client = None;
                }
            }
        }
        self.redraw();
    }

    fn dir_send(&mut self, i: usize, b: Vec<u8>) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(c) = self.servers.get(i).and_then(|s| s.client.as_ref()) {
            c.send(b);
        }
        #[cfg(target_arch = "wasm32")]
        crate::web::net_send(DIR_CONN + i as u64, b);
    }

    fn dir_close(&mut self, i: usize) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(s) = self.servers.get_mut(i) {
            s.client = None;
        }
        #[cfg(target_arch = "wasm32")]
        crate::web::dir_close(DIR_CONN + i as u64);
    }

    /// The link that joins page `p` of server `i`.
    fn page_link(&self, i: usize, p: usize) -> Option<(u128, String)> {
        let s = self.servers.get(i)?;
        let page = s.pages.get(p)?;
        let key = if page.edit.is_empty() {
            &page.view
        } else {
            &page.edit
        };
        Some((
            page.canvas,
            format!("{}/p/{:032x}?k={key}", s.link.url, page.canvas),
        ))
    }

    /// Open a server page: this device's copy if it has one, connected to
    /// the server (a new copy if not). The link is remembered so the copy
    /// reconnects whenever it is opened.
    pub(crate) fn open_server_page(&mut self, i: usize, p: usize) {
        let Some((canvas, link)) = self.page_link(i, p) else {
            return;
        };
        let mut prefs = crate::prefs::load();
        prefs.insert(format!("link.{canvas:032x}"), link.clone());
        crate::prefs::save(&prefs);
        if canvas == self.share.canvas {
            if self.net.is_none() {
                self.join(&link);
            }
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let known = crate::prefs::load()
                .get(&format!("file.{canvas:032x}"))
                .map(PathBuf::from)
                .filter(|p| p.exists());
            match known {
                // open_file reconnects by the remembered link.
                Some(path) => self.open_file(path),
                None => self.join(&link),
            }
            self.refresh_local();
        }
        // The page saves the open canvas, opens its copy if it has one,
        // then joins.
        #[cfg(target_arch = "wasm32")]
        crate::web::open_shared(canvas, &link);
        self.ui.pages_open = false;
    }

    /// The pages kept in this browser, from the page's JSON
    /// (`[{"c": "<canvas hex>", "name": "…", "t": ms}]`).
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn web_pages(&mut self, json: &str) {
        let me = format!("{:032x}", self.share.canvas);
        let field = |obj: &str, name: &str| -> Option<String> {
            let k = format!("\"{name}\":");
            let i = obj.find(&k)? + k.len();
            let rest = obj[i..].trim_start();
            if let Some(r) = rest.strip_prefix('"') {
                let mut out = String::new();
                let mut esc = false;
                for ch in r.chars() {
                    match (esc, ch) {
                        (true, c) => {
                            out.push(c);
                            esc = false;
                        }
                        (false, '\\') => esc = true,
                        (false, '"') => break,
                        (false, c) => out.push(c),
                    }
                }
                Some(out)
            } else {
                Some(rest.split([',', '}']).next()?.trim().to_string())
            }
        };
        self.ui.local_pages = json
            .split('{')
            .skip(1)
            .filter_map(|obj| {
                let key = field(obj, "c")?;
                Some(LocalPage {
                    current: key == me,
                    name: field(obj, "name").unwrap_or_else(|| "Untitled".into()),
                    changed: field(obj, "t").and_then(|t| t.parse().ok()).unwrap_or(0),
                    key,
                })
            })
            .collect();
    }

    /// Reconnect a canvas that was opened from a server, if it was.
    pub(crate) fn reconnect_page(&mut self) {
        if self.net.is_some() || self.headless {
            return;
        }
        let link = crate::prefs::load()
            .get(&format!("link.{:032x}", self.share.canvas))
            .cloned();
        if let Some(link) = link {
            self.join(&link);
        }
    }
}
