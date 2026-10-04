// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! A server for many pages: `og-paper --serve-dir DIR [--port N]
//! [--public wss://host]`, made to run in a container on a NAS.
//!
//! Each page is a canvas file `DIR/<canvas>.ogp`, named in `DIR/pages.txt`
//! (`<canvas>\t<name>` per line). One listener takes every connection:
//! `/p/<canvas>` joins that page (hosted like a desktop host, loaded when
//! someone comes, unloaded when everyone has left a while), anything else
//! is the directory, where apps list, make, rename and delete pages.
//!
//! `/api` is the automation API: JSON text frames for scripts and
//! services (see `docs/PLUGINS.md`). Its first message is
//! `{"auth": "<server key>"}`; then `pages`, `new_page`, `rename_page`,
//! `delete_page`, `run` (commands, see `script`), `texts` and `watch`.
//!
//! The directory has its own key, made on first start (`DIR/server.key`,
//! or the `OGP_SERVER_KEY` environment variable) and printed as a server
//! link: its edit form can list pages with their edit keys and change the
//! list; its view form lists them with view keys only. Frames are sealed
//! like everything else live (see `seal`).

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use web_time::{Duration, Instant};

use std::sync::{Arc, Mutex};

use crate::hubconsole::{Console, HubOp};
use crate::net::native::{Server, WebResp};
use crate::net::{page_keys_for, Ev};
use crate::seal::Keys;
use crate::wire::{Msg, PageInfo};
use crate::App;

/// A page nobody has had open for this long is put away (its file stays).
const IDLE: Duration = Duration::from_secs(300);

struct Page {
    app: App,
    conns: HashSet<u64>,
    empty_since: Option<Instant>,
    /// The log size last time watchers were told of changes.
    seen_len: usize,
}

/// What a connection is.
enum Conn {
    /// On page `canvas`.
    Page(u128),
    /// In the directory (apps' Pages list).
    Dir,
    /// The automation API: whether it may change things (once signed in),
    /// and the pages it watches.
    Api(Option<bool>, HashSet<u128>),
}

fn index_path(dir: &Path) -> PathBuf {
    dir.join("pages.txt")
}

/// Held while the index is read and rewritten (the console runs on the web
/// thread).
pub(crate) static INDEX: Mutex<()> = Mutex::new(());

pub(crate) fn page_path(dir: &Path, canvas: u128) -> PathBuf {
    dir.join(format!("{canvas:032x}.ogp"))
}

/// The pages and their names, in the order they were made.
pub(crate) fn read_index(dir: &Path) -> Vec<(u128, String)> {
    std::fs::read_to_string(index_path(dir))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (c, n) = l.split_once('\t')?;
            Some((
                u128::from_str_radix(c.trim(), 16).ok()?,
                n.trim().to_string(),
            ))
        })
        .collect()
}

pub(crate) fn write_index(dir: &Path, pages: &[(u128, String)]) {
    let text: String = pages
        .iter()
        .map(|(c, n)| format!("{c:032x}\t{}\n", n.replace(['\t', '\n'], " ")))
        .collect();
    let tmp = dir.join("pages.txt.part");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, index_path(dir));
    }
}

/// Make a page: an empty canvas file carrying its id.
pub(crate) fn make_page(dir: &Path, name: &str) -> Option<u128> {
    let canvas = crate::uid::new();
    let f = ogpaper_file::OgpFile::create(&page_path(dir, canvas)).ok()?;
    f.set_canvas_id(canvas).ok()?;
    let mut pages = read_index(dir);
    pages.push((canvas, clean_name(name)));
    write_index(dir, &pages);
    Some(canvas)
}

pub(crate) fn clean_name(n: &str) -> String {
    let n: String = n.trim().chars().take(80).collect();
    if n.is_empty() {
        "Untitled page".into()
    } else {
        n
    }
}

pub(crate) fn changed_ms(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64)
}

/// The server's directory key: from the environment, the key file, or new.
fn server_key(dir: &Path) -> String {
    if let Ok(k) = std::env::var("OGP_SERVER_KEY") {
        if Keys::parse(&k).is_some_and(|k| k.can_edit()) {
            return k;
        }
        eprintln!("OGP_SERVER_KEY is not an edit key (e…); using the key file");
    }
    let path = dir.join("server.key");
    if let Some(k) = std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|k| Keys::parse(k).is_some_and(|k| k.can_edit()))
    {
        return k;
    }
    let k = Keys::edit_token(&crate::seal::random_secret());
    let _ = std::fs::write(&path, &k);
    k
}

/// Serve the pages in `dir` until stopped.
pub fn run(dir: PathBuf, port: u16, public: Option<String>) {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("cannot use {}: {e}", dir.display());
        std::process::exit(1);
    }
    let token = server_key(&dir);
    let keys = Keys::parse(&token).expect("server key");
    let server = match Server::start(port) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot listen on port {port}: {e}");
            std::process::exit(1);
        }
    };
    if read_index(&dir).is_empty() {
        make_page(&dir, "First page");
    }
    let base = public
        .clone()
        .map(|p| p.trim_end_matches('/').to_string())
        .unwrap_or_else(|| format!("ws://{}:{}", crate::net::local_ip(), server.port));
    println!(
        "OG Paper server on port {}, pages in {}",
        server.port,
        dir.display()
    );
    println!("Server link (can make and change pages): {base}/?k={token}");
    println!(
        "Server link (view only):                 {base}/?k={}",
        keys.view_token()
    );
    println!("Add it in OG Paper: Pages > Add server. Keep it private: it opens every page.");
    // The page a browser sees at the server's address (see `hubpage`), and
    // the sign-in console under /console (see `hubconsole`).
    let console = Arc::new(Mutex::new(Console::load(&dir)));
    if std::env::var("OGP_WEB").map_or(true, |v| v != "off") {
        let (d, edit, view, public) = (
            dir.clone(),
            token.clone(),
            keys.view_token(),
            public.clone(),
        );
        let name = std::env::var("OGP_SERVER_NAME").unwrap_or_else(|_| "OG Paper server".into());
        let console = console.clone();
        server.set_web(move |req| {
            let path = req.target.split('?').next().unwrap_or("/");
            if path == "/login"
                || path == "/logout"
                || path == "/console"
                || path.starts_with("/console/")
            {
                let _g = INDEX.lock().unwrap_or_else(|e| e.into_inner());
                return console
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .handle(req, &name);
            }
            let (target, base) = (req.target.as_str(), req.base.as_str());
            let pages = read_index(&d)
                .into_iter()
                .map(|(c, n)| (n, changed_ms(&page_path(&d, c))))
                .collect();
            let info = crate::hubpage::Info {
                name: name.clone(),
                edit_key: edit.clone(),
                view_key: view.clone(),
                public: public.clone(),
                pages,
            };
            let nav = console
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .home_nav(req);
            let (status, ctype, body) = crate::hubpage::page(target, base, &info, &nav);
            WebResp {
                status,
                ctype,
                headers: vec![],
                body,
            }
        });
        println!(
            "In a browser: http://<this address>:{}/ shows how to connect.",
            server.port
        );
    }
    let mut pages: HashMap<u128, Page> = HashMap::new();
    let view_token = keys.view_token();
    let mut conns: HashMap<u64, Conn> = HashMap::new();
    loop {
        // What the console changed: rename or stop serving open pages.
        if let Ok(mut c) = console.try_lock() {
            for op in c.ops.drain(..) {
                match op {
                    HubOp::Renamed(canvas, name) => {
                        if let Some(p) = pages.get_mut(&canvas) {
                            p.app.ui.file_name = name;
                        }
                    }
                    HubOp::Unload(canvas) => {
                        pages.remove(&canvas);
                    }
                }
            }
        }
        for ev in server.poll() {
            match ev {
                Ev::Open(id) => {
                    let path = server.path(id).unwrap_or_default();
                    let page = path
                        .strip_prefix("/p/")
                        .and_then(|h| u128::from_str_radix(h.trim_matches('/'), 16).ok());
                    match page {
                        Some(c) if read_index(&dir).iter().any(|(p, _)| *p == c) => {
                            let p = pages
                                .entry(c)
                                .or_insert_with(|| open_page(&dir, c, &server));
                            p.conns.insert(id);
                            p.empty_since = None;
                            p.app.net_event(Ev::Open(id));
                            conns.insert(id, Conn::Page(c));
                        }
                        Some(_) => server.close(id),
                        None if path.trim_end_matches('/') == "/api" => {
                            conns.insert(id, Conn::Api(None, HashSet::new()));
                        }
                        None => {
                            conns.insert(id, Conn::Dir);
                        }
                    }
                }
                Ev::Data(id, b) => match conns.get_mut(&id) {
                    Some(Conn::Page(c)) => {
                        if let Some(p) = pages.get_mut(c) {
                            p.app.net_event(Ev::Data(id, b));
                        }
                    }
                    Some(Conn::Dir) => directory(&dir, &keys, &server, &mut pages, id, &b),
                    Some(Conn::Api(edit, watch)) => {
                        let reply = api(
                            &dir,
                            (&token, &view_token),
                            &server,
                            &mut pages,
                            (edit, watch),
                            &b,
                        );
                        let bad = reply.get("auth").and_then(Value::as_bool) == Some(false);
                        server.send_text(id, reply.to_string());
                        if bad {
                            server.close(id);
                        }
                    }
                    None => {}
                },
                Ev::Closed(id, why) => {
                    if let Some(Conn::Page(c)) = conns.remove(&id) {
                        if let Some(p) = pages.get_mut(&c) {
                            p.app.net_event(Ev::Closed(id, why));
                            p.conns.remove(&id);
                            if p.conns.is_empty() {
                                p.empty_since = Some(Instant::now());
                            }
                        }
                    }
                }
            }
        }
        let names: HashMap<u128, String> = read_index_cached(&dir);
        let mut changed = Vec::new();
        pages.retain(|c, p| {
            p.app.net_tick();
            if let Some(m) = p.app.ui.message.take() {
                println!("[{}] {m}", names.get(c).map_or("?", |n| n.as_str()));
            }
            let len = p.app.share.log.len();
            if len != p.seen_len {
                p.seen_len = len;
                changed.push(*c);
            }
            !p.empty_since.is_some_and(|t| t.elapsed() > IDLE)
        });
        // Tell API watchers which pages changed.
        for c in changed {
            for (id, conn) in &conns {
                if let Conn::Api(Some(_), watch) = conn {
                    if watch.contains(&c) {
                        let e = json!({"event": "changed", "page": format!("{c:032x}")});
                        server.send_text(*id, e.to_string());
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// The index, re-read at most every few seconds (names in the log).
fn read_index_cached(dir: &Path) -> HashMap<u128, String> {
    use std::sync::Mutex;
    static CACHE: Mutex<Option<(Instant, HashMap<u128, String>)>> = Mutex::new(None);
    let mut c = CACHE.lock().expect("cache");
    if c.as_ref()
        .is_none_or(|(t, _)| t.elapsed() > Duration::from_secs(5))
    {
        *c = Some((Instant::now(), read_index(dir).into_iter().collect()));
    }
    c.as_ref().map(|(_, m)| m.clone()).unwrap_or_default()
}

/// Load a page to host it.
fn open_page(dir: &Path, canvas: u128, server: &Server) -> Page {
    let mut app = App::new(None);
    app.headless = true;
    app.open_file(page_path(dir, canvas));
    if let Some(name) = read_index(dir)
        .into_iter()
        .find(|(c, _)| *c == canvas)
        .map(|p| p.1)
    {
        app.ui.file_name = name;
    }
    app.host_attach(server.handle());
    app.ui.message = None;
    let seen_len = app.share.log.len();
    Page {
        app,
        conns: HashSet::new(),
        empty_since: None,
        seen_len,
    }
}

/// A directory request: list, make, rename or delete pages.
fn directory(
    dir: &Path,
    keys: &Keys,
    server: &Server,
    pages: &mut HashMap<u128, Page>,
    id: u64,
    b: &[u8],
) {
    let Some((plain, signed)) = keys.open(b) else {
        server.send(id, b"OG-PAPER:BAD-KEY".to_vec());
        server.close(id);
        return;
    };
    let Ok(m) = Msg::decode(&plain) else { return };
    let _g = INDEX.lock().unwrap_or_else(|e| e.into_inner());
    // Only the edit form of the server link signs: it alone may change
    // the list, and sees the pages' edit keys.
    match m {
        Msg::NewPage(name) if signed => {
            make_page(dir, &name);
        }
        Msg::RenamePage(c, name) if signed => {
            let mut all = read_index(dir);
            if let Some(p) = all.iter_mut().find(|p| p.0 == c) {
                p.1 = clean_name(&name);
            }
            write_index(dir, &all);
            if let Some(p) = pages.get_mut(&c) {
                p.app.ui.file_name = clean_name(&name);
            }
        }
        Msg::DeletePage(c) if signed => {
            let mut all = read_index(dir);
            all.retain(|p| p.0 != c);
            write_index(dir, &all);
            pages.remove(&c);
            // Kept aside, not destroyed: a mistaken delete can be undone
            // by hand.
            let from = page_path(dir, c);
            let _ = std::fs::rename(&from, from.with_extension("ogp.deleted"));
        }
        Msg::ListPages => {}
        _ => return,
    }
    let list: Vec<PageInfo> = read_index(dir)
        .into_iter()
        .map(|(canvas, name)| {
            let (edit, k) = page_keys_for(canvas);
            PageInfo {
                canvas,
                name,
                edit: if signed { edit } else { String::new() },
                view: k.view_token(),
                changed: changed_ms(&page_path(dir, canvas)),
            }
        })
        .collect();
    server.send(id, keys.seal(&Msg::Pages(list).encode(), false));
}

/// One automation API request (JSON) and its reply.
fn api(
    dir: &Path,
    (edit_key, view_key): (&str, &str),
    server: &Server,
    pages: &mut HashMap<u128, Page>,
    (edit, watch): (&mut Option<bool>, &mut HashSet<u128>),
    b: &[u8],
) -> Value {
    let Ok(m) = serde_json::from_slice::<Value>(b) else {
        return json!({"ok": false, "error": "send JSON"});
    };
    let id = m.get("id").cloned().unwrap_or(Value::Null);
    let reply = |mut v: Value| {
        if let Some(o) = v.as_object_mut() {
            o.insert("id".into(), id.clone());
        }
        v
    };
    if let Some(k) = m.get("auth").and_then(Value::as_str) {
        *edit = if k == edit_key {
            Some(true)
        } else if k == view_key {
            Some(false)
        } else {
            None
        };
        return reply(match edit {
            Some(e) => json!({"ok": true, "auth": true, "can_edit": *e}),
            None => json!({"ok": false, "auth": false, "error": "that is not this server's key"}),
        });
    }
    let Some(can_edit) = *edit else {
        return reply(json!({"ok": false, "error": "sign in first: {\"auth\": \"<server key>\"}"}));
    };
    let page_id = || {
        m.get("page")
            .and_then(Value::as_str)
            .and_then(|h| u128::from_str_radix(h, 16).ok())
            .filter(|c| read_index(dir).iter().any(|(p, _)| p == c))
    };
    let need_edit = || json!({"ok": false, "error": "the view-only key cannot change things"});
    let no_page = || json!({"ok": false, "error": "no such page"});
    let cmd = m.get("cmd").and_then(Value::as_str).unwrap_or("");
    let out = match cmd {
        "pages" => {
            let list: Vec<Value> = read_index(dir)
                .into_iter()
                .map(|(c, name)| {
                    json!({"page": format!("{c:032x}"), "name": name,
                           "changed": changed_ms(&page_path(dir, c))})
                })
                .collect();
            json!({"ok": true, "pages": list})
        }
        "new_page" | "rename_page" | "delete_page" if !can_edit => need_edit(),
        "new_page" => {
            let name = m.get("name").and_then(Value::as_str).unwrap_or("API page");
            match make_page(dir, name) {
                Some(c) => json!({"ok": true, "page": format!("{c:032x}")}),
                None => json!({"ok": false, "error": "could not make the page"}),
            }
        }
        "rename_page" => match page_id() {
            Some(c) => {
                let name = clean_name(m.get("name").and_then(Value::as_str).unwrap_or(""));
                let mut all = read_index(dir);
                if let Some(p) = all.iter_mut().find(|p| p.0 == c) {
                    p.1 = name.clone();
                }
                write_index(dir, &all);
                if let Some(p) = pages.get_mut(&c) {
                    p.app.ui.file_name = name;
                }
                json!({"ok": true})
            }
            None => no_page(),
        },
        "delete_page" => match page_id() {
            Some(c) => {
                let mut all = read_index(dir);
                all.retain(|p| p.0 != c);
                write_index(dir, &all);
                pages.remove(&c);
                let from = page_path(dir, c);
                let _ = std::fs::rename(&from, from.with_extension("ogp.deleted"));
                json!({"ok": true})
            }
            None => no_page(),
        },
        "run" | "texts" => match page_id() {
            Some(c) => {
                let cmds = if cmd == "texts" {
                    json!([{"get": "texts"}])
                } else {
                    m.get("commands").cloned().unwrap_or(Value::Null)
                };
                let adds = match &cmds {
                    Value::Array(a) => a.iter().any(|c| c.get("add").is_some()),
                    v => v.get("add").is_some(),
                };
                if adds && !can_edit {
                    need_edit()
                } else {
                    let p = pages.entry(c).or_insert_with(|| {
                        let mut p = open_page(dir, c, server);
                        p.empty_since = Some(Instant::now());
                        p
                    });
                    let results = p.app.run_commands(&cmds);
                    if cmd == "texts" {
                        let texts = results
                            .get(0)
                            .and_then(|r| r.get("texts"))
                            .cloned()
                            .unwrap_or(json!([]));
                        json!({"ok": true, "texts": texts})
                    } else {
                        json!({"ok": true, "results": results})
                    }
                }
            }
            None => no_page(),
        },
        "watch" => match page_id() {
            Some(c) => {
                watch.insert(c);
                json!({"ok": true})
            }
            None => no_page(),
        },
        "unwatch" => {
            if let Some(c) = page_id() {
                watch.remove(&c);
            }
            json!({"ok": true})
        }
        other => json!({"ok": false, "error": format!("unknown cmd: {other}")}),
    };
    reply(out)
}
