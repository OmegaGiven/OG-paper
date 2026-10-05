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

use crate::hubconsole::{Console, HubOp, PageOp, Role};
use crate::net::native::{Server, WebReq, WebResp};
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
    /// In the directory (apps' Pages list), signed in to an account or
    /// not.
    Dir(Option<Role>),
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
        // The web app, served from here too (OGP_WEB_APP: the built
        // web/app folder; the Docker image has it at /srv/web/app).
        let web_app = std::env::var("OGP_WEB_APP")
            .ok()
            .or_else(|| Some("/srv/web/app".to_string()))
            .map(PathBuf::from)
            .filter(|p| p.join("index.html").is_file() && p.join("pkg").is_dir());
        if let Some(p) = &web_app {
            println!("Web app: served at /app/ (from {})", p.display());
        }
        let has_web_app = web_app.is_some();
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
                web_app: has_web_app,
            };
            let nav = console
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .home_nav(req);
            let (status, ctype, body) = crate::hubpage::page(target, base, &info, &nav);
            if status == 404 {
                if let Some(r) = web_app
                    .as_deref()
                    .and_then(|root| web_app_file(root, path, req))
                {
                    return r;
                }
            }
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
                            conns.insert(id, Conn::Dir(None));
                        }
                    }
                }
                Ev::Data(id, b) => match conns.get_mut(&id) {
                    Some(Conn::Page(c)) => {
                        if sign_in(&console, &server, *c, id, &b) {
                            continue;
                        }
                        if let Some(p) = pages.get_mut(c) {
                            p.app.net_event(Ev::Data(id, b));
                        }
                    }
                    // An app with only the address: the view key (the same
                    // the server's home page shows anyone).
                    Some(Conn::Dir(_)) if b == b"OG-PAPER:VIEW?" => {
                        server.send(id, format!("OG-PAPER:VIEW {view_token}").into_bytes());
                    }
                    Some(Conn::Dir(account)) => {
                        directory(&dir, &keys, &server, &console, account, id, &b)
                    }
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

/// A file of the web app, for `/app/...` (`/app` itself goes to `/app/`).
/// Gzipped when the browser takes it and a `.gz` copy is there (the Docker
/// image makes them: the wasm is 14 MB, 4.6 MB gzipped); browsers keep it
/// and ask again with its ETag (304 when unchanged).
fn web_app_file(root: &Path, path: &str, req: &WebReq) -> Option<WebResp> {
    if path == "/app" {
        return Some(WebResp {
            status: 303,
            ctype: "text/plain; charset=utf-8",
            headers: vec![("Location".into(), "/app/".into())],
            body: vec![],
        });
    }
    let rel = path.strip_prefix("/app/")?;
    let rel = if rel.is_empty() { "index.html" } else { rel };
    // Plain names only: no "..", no hidden files, no absolute paths.
    if rel
        .split('/')
        .any(|s| s.is_empty() || s.starts_with('.') || s.contains('\\'))
    {
        return None;
    }
    let file = root.join(rel);
    let meta = std::fs::metadata(&file).ok().filter(|m| m.is_file())?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let etag = format!("\"{:x}-{mtime:x}\"", meta.len());
    let mut headers = vec![
        ("Cache-Control".to_string(), "no-cache".to_string()),
        ("ETag".to_string(), etag.clone()),
        ("Vary".to_string(), "Accept-Encoding".to_string()),
    ];
    let ctype = match file.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json",
        "webmanifest" => "application/manifest+json",
        "css" => "text/css; charset=utf-8",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "ttf" => "font/ttf",
        "woff2" => "font/woff2",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    };
    if req
        .header("if-none-match")
        .is_some_and(|t| t.split(',').any(|t| t.trim() == etag))
    {
        return Some(WebResp {
            status: 304,
            ctype,
            headers,
            body: vec![],
        });
    }
    let gzip = req
        .header("accept-encoding")
        .is_some_and(|a| a.split(',').any(|e| e.trim().starts_with("gzip")));
    let gz = PathBuf::from(format!("{}.gz", file.display()));
    let body = match (gzip, std::fs::read(&gz)) {
        (true, Ok(b)) => {
            headers.push(("Content-Encoding".to_string(), "gzip".to_string()));
            b
        }
        _ => std::fs::read(&file).ok()?,
    };
    Some(WebResp {
        status: 200,
        ctype,
        headers,
        body,
    })
}

/// A guest on a page signing in with a console account: answer with the
/// page's edit key for any role but viewer. True when `b` was that.
fn sign_in(console: &Mutex<Console>, server: &Server, canvas: u128, id: u64, b: &[u8]) -> bool {
    // Sign-ins are small; don't open every big change twice.
    if b.len() > 4096 {
        return false;
    }
    let (edit, keys) = page_keys_for(canvas);
    let Some((plain, _)) = keys.open(b) else {
        return false;
    };
    let Ok(Msg::SignIn { user, password }) = Msg::decode(&plain) else {
        return false;
    };
    let checked = console
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .check(&user, &password);
    let reply = match checked {
        Ok(role) if role.can_draw() => {
            println!("[{canvas:032x}] {user} signed in to draw");
            Msg::SignedIn {
                edit,
                note: format!("Signed in as {user}"),
            }
        }
        Ok(_) => Msg::SignedIn {
            edit: String::new(),
            note: "That account can view pages, not draw".into(),
        },
        Err(e) => Msg::SignedIn {
            edit: String::new(),
            note: e.into(),
        },
    };
    server.send(id, keys.seal(&reply.encode(), false));
    true
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
#[allow(clippy::too_many_arguments)]
fn directory(
    dir: &Path,
    keys: &Keys,
    server: &Server,
    console: &Mutex<Console>,
    account: &mut Option<Role>,
    id: u64,
    b: &[u8],
) {
    let Some((plain, signed)) = keys.open(b) else {
        server.send(id, b"OG-PAPER:BAD-KEY".to_vec());
        server.close(id);
        return;
    };
    let Ok(m) = Msg::decode(&plain) else { return };
    if let Msg::AccountIn {
        user,
        password,
        token,
    } = m
    {
        let r = console
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .account(&user, &password, &token);
        let reply = match r {
            Ok((role, token)) => {
                *account = Some(role);
                Msg::Account {
                    note: format!("Signed in as {user}"),
                    user,
                    role: role.key().into(),
                    token,
                }
            }
            Err(e) => {
                *account = None;
                Msg::Account {
                    user,
                    role: String::new(),
                    token: String::new(),
                    note: e.into(),
                }
            }
        };
        server.send(id, keys.seal(&reply.encode(), false));
        return;
    }
    let _g = INDEX.lock().unwrap_or_else(|e| e.into_inner());
    // The edit form of the server link (it signs) may do anything; an
    // account, what its role allows. Viewers and the plain view link see
    // only the pages' view keys.
    let role = if signed { Some(Role::Admin) } else { *account };
    let draws = role.is_some_and(Role::can_draw);
    let op = match m {
        Msg::NewPage(name) => Some(PageOp::New {
            name,
            folder: String::new(),
        }),
        Msg::RenamePage(c, name) => Some(PageOp::Rename(c, name)),
        Msg::DeletePage(c) => Some(PageOp::Delete(c)),
        Msg::Change {
            op,
            page,
            name,
            folder,
        } => match op.as_str() {
            "new_page" => Some(PageOp::New { name, folder }),
            "rename" => Some(PageOp::Rename(page, name)),
            "move" => Some(PageOp::Move(page, folder)),
            "delete" => Some(PageOp::Delete(page)),
            "new_folder" => Some(PageOp::NewFolder {
                parent: folder,
                name,
            }),
            "rename_folder" => Some(PageOp::RenameFolder { folder, name }),
            "delete_folder" => Some(PageOp::DeleteFolder(folder)),
            _ => None,
        },
        // An app joining the server link as if it were a page (Share live ›
        // Join): the page list tells it this is a workspace.
        Msg::ListPages | Msg::Hello { .. } => None,
        _ => return,
    };
    let mut c = console.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(op) = op {
        let done = match role {
            Some(r) => c.page_op(r, op),
            None => Err("Sign in to change this workspace.".to_string()),
        };
        let reply = match done {
            Ok((note, made)) => Msg::Changed {
                note,
                ok: true,
                made: made.unwrap_or(0),
            },
            Err(note) => Msg::Changed {
                note,
                ok: false,
                made: 0,
            },
        };
        server.send(id, keys.seal(&reply.encode(), false));
    }
    let (folders, in_folder) = c.folder_list();
    drop(c);
    let folders = Msg::Folders {
        folders,
        pages: in_folder,
    };
    server.send(id, keys.seal(&folders.encode(), false));
    let list: Vec<PageInfo> = read_index(dir)
        .into_iter()
        .map(|(canvas, name)| {
            let (edit, k) = page_keys_for(canvas);
            PageInfo {
                canvas,
                name,
                edit: if draws { edit } else { String::new() },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::native::Client;

    /// Send `m` on page `canvas` with `key` and return the server's answer.
    fn ask(
        server: &Server,
        console: &Mutex<Console>,
        canvas: u128,
        key: &crate::seal::Keys,
        m: Msg,
    ) -> Msg {
        let client = Client::connect(format!("ws://127.0.0.1:{}/p/{canvas:032x}", server.port));
        let start = Instant::now();
        let mut sent = false;
        while start.elapsed() < Duration::from_secs(10) {
            for ev in client.poll() {
                if let Ev::Data(_, b) = ev {
                    let (plain, _) = key.open(&b).expect("sealed answer");
                    return Msg::decode(&plain).expect("answer");
                }
            }
            for ev in server.poll() {
                match ev {
                    Ev::Open(_) if !sent => {
                        client.send(key.seal(&m.encode(), false));
                        sent = true;
                    }
                    Ev::Data(id, b) => {
                        assert!(sign_in(console, server, canvas, id, &b), "a sign-in");
                    }
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("no answer");
    }

    #[test]
    fn page_sign_in_gives_drawing_rights_but_not_to_viewers() {
        let dir = std::env::temp_dir().join(format!("ogp-signin-{}", crate::uid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let console = Mutex::new(Console::load(&dir));
        console
            .lock()
            .unwrap()
            .add_user("val", Role::Viewer, "viewpass1");
        let canvas = make_page(&dir, "Wall").unwrap();
        let (edit, keys) = page_keys_for(canvas);
        let view = crate::seal::Keys::parse(&keys.view_token()).unwrap();
        let server = Server::start(0).unwrap();
        let sign = |user: &str, password: &str| {
            ask(
                &server,
                &console,
                canvas,
                &view,
                Msg::SignIn {
                    user: user.into(),
                    password: password.into(),
                },
            )
        };
        // The default admin gets the page's edit key, through a view link.
        assert!(matches!(sign("admin", "password"), Msg::SignedIn { edit: e, .. } if e == edit));
        // A viewer and a wrong password get none.
        assert!(matches!(sign("val", "viewpass1"), Msg::SignedIn { edit: e, .. } if e.is_empty()));
        assert!(matches!(sign("admin", "nope"), Msg::SignedIn { edit: e, .. } if e.is_empty()));
        server.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Send `msgs` to the directory with `key`; the answers, in order
    /// (folder lists left out), once each request has its page list.
    fn dir_talk(
        server: &Server,
        console: &Mutex<Console>,
        dir: &Path,
        key: &Keys,
        msgs: &[Msg],
    ) -> Vec<Msg> {
        let client = Client::connect(format!("ws://127.0.0.1:{}/", server.port));
        let mut account = None;
        let mut out = Vec::new();
        let want = msgs
            .iter()
            .filter(|m| !matches!(m, Msg::AccountIn { .. }))
            .count();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10)
            && out.iter().filter(|m| matches!(m, Msg::Pages(_))).count() < want
        {
            for ev in client.poll() {
                if let Ev::Data(_, b) = ev {
                    let (plain, _) = key.open(&b).expect("sealed answer");
                    let m = Msg::decode(&plain).expect("answer");
                    if !matches!(m, Msg::Folders { .. }) {
                        out.push(m);
                    }
                }
            }
            for ev in server.poll() {
                match ev {
                    Ev::Open(_) => {
                        for m in msgs {
                            client.send(key.seal(&m.encode(), true));
                        }
                    }
                    Ev::Data(id, b) => directory(dir, key, server, console, &mut account, id, &b),
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        out
    }

    #[test]
    fn directory_accounts_decide_what_the_page_list_gives() {
        let dir = std::env::temp_dir().join(format!("ogp-dir-{}", crate::uid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let console = Mutex::new(Console::load(&dir));
        console
            .lock()
            .unwrap()
            .add_user("val", Role::Viewer, "viewpass1");
        make_page(&dir, "Wall").unwrap();
        let server_keys = Keys::parse(&Keys::edit_token(&crate::seal::random_secret())).unwrap();
        let view = Keys::parse(&server_keys.view_token()).unwrap();
        let server = Server::start(0).unwrap();
        let login = |user: &str, password: &str, token: &str| Msg::AccountIn {
            user: user.into(),
            password: password.into(),
            token: token.into(),
        };
        let edit_keys = |m: &Msg| match m {
            Msg::Pages(l) => l.iter().all(|p| !p.edit.is_empty()),
            _ => panic!("not pages: {m:?}"),
        };
        // The view link alone: view keys only.
        let r = dir_talk(&server, &console, &dir, &view, &[Msg::ListPages]);
        assert!(!edit_keys(&r[0]));
        // Admin by password: a token, and the edit keys.
        let r = dir_talk(
            &server,
            &console,
            &dir,
            &view,
            &[login("admin", "password", ""), Msg::ListPages],
        );
        let Msg::Account { role, token, .. } = &r[0] else {
            panic!()
        };
        assert_eq!(role, "admin");
        assert!(!token.is_empty() && edit_keys(&r[1]));
        // The token signs in again; admins may make pages.
        let r = dir_talk(
            &server,
            &console,
            &dir,
            &view,
            &[login("admin", "", token), Msg::NewPage("Two".into())],
        );
        assert!(matches!(&r[0], Msg::Account { role, .. } if role == "admin"));
        assert!(matches!(&r[1], Msg::Changed { ok: true, made, .. } if *made != 0));
        assert!(matches!(&r[2], Msg::Pages(l) if l.len() == 2));
        // Folders: admin makes one and a page in it; a subadmin moves it.
        let change = |op: &str, page: u128, name: &str, folder: &str| Msg::Change {
            op: op.into(),
            page,
            name: name.into(),
            folder: folder.into(),
        };
        let r = dir_talk(
            &server,
            &console,
            &dir,
            &view,
            &[
                login("admin", "", token),
                change("new_folder", 0, "Art", ""),
                change("new_page", 0, "Mural", "Art"),
            ],
        );
        let Some(Msg::Changed { made, .. }) =
            r.iter().rev().find(|m| matches!(m, Msg::Changed { .. }))
        else {
            panic!()
        };
        let mural = *made;
        assert_eq!(
            console.lock().unwrap().folder_list().1,
            vec![(mural, "Art".to_string())]
        );
        console
            .lock()
            .unwrap()
            .add_user("sam", Role::Subadmin, "subpass12");
        dir_talk(
            &server,
            &console,
            &dir,
            &view,
            &[login("sam", "subpass12", ""), change("move", mural, "", "")],
        );
        assert!(console.lock().unwrap().folder_list().1.is_empty());
        // A viewer: view keys, and can't make pages.
        let r = dir_talk(
            &server,
            &console,
            &dir,
            &view,
            &[login("val", "viewpass1", ""), Msg::NewPage("Nope".into())],
        );
        assert!(matches!(&r[0], Msg::Account { role, .. } if role == "viewer"));
        assert!(matches!(&r[1], Msg::Changed { ok: false, .. }));
        assert!(matches!(&r[2], Msg::Pages(l) if l.len() == 3) && !edit_keys(&r[2]));
        // A wrong password or a made-up token: refused.
        let r = dir_talk(
            &server,
            &console,
            &dir,
            &view,
            &[login("admin", "nope", ""), Msg::ListPages],
        );
        assert!(
            matches!(&r[0], Msg::Account { token, .. } if token.is_empty()) && !edit_keys(&r[1])
        );
        let r = dir_talk(
            &server,
            &console,
            &dir,
            &view,
            &[login("admin", "", "abc"), Msg::ListPages],
        );
        assert!(matches!(&r[0], Msg::Account { token, .. } if token.is_empty()));
        server.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_web_app_is_served_from_its_folder_and_nothing_else() {
        let root = std::env::temp_dir().join(format!("ogp-webapp-{}", crate::uid::new()));
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        std::fs::write(root.join("index.html"), "<!doctype html>app").unwrap();
        std::fs::write(root.join("pkg/a.wasm"), [0u8, 97, 115, 109]).unwrap();
        std::fs::write(root.parent().unwrap().join("secret.txt"), "no").unwrap();
        let req = |headers: Vec<(&str, &str)>| WebReq {
            method: "GET".into(),
            target: String::new(),
            base: String::new(),
            headers: headers
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: vec![],
        };
        let plain = req(vec![]);
        assert_eq!(web_app_file(&root, "/app", &plain).unwrap().status, 303);
        let index = web_app_file(&root, "/app/", &plain).unwrap();
        assert!(index.ctype.starts_with("text/html") && index.body.ends_with(b"app"));
        assert_eq!(
            web_app_file(&root, "/app/pkg/a.wasm", &plain)
                .unwrap()
                .ctype,
            "application/wasm"
        );
        for bad in [
            "/app/../secret.txt",
            "/app/pkg/../../secret.txt",
            "/app/.hidden",
            "/app//etc/passwd",
            "/other/index.html",
        ] {
            assert!(web_app_file(&root, bad, &plain).is_none(), "{bad}");
        }
        // Gzipped for browsers that take it; 304 for one that has it.
        std::fs::write(root.join("pkg/a.wasm.gz"), b"GZ").unwrap();
        let gz = web_app_file(
            &root,
            "/app/pkg/a.wasm",
            &req(vec![("accept-encoding", "gzip, br")]),
        )
        .unwrap();
        assert!(
            gz.body == b"GZ"
                && gz
                    .headers
                    .iter()
                    .any(|(k, v)| k == "Content-Encoding" && v == "gzip")
        );
        let etag = gz
            .headers
            .iter()
            .find(|(k, _)| k == "ETag")
            .unwrap()
            .1
            .clone();
        let again = web_app_file(
            &root,
            "/app/pkg/a.wasm",
            &req(vec![("if-none-match", &etag)]),
        )
        .unwrap();
        assert!(again.status == 304 && again.body.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
