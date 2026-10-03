// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Sharing a canvas live: one app hosts it (the master copy), others join
//! with a link and keep their own copy, saved on their device.
//!
//! The protocol is the merge of `share` run continuously. On connecting, a
//! guest says what it has (its version); the host answers with the changes
//! the guest lacks, the guest sends back the changes the host lacks (all it
//! did offline), and from then on each side sends its new changes as they
//! happen. The host merges what a guest sends into the master and passes
//! it on to the other guests. Losing the connection loses nothing: the
//! guest keeps drawing on its copy and catches up when it reconnects.
//!
//! The host runs a WebSocket server (desktop, or headless with
//! `og-paper --serve`). Guests connect from the desktop app or a browser
//! (whose page does the WebSocket itself, see `web`). Links carry a key:
//! the edit key lets a guest change the canvas, the view key only follow
//! it.

#![cfg_attr(
    target_arch = "wasm32",
    allow(unused_imports, unused_variables, dead_code)
)]

use std::collections::HashMap;

use ogpaper_core::sync::{Policy, Version};
use web_time::{Duration, Instant};

use crate::wire::Msg;
use crate::App;

/// The port a host listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 8991;
/// How long a guest waits before trying to reconnect.
const RETRY: Duration = Duration::from_secs(3);

/// A transport event (connection `id`; a guest's one connection is 0).
#[derive(Clone, Debug)]
pub enum Ev {
    Open(u64),
    Data(u64, Vec<u8>),
    Closed(u64, String),
}

pub struct Net {
    pub role: Role,
    /// The log size when changes were last pushed.
    pushed: usize,
}

pub enum Role {
    #[cfg(not(target_arch = "wasm32"))]
    Host(Host),
    Guest(Guest),
}

#[cfg(not(target_arch = "wasm32"))]
pub struct Host {
    pub port: u16,
    pub edit_key: String,
    pub view_key: String,
    pub conns: HashMap<u64, Conn>,
    server: native::Server,
}

pub struct Conn {
    pub peer: u64,
    pub name: String,
    /// What it has; None until it said hello.
    their: Option<Version>,
    pub edit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuestState {
    Connecting,
    Live,
    /// Disconnected; trying again at this time.
    Offline(Instant),
    /// Refused (bad key); not retrying.
    Refused,
}

pub struct Guest {
    pub url: String,
    pub key: String,
    pub state: GuestState,
    /// What the host has (as far as we know); None until welcomed.
    their: Option<Version>,
    pub edit: bool,
    pub host: u64,
    pub canvas_name: String,
    #[cfg(not(target_arch = "wasm32"))]
    client: Option<native::Client>,
}

/// A link split into the WebSocket address and its key. Takes
/// `ws(s)://host:port/?k=KEY`, or a web app address with
/// `#join=ws(s)://host:port&k=KEY`.
pub fn parse_link(link: &str) -> Option<(String, String)> {
    let link = link.trim();
    let (url, key) = if let Some(i) = link.find("#join=") {
        let rest = &link[i + 6..];
        match rest.split_once("&k=") {
            Some((u, k)) => (u.to_string(), k.to_string()),
            None => (rest.to_string(), String::new()),
        }
    } else {
        match link.split_once("?k=") {
            Some((u, k)) => (u.to_string(), k.to_string()),
            None => (link.to_string(), String::new()),
        }
    };
    let url = url.trim_end_matches('/').to_string();
    (url.starts_with("ws://") || url.starts_with("wss://")).then_some((url, key))
}

/// A random key for links.
pub fn new_key() -> String {
    // The random low halves of two ids (their top bits are a timestamp).
    format!(
        "{:016x}{:016x}",
        crate::uid::new() as u64,
        crate::uid::new() as u64
    )
}

/// This machine's address on its network, as others would reach it.
#[cfg(not(target_arch = "wasm32"))]
pub fn local_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "localhost".into())
}

impl Net {
    pub fn is_host(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(self.role, Role::Host(_)) {
            return true;
        }
        false
    }
}

impl App {
    /// Start hosting this canvas on `port` (keys kept per canvas, so links
    /// stay good across restarts).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn host_start(&mut self, port: u16) {
        self.net_stop();
        let server = match native::Server::start(port) {
            Ok(s) => s,
            Err(e) => {
                self.say(format!("Could not host on port {port}: {e}"));
                return;
            }
        };
        let mut prefs = crate::prefs::load();
        let k = format!("host.{:032x}", self.share.canvas);
        let keys = prefs.get(&k).cloned().unwrap_or_default();
        let (edit_key, view_key) = match keys.split_once(',') {
            Some((e, v)) if e.len() == 32 && v.len() == 32 => (e.to_string(), v.to_string()),
            _ => {
                let pair = (new_key(), new_key());
                prefs.insert(k, format!("{},{}", pair.0, pair.1));
                crate::prefs::save(&prefs);
                pair
            }
        };
        let port = server.port;
        // The host's policy holds on every copy while it hosts.
        let code = crate::prefs::load()
            .get(&format!("policy.{:032x}", self.share.canvas))
            .and_then(|v| v.parse::<u8>().ok())
            .unwrap_or(0);
        self.share.log.policy = Policy::from_code(code, self.share.clock.peer());
        self.net = Some(Net {
            role: Role::Host(Host {
                port,
                edit_key,
                view_key,
                conns: HashMap::new(),
                server,
            }),
            pushed: 0,
        });
        self.ensure_file();
        self.say(format!(
            "Hosting on port {port}: share a link from Share live"
        ));
    }

    /// Set the host's conflict policy (0 newest, 1 host wins, 2 guests win).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_policy(&mut self, code: u8) {
        let mut p = crate::prefs::load();
        p.insert(
            format!("policy.{:032x}", self.share.canvas),
            code.to_string(),
        );
        crate::prefs::save(&p);
        self.share.log.policy = Policy::from_code(code, self.share.clock.peer());
        self.reapply_visibility();
        self.say(match code {
            1 => "Rival edits: the host's wins",
            2 => "Rival edits: the guest's wins",
            _ => "Rival edits: the newest wins",
        });
        // Guests learn it when they next connect.
    }

    /// Join a shared canvas by its link.
    pub(crate) fn join(&mut self, link: &str) {
        let Some((url, key)) = parse_link(link) else {
            self.say("That is not a share link (it starts with ws:// or wss://)");
            return;
        };
        self.net_stop();
        self.net = Some(Net {
            role: Role::Guest(Guest {
                url,
                key,
                state: GuestState::Connecting,
                their: None,
                edit: true,
                host: 0,
                canvas_name: String::new(),
                #[cfg(not(target_arch = "wasm32"))]
                client: None,
            }),
            pushed: 0,
        });
        self.guest_connect();
    }

    fn guest_connect(&mut self) {
        let Some(Net {
            role: Role::Guest(g),
            ..
        }) = self.net.as_mut()
        else {
            return;
        };
        g.state = GuestState::Connecting;
        g.their = None;
        #[cfg(not(target_arch = "wasm32"))]
        {
            g.client = Some(native::Client::connect(g.url.clone()));
        }
        #[cfg(target_arch = "wasm32")]
        crate::web::net_connect(&g.url);
    }

    /// Stop hosting or leave.
    pub(crate) fn net_stop(&mut self) {
        let Some(n) = self.net.take() else { return };
        match n.role {
            #[cfg(not(target_arch = "wasm32"))]
            Role::Host(h) => h.server.stop(),
            Role::Guest(_g) => {
                #[cfg(target_arch = "wasm32")]
                crate::web::net_close();
            }
        }
        self.view_only = false;
        self.peers.clear();
    }

    fn send(&mut self, conn: u64, m: &Msg) {
        let b = m.encode();
        let Some(n) = self.net.as_mut() else { return };
        match &mut n.role {
            #[cfg(not(target_arch = "wasm32"))]
            Role::Host(h) => h.server.send(conn, b),
            Role::Guest(_g) => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = &_g.client {
                    c.send(b);
                }
                #[cfg(target_arch = "wasm32")]
                crate::web::net_send(b);
            }
        }
    }

    /// Send to every guest (host) or the host (guest), except `but`.
    pub(crate) fn broadcast(&mut self, m: &Msg, but: Option<u64>) {
        let targets: Vec<u64> = match self.net.as_ref().map(|n| &n.role) {
            #[cfg(not(target_arch = "wasm32"))]
            Some(Role::Host(h)) => h
                .conns
                .iter()
                .filter(|(id, c)| Some(**id) != but && c.their.is_some())
                .map(|(id, _)| *id)
                .collect(),
            Some(Role::Guest(g)) if g.state == GuestState::Live => vec![0],
            _ => vec![],
        };
        for t in targets {
            self.send(t, m);
        }
    }

    /// Run the connection: take what came in, send what is new. Called
    /// every frame (and on a timer while connected).
    pub(crate) fn net_tick(&mut self) {
        let Some(n) = self.net.as_mut() else { return };
        let evs: Vec<Ev> = match &mut n.role {
            #[cfg(not(target_arch = "wasm32"))]
            Role::Host(h) => h.server.poll(),
            Role::Guest(_g) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    _g.client.as_ref().map(|c| c.poll()).unwrap_or_default()
                }
                #[cfg(target_arch = "wasm32")]
                {
                    Vec::new()
                }
            }
        };
        for e in evs {
            self.net_event(e);
        }
        // A guest that lost the host tries again.
        let retry = matches!(
            self.net.as_ref().map(|n| &n.role),
            Some(Role::Guest(g)) if matches!(g.state, GuestState::Offline(t) if Instant::now() >= t)
        );
        if retry {
            self.guest_connect();
        }
        self.push_changes();
        self.presence_tick();
    }

    /// Send each side the changes it lacks.
    fn push_changes(&mut self) {
        let Some(n) = self.net.as_ref() else { return };
        let len = self.share.log.len();
        let targets: Vec<(u64, Version)> = match &n.role {
            #[cfg(not(target_arch = "wasm32"))]
            Role::Host(h) => h
                .conns
                .iter()
                .filter_map(|(id, c)| c.their.clone().map(|v| (*id, v)))
                .collect(),
            Role::Guest(g) if g.state == GuestState::Live && g.edit => {
                g.their.clone().map(|v| vec![(0, v)]).unwrap_or_default()
            }
            _ => vec![],
        };
        if len == n.pushed && targets.iter().all(|(_, v)| v == self.share.log.version()) {
            return;
        }
        let ours = self.share.log.version().clone();
        for (id, v) in targets {
            let (bytes, count) = self.changes_since(&v);
            if count > 0 {
                self.send(id, &Msg::Changes(bytes));
            }
            self.set_their(id, &ours);
        }
        if let Some(n) = self.net.as_mut() {
            n.pushed = len;
        }
    }

    /// Note that connection `id` now has (at least) `v`.
    fn set_their(&mut self, id: u64, v: &Version) {
        let Some(n) = self.net.as_mut() else { return };
        match &mut n.role {
            #[cfg(not(target_arch = "wasm32"))]
            Role::Host(h) => {
                if let Some(t) = h.conns.get_mut(&id).and_then(|c| c.their.as_mut()) {
                    t.join(v);
                }
            }
            Role::Guest(g) => {
                if let Some(t) = g.their.as_mut() {
                    t.join(v);
                }
            }
        }
    }

    pub(crate) fn net_event(&mut self, e: Ev) {
        match e {
            Ev::Open(id) => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(Net {
                    role: Role::Host(h),
                    ..
                }) = self.net.as_mut()
                {
                    h.conns.insert(
                        id,
                        Conn {
                            peer: 0,
                            name: String::new(),
                            their: None,
                            edit: false,
                        },
                    );
                    return;
                }
                // A guest's connection opened: say hello.
                let _ = id;
                let hello = Msg::Hello {
                    canvas: self.share.canvas,
                    peer: self.share.clock.peer(),
                    version: self.share.log.version().clone(),
                    key: match self.net.as_ref().map(|n| &n.role) {
                        Some(Role::Guest(g)) => g.key.clone(),
                        _ => String::new(),
                    },
                    name: crate::presence::my_name(),
                };
                self.send(0, &hello);
            }
            Ev::Data(id, b) => match Msg::decode(&b) {
                Ok(m) => {
                    if self.net.as_ref().is_some_and(|n| n.is_host()) {
                        self.host_msg(id, m);
                    } else {
                        self.guest_msg(m);
                    }
                }
                Err(e) => log::warn!("bad message: {e}"),
            },
            Ev::Closed(id, why) => {
                let host = self.net.as_ref().is_some_and(|n| n.is_host());
                if host {
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(Net {
                        role: Role::Host(h),
                        ..
                    }) = self.net.as_mut()
                    {
                        if let Some(c) = h.conns.remove(&id) {
                            if c.their.is_some() {
                                self.peers.remove(&c.peer);
                                self.broadcast(&Msg::Gone(c.peer), None);
                                self.say(format!("{} left", display_name(&c.name)));
                            }
                        }
                    }
                } else if let Some(Net {
                    role: Role::Guest(g),
                    ..
                }) = self.net.as_mut()
                {
                    if g.state != GuestState::Refused {
                        let was_live = g.state == GuestState::Live;
                        g.state = GuestState::Offline(Instant::now() + RETRY);
                        self.peers.clear();
                        if was_live {
                            self.say(format!(
                                "Lost the connection ({why}): keep drawing, it syncs when back"
                            ));
                        }
                    }
                }
            }
        }
    }

    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    fn host_msg(&mut self, id: u64, m: Msg) {
        #[cfg(not(target_arch = "wasm32"))]
        match m {
            Msg::Hello {
                peer,
                version,
                key,
                name,
                ..
            } => {
                let Some(Net {
                    role: Role::Host(h),
                    ..
                }) = self.net.as_mut()
                else {
                    return;
                };
                let edit = key == h.edit_key;
                if !edit && key != h.view_key {
                    h.server.send(
                        id,
                        Msg::Error("That link's key is not valid for this canvas".into()).encode(),
                    );
                    h.server.close(id);
                    return;
                }
                if let Some(c) = h.conns.get_mut(&id) {
                    c.peer = peer;
                    c.name = name.clone();
                    c.edit = edit;
                    c.their = Some(version);
                }
                let welcome = Msg::Welcome {
                    canvas: self.share.canvas,
                    host: self.share.clock.peer(),
                    version: self.share.log.version().clone(),
                    policy: self.share.log.policy.code(),
                    edit,
                    name: self.ui.file_name.clone(),
                };
                self.send(id, &welcome);
                self.say(format!(
                    "{} joined{}",
                    display_name(&name),
                    if edit { "" } else { " (view only)" }
                ));
                // Who else is here.
                let others: Vec<Msg> = self.peers.values().map(|p| p.presence_msg()).collect();
                for m in others {
                    self.send(id, &m);
                }
                if let Some(m) = self.my_presence() {
                    self.send(id, &m);
                }
                // push_changes sends what it lacks.
                if let Some(n) = self.net.as_mut() {
                    n.pushed = usize::MAX;
                }
            }
            Msg::Changes(b) => {
                let can = matches!(
                    self.net.as_ref().map(|n| &n.role),
                    Some(Role::Host(h)) if h.conns.get(&id).is_some_and(|c| c.edit && c.their.is_some())
                );
                if !can {
                    return;
                }
                if let Ok(s) = crate::snapshot::decode(&b, crate::BASE_PX) {
                    if let Some(v) = self.merge_quiet(s.scene, s.objs, s.share) {
                        self.set_their(id, &v);
                    }
                }
            }
            m @ (Msg::Presence { .. } | Msg::Wet { .. }) => {
                self.peer_msg(&m);
                self.broadcast(&m, Some(id));
            }
            _ => {}
        }
    }

    fn guest_msg(&mut self, m: Msg) {
        match m {
            Msg::Welcome {
                canvas,
                host,
                version,
                policy,
                edit,
                name,
            } => {
                if canvas != self.share.canvas {
                    self.switch_to_shared(canvas, &name);
                }
                self.share.log.policy = Policy::from_code(policy, host);
                let mut p = crate::prefs::load();
                p.insert(format!("policy.{canvas:032x}"), policy.to_string());
                crate::prefs::save(&p);
                self.view_only = !edit;
                if let Some(Net {
                    role: Role::Guest(g),
                    pushed,
                }) = self.net.as_mut()
                {
                    let first = g.host == 0;
                    g.state = GuestState::Live;
                    g.their = Some(version);
                    g.edit = edit;
                    g.host = host;
                    g.canvas_name = name.clone();
                    *pushed = usize::MAX;
                    if first {
                        self.say(format!(
                            "Joined {}{}",
                            display_name(&name),
                            if edit { "" } else { " (view only)" }
                        ));
                    } else {
                        self.say("Back online: synced");
                    }
                }
                self.reapply_visibility();
                if let Some(m) = self.my_presence() {
                    self.send(0, &m);
                }
            }
            Msg::Changes(b) => {
                if let Ok(s) = crate::snapshot::decode(&b, crate::BASE_PX) {
                    if let Some(v) = self.merge_quiet(s.scene, s.objs, s.share) {
                        self.set_their(0, &v);
                    }
                }
            }
            Msg::Error(t) => {
                if let Some(Net {
                    role: Role::Guest(g),
                    ..
                }) = self.net.as_mut()
                {
                    g.state = GuestState::Refused;
                }
                self.say(t);
            }
            Msg::Gone(peer) => {
                self.peers.remove(&peer);
                self.redraw();
            }
            m @ (Msg::Presence { .. } | Msg::Wet { .. }) => self.peer_msg(&m),
            Msg::Hello { .. } => {}
        }
    }

    /// Joining a canvas this device does not have yet: start a fresh copy
    /// of it here (the host sends its contents next).
    fn switch_to_shared(&mut self, canvas: u128, name: &str) {
        let keep = self.net.take();
        self.load_scene(ogpaper_core::Scene::new(), crate::home_camera());
        self.net = keep;
        self.share.canvas = canvas;
        self.ui.file_name = if name.is_empty() {
            "Shared canvas".into()
        } else {
            format!("{name} (shared)")
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.file = None;
            self.ensure_file();
        }
        self.persist_share();
    }

    /// After the policy changed: show and hide strokes as the log now says.
    pub(crate) fn reapply_visibility(&mut self) {
        let visible = self.share.log.visible();
        let known = self.share.log.state();
        for id in 0..self.scene.strokes.len() as u32 {
            let s = &self.scene.strokes[id as usize];
            if !known.contains_key(&s.uid) {
                continue;
            }
            let want = visible.contains(&s.uid);
            if want == !s.deleted {
                continue;
            }
            if want {
                self.scene.restore(id);
            } else {
                self.scene.delete(id);
            }
            self.timeline.record(id, want);
            self.persist_deleted(id);
        }
        if let Some(g) = self.gpu.as_mut() {
            g.sync(&self.scene);
        }
        self.redraw();
    }

    /// What the Share live panel shows.
    pub(crate) fn live_info(&self) -> Option<crate::ui::LiveInfo> {
        let n = self.net.as_ref()?;
        Some(match &n.role {
            #[cfg(not(target_arch = "wasm32"))]
            Role::Host(h) => {
                let ip = local_ip();
                let ws = format!("ws://{ip}:{}", h.port);
                crate::ui::LiveInfo {
                    hosting: true,
                    state: format!("Hosting on port {}", h.port),
                    people: h
                        .conns
                        .values()
                        .filter(|c| c.their.is_some())
                        .map(|c| {
                            (
                                c.peer,
                                format!(
                                    "{}{}",
                                    display_name(&c.name),
                                    if c.edit { "" } else { " (view)" }
                                ),
                            )
                        })
                        .collect(),
                    links: vec![
                        ("Edit link (apps)".into(), format!("{ws}/?k={}", h.edit_key)),
                        ("View link (apps)".into(), format!("{ws}/?k={}", h.view_key)),
                        (
                            "Edit link (browser)".into(),
                            format!("{}#join={ws}&k={}", WEB_APP, h.edit_key),
                        ),
                    ],
                    policy: self.share.log.policy.code(),
                    view_only: false,
                }
            }
            Role::Guest(g) => crate::ui::LiveInfo {
                hosting: false,
                state: match g.state {
                    GuestState::Connecting => "Connecting…".into(),
                    GuestState::Live => format!(
                        "Live with {}{}",
                        display_name(&g.canvas_name),
                        if g.edit { "" } else { " (view only)" }
                    ),
                    GuestState::Offline(_) => "Offline: your changes sync when back".into(),
                    GuestState::Refused => "Refused: ask for a new link".into(),
                },
                people: self
                    .peers
                    .values()
                    .map(|p| (p.peer, display_name(&p.name)))
                    .collect(),
                links: vec![],
                policy: self.share.log.policy.code(),
                view_only: self.view_only,
            },
        })
    }

    /// Whether a connection needs frequent wake-ups.
    pub(crate) fn net_active(&self) -> bool {
        self.net.is_some()
    }
}

/// The public web app (a host's browser link points at it).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const WEB_APP: &str = "https://omegagiven.github.io/OG-paper/app/";

pub fn display_name(n: &str) -> String {
    if n.trim().is_empty() {
        "Someone".into()
    } else {
        n.trim().to_string()
    }
}

/// Native transports: a WebSocket server (host) and client (guest), each
/// connection on its own thread, talking to the app through channels.
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::HashMap;
    use std::io::ErrorKind;
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tungstenite::{Message, WebSocket};

    use super::Ev;

    enum Out {
        Data(Vec<u8>),
        Close,
    }

    type Outs = Arc<Mutex<HashMap<u64, Sender<Out>>>>;

    pub struct Server {
        pub port: u16,
        rx: Receiver<Ev>,
        outs: Outs,
        stop: Arc<AtomicBool>,
    }

    impl Server {
        pub fn start(port: u16) -> std::io::Result<Server> {
            let l = TcpListener::bind(("0.0.0.0", port))?;
            let port = l.local_addr()?.port();
            l.set_nonblocking(true)?;
            let (tx, rx) = channel();
            let outs: Outs = Arc::default();
            let stop = Arc::new(AtomicBool::new(false));
            let (o, s) = (outs.clone(), stop.clone());
            std::thread::spawn(move || {
                let next = AtomicU64::new(1);
                while !s.load(Ordering::Relaxed) {
                    match l.accept() {
                        Ok((stream, _)) => {
                            let id = next.fetch_add(1, Ordering::Relaxed);
                            let (otx, orx) = channel();
                            o.lock().expect("outs").insert(id, otx);
                            let (tx, o, s) = (tx.clone(), o.clone(), s.clone());
                            std::thread::spawn(move || {
                                let why = match accept(stream) {
                                    Ok(ws) => {
                                        let _ = tx.send(Ev::Open(id));
                                        pump(ws, id, &tx, &orx, &s)
                                    }
                                    Err(e) => e,
                                };
                                o.lock().expect("outs").remove(&id);
                                let _ = tx.send(Ev::Closed(id, why));
                            });
                        }
                        Err(e) if e.kind() == ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(40))
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(200)),
                    }
                }
            });
            Ok(Server {
                port,
                rx,
                outs,
                stop,
            })
        }

        pub fn send(&self, id: u64, b: Vec<u8>) {
            if let Some(s) = self.outs.lock().expect("outs").get(&id) {
                let _ = s.send(Out::Data(b));
            }
        }

        pub fn close(&self, id: u64) {
            if let Some(s) = self.outs.lock().expect("outs").get(&id) {
                let _ = s.send(Out::Close);
            }
        }

        pub fn poll(&self) -> Vec<Ev> {
            self.rx.try_iter().collect()
        }

        pub fn stop(self) {
            self.stop.store(true, Ordering::Relaxed);
            for s in self.outs.lock().expect("outs").values() {
                let _ = s.send(Out::Close);
            }
        }
    }

    fn accept(stream: TcpStream) -> Result<WebSocket<TcpStream>, String> {
        stream.set_nonblocking(false).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;
        let ws = tungstenite::accept(stream).map_err(|e| e.to_string())?;
        ws.get_ref()
            .set_read_timeout(Some(Duration::from_millis(15)))
            .map_err(|e| e.to_string())?;
        Ok(ws)
    }

    /// Move frames both ways until the connection ends; the reason.
    fn pump<S: std::io::Read + std::io::Write>(
        mut ws: WebSocket<S>,
        id: u64,
        tx: &Sender<Ev>,
        out: &Receiver<Out>,
        stop: &AtomicBool,
    ) -> String {
        loop {
            if stop.load(Ordering::Relaxed) {
                let _ = ws.close(None);
                let _ = ws.flush();
                return "stopped".into();
            }
            loop {
                match out.try_recv() {
                    Ok(Out::Data(b)) => {
                        if let Err(e) = ws.send(Message::Binary(b.into())) {
                            return e.to_string();
                        }
                    }
                    Ok(Out::Close) => {
                        let _ = ws.close(None);
                        let _ = ws.flush();
                        return "closed".into();
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return "closed".into(),
                }
            }
            match ws.read() {
                Ok(Message::Binary(b)) => {
                    let _ = tx.send(Ev::Data(id, b.to_vec()));
                }
                Ok(Message::Close(_)) => return "closed by the other side".into(),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(e) => return e.to_string(),
            }
        }
    }

    pub struct Client {
        rx: Receiver<Ev>,
        out: Sender<Out>,
        stop: Arc<AtomicBool>,
    }

    impl Client {
        pub fn connect(url: String) -> Client {
            let (tx, rx) = channel();
            let (otx, orx) = channel();
            let stop = Arc::new(AtomicBool::new(false));
            let s = stop.clone();
            std::thread::spawn(move || {
                let why = match tungstenite::connect(url.as_str()) {
                    Ok((mut ws, _)) => {
                        let t = Some(Duration::from_millis(15));
                        let _ = match ws.get_mut() {
                            tungstenite::stream::MaybeTlsStream::Plain(s) => s.set_read_timeout(t),
                            #[cfg(not(target_os = "android"))]
                            tungstenite::stream::MaybeTlsStream::Rustls(s) => {
                                s.get_mut().set_read_timeout(t)
                            }
                            _ => Ok(()),
                        };
                        let _ = tx.send(Ev::Open(0));
                        pump(ws, 0, &tx, &orx, &s)
                    }
                    Err(e) => e.to_string(),
                };
                let _ = tx.send(Ev::Closed(0, why));
            });
            Client { rx, out: otx, stop }
        }

        pub fn send(&self, b: Vec<u8>) {
            let _ = self.out.send(Out::Data(b));
        }

        pub fn poll(&self) -> Vec<Ev> {
            self.rx.try_iter().collect()
        }
    }

    impl Drop for Client {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_parse_in_both_forms() {
        assert_eq!(
            parse_link("ws://10.0.0.5:8991/?k=abc"),
            Some(("ws://10.0.0.5:8991".into(), "abc".into()))
        );
        assert_eq!(
            parse_link(" https://x.github.io/OG-paper/app/#join=wss://h.ts.net&k=K1 "),
            Some(("wss://h.ts.net".into(), "K1".into()))
        );
        assert_eq!(parse_link("http://example.com"), None);
    }
}
