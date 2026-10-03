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
//! The directory has its own key, made on first start (`DIR/server.key`,
//! or the `OGP_SERVER_KEY` environment variable) and printed as a server
//! link: its edit form can list pages with their edit keys and change the
//! list; its view form lists them with view keys only. Frames are sealed
//! like everything else live (see `seal`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use web_time::{Duration, Instant};

use crate::net::native::Server;
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
}

fn index_path(dir: &Path) -> PathBuf {
    dir.join("pages.txt")
}

fn page_path(dir: &Path, canvas: u128) -> PathBuf {
    dir.join(format!("{canvas:032x}.ogp"))
}

/// The pages and their names, in the order they were made.
fn read_index(dir: &Path) -> Vec<(u128, String)> {
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

fn write_index(dir: &Path, pages: &[(u128, String)]) {
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
fn make_page(dir: &Path, name: &str) -> Option<u128> {
    let canvas = crate::uid::new();
    let f = ogpaper_file::OgpFile::create(&page_path(dir, canvas)).ok()?;
    f.set_canvas_id(canvas).ok()?;
    let mut pages = read_index(dir);
    pages.push((canvas, clean_name(name)));
    write_index(dir, &pages);
    Some(canvas)
}

fn clean_name(n: &str) -> String {
    let n: String = n.trim().chars().take(80).collect();
    if n.is_empty() {
        "Untitled page".into()
    } else {
        n
    }
}

fn changed_ms(path: &Path) -> u64 {
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
    let mut pages: HashMap<u128, Page> = HashMap::new();
    // Each connection: Some(canvas) on a page, None in the directory.
    let mut conn_page: HashMap<u64, Option<u128>> = HashMap::new();
    loop {
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
                            conn_page.insert(id, Some(c));
                        }
                        Some(_) => server.close(id),
                        None => {
                            conn_page.insert(id, None);
                        }
                    }
                }
                Ev::Data(id, b) => match conn_page.get(&id) {
                    Some(Some(c)) => {
                        if let Some(p) = pages.get_mut(c) {
                            p.app.net_event(Ev::Data(id, b));
                        }
                    }
                    Some(None) => directory(&dir, &keys, &server, &mut pages, id, &b),
                    None => {}
                },
                Ev::Closed(id, why) => {
                    if let Some(Some(c)) = conn_page.remove(&id) {
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
        pages.retain(|c, p| {
            p.app.net_tick();
            if let Some(m) = p.app.ui.message.take() {
                println!("[{}] {m}", names.get(c).map_or("?", |n| n.as_str()));
            }
            !p.empty_since.is_some_and(|t| t.elapsed() > IDLE)
        });
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
    Page {
        app,
        conns: HashSet::new(),
        empty_since: None,
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
