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
    /// The link's keys: every frame is sealed with them (see `seal`).
    keys: crate::seal::Keys,
    /// The log size when changes were last pushed.
    pushed: usize,
}

pub enum Role {
    Host(Host),
    Guest(Guest),
}

pub struct Host {
    pub port: u16,
    /// The link keys handed out (edit: can draw; view: only look).
    pub edit_token: String,
    pub view_token: String,
    pub conns: HashMap<u64, Conn>,
    link: HostLink,
}

/// How a host reaches its guests: its WebSocket server (desktop), or the
/// page's WebRTC data channels (a browser hosting with no server).
pub enum HostLink {
    #[cfg(not(target_arch = "wasm32"))]
    Server(native::Server),
    /// One page of a server hosting many: its connections come and go
    /// through the server's shared listener (see `hub`).
    #[cfg(not(target_arch = "wasm32"))]
    Hub(native::Handle),
    #[cfg(target_arch = "wasm32")]
    Rtc,
}

impl HostLink {
    fn send(&self, id: u64, b: Vec<u8>) {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            HostLink::Server(s) => s.send(id, b),
            #[cfg(not(target_arch = "wasm32"))]
            HostLink::Hub(h) => h.send(id, b),
            #[cfg(target_arch = "wasm32")]
            HostLink::Rtc => crate::web::net_send(id, b),
        }
    }

    fn close(&self, id: u64) {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            HostLink::Server(s) => s.close(id),
            #[cfg(not(target_arch = "wasm32"))]
            HostLink::Hub(h) => h.close(id),
            #[cfg(target_arch = "wasm32")]
            HostLink::Rtc => crate::web::rtc_close(id),
        }
    }

    fn poll(&self) -> Vec<Ev> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            HostLink::Server(s) => s.poll(),
            #[cfg(not(target_arch = "wasm32"))]
            HostLink::Hub(_) => Vec::new(),
            #[cfg(target_arch = "wasm32")]
            HostLink::Rtc => Vec::new(),
        }
    }

    fn stop(self) {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            HostLink::Server(s) => s.stop(),
            #[cfg(not(target_arch = "wasm32"))]
            HostLink::Hub(_) => {}
            #[cfg(target_arch = "wasm32")]
            HostLink::Rtc => crate::web::emit("rtc-stop"),
        }
    }
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
    /// Through a relay (no host): no hello, frames filed under a room.
    pub relay: bool,
    /// The link joined by (relay guests hand it on; see `live_info`).
    link: Link,
    /// What the relay's stored frames held (relay guests), and their size.
    backlog: Version,
    backlog_bytes: usize,
    pub state: GuestState,
    /// What the host has (as far as we know); None until welcomed.
    their: Option<Version>,
    pub edit: bool,
    pub host: u64,
    pub canvas_name: String,
    #[cfg(not(target_arch = "wasm32"))]
    client: Option<native::Client>,
}

/// A relay's word that its stored frames have all been sent.
pub const READY: &[u8] = b"READY";
/// Past this much stored, a relay guest that can edit sends one full copy
/// for the relay to keep instead of everything before.
const CHECKPOINT_AFTER: usize = 1 << 20;

/// Sent in the clear to a connection whose frames the host cannot open.
const BAD_KEY: &[u8] = b"OG-PAPER:BAD-KEY";

/// The address a WebRTC guest has (its page holds the channel).
pub const RTC: &str = "rtc";

/// What a share link says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// The WebSocket address (or [`RTC`]).
    pub url: String,
    /// Its key (see `seal`).
    pub key: String,
    /// The canvas, for relay links (a relay does not know it).
    pub canvas: Option<u128>,
    /// A relay (stores sealed changes) rather than a host.
    pub relay: bool,
}

/// Read a share link: `ws(s)://host:port/?k=KEY`, a relay's
/// `ws(s)://host:port/relay?k=KEY&c=CANVAS`, either inside a web app
/// address as `#join=<address>&k=KEY[&c=CANVAS]`, or a WebRTC invite's
/// `rtc?k=KEY`.
pub fn parse_link(link: &str) -> Option<Link> {
    let link = link.trim();
    if let Some(k) = link.strip_prefix("rtc?k=") {
        return Some(Link {
            url: RTC.into(),
            key: k.into(),
            canvas: None,
            relay: false,
        });
    }
    let (url, params) = if let Some(i) = link.find("#join=") {
        let rest = &link[i + 6..];
        rest.split_once('&').unwrap_or((rest, ""))
    } else {
        link.split_once('?').unwrap_or((link, ""))
    };
    let param = |name: &str| {
        params
            .split('&')
            .find_map(|p| p.strip_prefix(name).and_then(|v| v.strip_prefix('=')))
            .map(str::to_string)
    };
    let url = url.trim_end_matches('/').to_string();
    if !(url.starts_with("ws://") || url.starts_with("wss://")) {
        return None;
    }
    Some(Link {
        relay: url.ends_with("/relay"),
        canvas: param("c").and_then(|c| u128::from_str_radix(&c, 16).ok()),
        key: param("k").unwrap_or_default(),
        url,
    })
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
        matches!(self.role, Role::Host(_))
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
        let keys = match prefs.get(&k).and_then(|t| crate::seal::Keys::parse(t)) {
            Some(keys) if keys.can_edit() => (prefs[&k].clone(), keys),
            _ => {
                let t = crate::seal::Keys::edit_token(&crate::seal::random_secret());
                prefs.insert(k, t.clone());
                crate::prefs::save(&prefs);
                let keys = crate::seal::Keys::parse(&t).expect("new key");
                (t, keys)
            }
        };
        let (edit_token, keys) = keys;
        let view_token = keys.view_token();
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
                edit_token,
                view_token,
                conns: HashMap::new(),
                link: HostLink::Server(server),
            }),
            keys,
            pushed: 0,
        });
        self.ensure_file();
        self.say(format!(
            "Hosting on port {port}: share a link from Share live"
        ));
    }

    /// Host this canvas as one page of a server (see `hub`): the server's
    /// listener carries its connections. The page's keys are kept per
    /// canvas like a desktop host's.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn host_attach(&mut self, link: native::Handle) {
        self.net_stop();
        let (edit_token, keys) = self.page_keys();
        let view_token = keys.view_token();
        let code = crate::prefs::load()
            .get(&format!("policy.{:032x}", self.share.canvas))
            .and_then(|v| v.parse::<u8>().ok())
            .unwrap_or(0);
        self.share.log.policy = Policy::from_code(code, self.share.clock.peer());
        self.net = Some(Net {
            role: Role::Host(Host {
                port: 1,
                edit_token,
                view_token,
                conns: HashMap::new(),
                link: HostLink::Hub(link),
            }),
            keys,
            pushed: 0,
        });
    }

    /// This canvas's host keys (made and kept on first use).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn page_keys(&self) -> (String, crate::seal::Keys) {
        page_keys_for(self.share.canvas)
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
        let Some(parsed) = parse_link(link) else {
            self.say("That is not a share link (it starts with ws:// or wss://)");
            return;
        };
        let Link {
            url,
            key,
            canvas,
            relay,
        } = parsed.clone();
        let Some(keys) = crate::seal::Keys::parse(&key) else {
            self.say("That link's key is incomplete: copy the whole link again");
            return;
        };
        self.net_stop();
        // A relay link names its canvas: start a copy of it here if new.
        if let Some(c) = canvas.filter(|c| relay && *c != self.share.canvas) {
            self.switch_to_shared(c, "");
        }
        if relay {
            self.view_only = !keys.can_edit();
            self.share.log.policy = Policy::Newest;
        }
        self.net = Some(Net {
            keys,
            role: Role::Guest(Guest {
                url,
                relay,
                link: parsed,
                backlog: Version::default(),
                backlog_bytes: 0,
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
        g.backlog = Version::default();
        g.backlog_bytes = 0;
        // A WebRTC invite: the page holds the channel; nothing to dial.
        if g.url == RTC {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            g.client = Some(native::Client::connect(g.url.clone()));
        }
        #[cfg(target_arch = "wasm32")]
        crate::web::net_connect(&g.url);
    }

    /// Host from this browser with no server: guests come in through
    /// WebRTC invites the page makes (with these keys inside).
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn host_start_rtc(&mut self, edit_token: String) {
        self.net_stop();
        let Some(keys) = crate::seal::Keys::parse(&edit_token).filter(|k| k.can_edit()) else {
            self.say("Could not start hosting: bad key");
            return;
        };
        let view_token = keys.view_token();
        self.net = Some(Net {
            role: Role::Host(Host {
                port: 0,
                edit_token,
                view_token,
                conns: HashMap::new(),
                link: HostLink::Rtc,
            }),
            keys,
            pushed: 0,
        });
        self.say("Hosting in this browser: invite people from Share live (keep this tab open)");
    }

    /// Stop hosting or leave.
    pub(crate) fn net_stop(&mut self) {
        let Some(n) = self.net.take() else { return };
        match n.role {
            Role::Host(h) => h.link.stop(),
            Role::Guest(_g) => {
                #[cfg(target_arch = "wasm32")]
                crate::web::net_close();
            }
        }
        self.view_only = false;
        self.peers.clear();
    }

    fn send(&mut self, conn: u64, m: &Msg) {
        let Some(n) = self.net.as_mut() else { return };
        let changes = matches!(m, Msg::Changes(_) | Msg::Images(_));
        let mut b = n.keys.seal(&m.encode(), changes);
        // To a relay: changes are kept, the rest only passed on.
        if matches!(&n.role, Role::Guest(g) if g.relay) {
            let mut f = if changes {
                b"PUT ".to_vec()
            } else {
                b"EPH ".to_vec()
            };
            f.extend_from_slice(&b);
            b = f;
        }
        self.send_raw(conn, b);
    }

    /// Send bytes as they are (sealed already, or a relay's plain frames).
    fn send_raw(&mut self, conn: u64, b: Vec<u8>) {
        let Some(n) = self.net.as_mut() else { return };
        match &mut n.role {
            Role::Host(h) => h.link.send(conn, b),
            Role::Guest(_g) => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(c) = &_g.client {
                    c.send(b);
                }
                #[cfg(target_arch = "wasm32")]
                crate::web::net_send(0, b);
            }
        }
    }

    /// Send to every guest (host) or the host (guest), except `but`.
    pub(crate) fn broadcast(&mut self, m: &Msg, but: Option<u64>) {
        let targets: Vec<u64> = match self.net.as_ref().map(|n| &n.role) {
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
            Role::Host(h) => h.link.poll(),
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
            let relay = self.relay_guest();
            let (bytes, count) = self.changes_since(&v, relay);
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
        // The web page's directory connections (Pages) share this road.
        #[cfg(target_arch = "wasm32")]
        if matches!(&e, Ev::Open(c) | Ev::Data(c, _) | Ev::Closed(c, _) if *c >= crate::pages::DIR_CONN)
        {
            self.dir_event(e);
            return;
        }
        match e {
            Ev::Open(id) => {
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
                // A guest's connection opened: say hello (to a relay: which
                // room).
                let _ = id;
                let room = match self.net.as_ref() {
                    Some(n) if matches!(&n.role, Role::Guest(g) if g.relay) => Some(n.keys.room()),
                    _ => None,
                };
                if let Some(room) = room {
                    let mut f = b"ROOM".to_vec();
                    f.extend_from_slice(&room);
                    self.send_raw(0, f);
                    return;
                }
                let hello = Msg::Hello {
                    canvas: self.share.canvas,
                    peer: self.share.clock.peer(),
                    version: self.share.log.version().clone(),
                    // Only a claim, for the host's list: edits are checked by
                    // their signatures.
                    key: if self.net.as_ref().is_some_and(|n| n.keys.can_edit()) {
                        "edit".into()
                    } else {
                        "view".into()
                    },
                    name: crate::presence::my_name(),
                    proto: crate::wire::PROTOCOL,
                };
                self.send(0, &hello);
            }
            Ev::Data(_, b) if b == READY && self.relay_guest() => self.relay_ready(),
            Ev::Data(id, b) => {
                let host = self.net.as_ref().is_some_and(|n| n.is_host());
                if let Some(Net {
                    role: Role::Guest(g),
                    ..
                }) = self.net.as_mut()
                {
                    if g.relay && g.state != GuestState::Live {
                        g.backlog_bytes += b.len();
                    }
                }
                let opened = self.net.as_ref().and_then(|n| n.keys.open(&b));
                let Some((plain, signed)) = opened else {
                    if host {
                        // Not our key: tell it plainly (it cannot read us).
                        if let Some(Net {
                            role: Role::Host(h),
                            ..
                        }) = self.net.as_ref()
                        {
                            h.link.send(id, BAD_KEY.to_vec());
                            h.link.close(id);
                        }
                    } else if b == BAD_KEY {
                        if let Some(Net {
                            role: Role::Guest(g),
                            ..
                        }) = self.net.as_mut()
                        {
                            g.state = GuestState::Refused;
                        }
                        self.say("The host did not accept this link's key: ask for a new link");
                    }
                    return;
                };
                match Msg::decode(&plain) {
                    // Changes and pictures count only when signed by an edit key.
                    Ok(Msg::Changes(_) | Msg::Images(_)) if !signed => {
                        log::warn!("unsigned changes dropped");
                    }
                    Ok(Msg::NeedImages(ids)) => self.send_images(id, &ids),
                    Ok(Msg::Images(all)) => self.take_images(all),
                    Ok(m) if host => self.host_msg(id, m),
                    Ok(m) => self.guest_msg(m),
                    Err(e) => log::warn!("bad message: {e}"),
                }
            }
            Ev::Closed(id, why) => {
                let host = self.net.as_ref().is_some_and(|n| n.is_host());
                if host {
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
                        g.state = if g.url == RTC {
                            // A WebRTC channel cannot redial: a new invite does.
                            GuestState::Refused
                        } else {
                            GuestState::Offline(Instant::now() + RETRY)
                        };
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
                let edit = key == "edit";
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
                    proto: crate::wire::PROTOCOL,
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
                    Some(Role::Host(h)) if h.conns.get(&id).is_some_and(|c| c.their.is_some())
                );
                if !can {
                    return;
                }
                if let Ok(s) = crate::snapshot::decode(&b, crate::BASE_PX) {
                    if let Some(v) = self.merge_quiet(s.scene, s.objs, s.share) {
                        self.set_their(id, &v);
                    }
                    self.ask_images(id);
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
                ..
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
                    ..
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
                        if let Some(Net {
                            role: Role::Guest(g),
                            ..
                        }) = self.net.as_mut()
                        {
                            if g.relay {
                                g.backlog.join(&v);
                            }
                        }
                    }
                    if !self.relay_guest() {
                        self.ask_images(0);
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
            _ => {}
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
            Role::Host(h) => {
                let rtc = h.port == 0;
                #[cfg(not(target_arch = "wasm32"))]
                let ws = self
                    .public_addr
                    .clone()
                    .map(|a| a.trim_end_matches('/').to_string())
                    .unwrap_or_else(|| format!("ws://{}:{}", local_ip(), h.port));
                #[cfg(target_arch = "wasm32")]
                let ws = String::new();
                crate::ui::LiveInfo {
                    hosting: true,
                    state: if rtc {
                        "Hosting in this browser (no server)".into()
                    } else {
                        format!("Hosting on port {}", h.port)
                    },
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
                    links: if rtc {
                        vec![]
                    } else {
                        vec![
                            (
                                "Edit link (apps)".into(),
                                format!("{ws}/?k={}", h.edit_token),
                            ),
                            (
                                "View link (apps)".into(),
                                format!("{ws}/?k={}", h.view_token),
                            ),
                            (
                                "Edit link (browser)".into(),
                                format!("{}#join={ws}&k={}", WEB_APP, h.edit_token),
                            ),
                        ]
                    },
                    policy: self.share.log.policy.code(),
                    view_only: false,
                }
            }
            Role::Guest(g) if g.relay => {
                let base = g.link.url.clone();
                let c = self.share.canvas;
                let mut links = Vec::new();
                if n.keys.can_edit() {
                    links.push((
                        "Edit link (apps)".into(),
                        format!("{base}?k={}&c={c:032x}", g.link.key),
                    ));
                    links.push((
                        "Edit link (browser)".into(),
                        format!("{WEB_APP}#join={base}&k={}&c={c:032x}", g.link.key),
                    ));
                }
                links.push((
                    "View link (apps)".into(),
                    format!("{base}?k={}&c={c:032x}", n.keys.view_token()),
                ));
                crate::ui::LiveInfo {
                    hosting: false,
                    state: match g.state {
                        GuestState::Live => "Synced through the relay".into(),
                        GuestState::Connecting => "Connecting to the relay…".into(),
                        GuestState::Offline(_) => {
                            "Relay unreachable: your changes sync when back".into()
                        }
                        GuestState::Refused => "The relay closed the connection".into(),
                    },
                    people: self
                        .peers
                        .values()
                        .map(|p| (p.peer, display_name(&p.name)))
                        .collect(),
                    links,
                    policy: 0,
                    view_only: self.view_only,
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
                    GuestState::Refused if g.url == RTC => {
                        "Disconnected: ask the host for a new invite".into()
                    }
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

    /// Share this canvas through the relay at `addr` with a new key (the
    /// edit token; the page makes it on the web).
    pub(crate) fn relay_share(&mut self, addr: &str, edit_token: &str) {
        let addr = addr.trim().trim_end_matches('/').trim_end_matches("/relay");
        if !(addr.starts_with("ws://") || addr.starts_with("wss://")) {
            self.say("A relay address starts with ws:// or wss://");
            return;
        }
        let mut p = crate::prefs::load();
        p.insert("relay".into(), addr.to_string());
        crate::prefs::save(&p);
        let link = format!("{addr}/relay?k={edit_token}&c={:032x}", self.share.canvas);
        self.join(&link);
    }

    /// Share through a relay with a fresh key (desktop makes it here).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn relay_share_new(&mut self, addr: &str) {
        let t = crate::seal::Keys::edit_token(&crate::seal::random_secret());
        self.relay_share(addr, &t);
    }

    /// Ask `conn` for pictures the canvas shows but this copy lacks.
    fn ask_images(&mut self, conn: u64) {
        let mut want: Vec<u64> = Vec::new();
        for g in &self.objs.groups {
            if let crate::objects::ObjData::Image { id, .. } = g.data {
                if !self.objs.images.contains_key(&id) && !want.contains(&id) {
                    want.push(id);
                }
            }
        }
        if !want.is_empty() {
            self.send(conn, &Msg::NeedImages(want));
        }
    }

    /// Send `conn` the pictures it asked for that this copy has.
    fn send_images(&mut self, conn: u64, ids: &[u64]) {
        let all: Vec<(u64, Vec<u8>)> = ids
            .iter()
            .filter_map(|id| self.objs.images.get(id).map(|a| (*id, a.bytes.to_vec())))
            .collect();
        if !all.is_empty() {
            self.send(conn, &Msg::Images(all));
        }
    }

    /// Pictures arrived: keep those whose content matches their id.
    fn take_images(&mut self, all: Vec<(u64, Vec<u8>)>) {
        for (id, b) in all {
            if crate::images::id_of(&b) != id || self.objs.images.contains_key(&id) {
                continue;
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(f) = &self.file {
                let _ = f.put_image(id, &b);
            }
            if let Ok(a) = crate::images::load(b) {
                self.objs.images.insert(id, a);
                if let Some(g) = self.gpu.as_mut() {
                    g.forget_picture(id);
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        crate::web::touch();
        self.redraw();
    }

    /// New keys: everyone with an old link is cut off and needs a new one.
    pub(crate) fn new_links(&mut self) {
        let (host_port, relay_addr) = match self.net.as_ref().map(|n| &n.role) {
            Some(Role::Host(h)) => (Some(h.port), None),
            Some(Role::Guest(g)) if g.relay => (None, Some(g.link.url.clone())),
            _ => (None, None),
        };
        match (host_port, relay_addr) {
            #[cfg(not(target_arch = "wasm32"))]
            (Some(port), _) if port != 0 => {
                let mut p = crate::prefs::load();
                p.remove(&format!("host.{:032x}", self.share.canvas));
                crate::prefs::save(&p);
                self.host_start(port);
                self.say("New links made: the old ones no longer work");
            }
            // A browser host: invites carry the keys; start afresh.
            (Some(_), _) => {
                self.net_stop();
                #[cfg(target_arch = "wasm32")]
                crate::web::emit("rtc");
                self.say("Hosting stopped: new invites use new keys");
            }
            (None, Some(addr)) => {
                #[cfg(not(target_arch = "wasm32"))]
                self.relay_share_new(&addr);
                #[cfg(target_arch = "wasm32")]
                {
                    self.ui.relay_text = Some(addr);
                    crate::web::emit("relay-new");
                }
                self.say("New relay links made: the old ones no longer get changes");
            }
            _ => {}
        }
    }

    fn relay_guest(&self) -> bool {
        matches!(self.net.as_ref().map(|n| &n.role), Some(Role::Guest(g)) if g.relay)
    }

    /// The relay sent everything it kept: send it what it lacks (all done
    /// here since), and compact what it keeps when that has grown big.
    fn relay_ready(&mut self) {
        let Some(Net {
            role: Role::Guest(g),
            pushed,
            keys,
        }) = self.net.as_mut()
        else {
            return;
        };
        let first = g.host == 0;
        g.host = 1;
        g.state = GuestState::Live;
        g.their = Some(g.backlog.clone());
        g.edit = keys.can_edit();
        *pushed = usize::MAX;
        let big = g.backlog_bytes > CHECKPOINT_AFTER && keys.can_edit();
        if big {
            let (bytes, _) = self.changes_since(&Version::default(), true);
            let sealed = self
                .net
                .as_ref()
                .map(|n| n.keys.seal(&Msg::Changes(bytes).encode(), true))
                .unwrap_or_default();
            let mut f = b"CKPT".to_vec();
            f.extend_from_slice(&sealed);
            self.send_raw(0, f);
            let v = self.share.log.version().clone();
            self.set_their(0, &v);
        }
        self.say(if first {
            "Synced through the relay"
        } else {
            "Back online: synced"
        });
        if let Some(m) = self.my_presence() {
            self.send(0, &m);
        }
        self.redraw();
    }

    /// Whether a connection needs frequent wake-ups.
    pub(crate) fn net_active(&self) -> bool {
        self.net.is_some()
    }
}

/// A canvas's host keys, made and kept (in the prefs) on first use: the
/// edit token and the keys.
#[cfg(not(target_arch = "wasm32"))]
pub fn page_keys_for(canvas: u128) -> (String, crate::seal::Keys) {
    let mut prefs = crate::prefs::load();
    let k = format!("host.{canvas:032x}");
    match prefs.get(&k).and_then(|t| crate::seal::Keys::parse(t)) {
        Some(keys) if keys.can_edit() => (prefs[&k].clone(), keys),
        _ => {
            let t = crate::seal::Keys::edit_token(&crate::seal::random_secret());
            prefs.insert(k, t.clone());
            crate::prefs::save(&prefs);
            let keys = crate::seal::Keys::parse(&t).expect("new key");
            (t, keys)
        }
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
pub(crate) mod native {
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
        /// A text frame (the JSON API).
        Text(String),
        Close,
    }

    type Outs = Arc<Mutex<HashMap<u64, Sender<Out>>>>;

    pub struct Server {
        pub port: u16,
        rx: Receiver<Ev>,
        outs: Outs,
        stop: Arc<AtomicBool>,
        /// The path each connection asked for (`/`, `/p/<canvas>`, …).
        paths: Arc<Mutex<HashMap<u64, String>>>,
        web: Web,
    }

    /// Sends to a server's connections (shared by the pages it hosts).
    #[derive(Clone)]
    pub struct Handle {
        outs: Outs,
    }

    impl Handle {
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
    }

    impl Server {
        pub fn start(port: u16) -> std::io::Result<Server> {
            let l = TcpListener::bind(("0.0.0.0", port))?;
            let port = l.local_addr()?.port();
            l.set_nonblocking(true)?;
            let (tx, rx) = channel();
            let outs: Outs = Arc::default();
            let stop = Arc::new(AtomicBool::new(false));
            let paths: Arc<Mutex<HashMap<u64, String>>> = Arc::default();
            let web: Web = Arc::default();
            let (o, s, ps, wb) = (outs.clone(), stop.clone(), paths.clone(), web.clone());
            std::thread::spawn(move || {
                let next = AtomicU64::new(1);
                while !s.load(Ordering::Relaxed) {
                    match l.accept() {
                        Ok((stream, _)) => {
                            let id = next.fetch_add(1, Ordering::Relaxed);
                            let (otx, orx) = channel();
                            o.lock().expect("outs").insert(id, otx);
                            let (tx, o, s, ps) = (tx.clone(), o.clone(), s.clone(), ps.clone());
                            let wb = wb.clone();
                            std::thread::spawn(move || {
                                let why = match accept(stream, &wb) {
                                    Ok((ws, path)) => {
                                        ps.lock().expect("paths").insert(id, path);
                                        let _ = tx.send(Ev::Open(id));
                                        pump(ws, id, &tx, &orx, &s)
                                    }
                                    Err(e) => e,
                                };
                                o.lock().expect("outs").remove(&id);
                                let _ = tx.send(Ev::Closed(id, why));
                                ps.lock().expect("paths").remove(&id);
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
                paths,
                web,
            })
        }

        /// Answer plain web requests (a browser opening the server's
        /// address) with `page`.
        pub fn set_web(&self, page: impl Fn(&WebReq) -> WebResp + Send + Sync + 'static) {
            *self.web.lock().expect("web") = Some(Arc::new(page));
        }

        /// The path connection `id` asked for.
        pub fn path(&self, id: u64) -> Option<String> {
            self.paths.lock().expect("paths").get(&id).cloned()
        }

        pub fn handle(&self) -> Handle {
            Handle {
                outs: self.outs.clone(),
            }
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

        /// Send a text frame (the JSON API).
        pub fn send_text(&self, id: u64, t: String) {
            if let Some(s) = self.outs.lock().expect("outs").get(&id) {
                let _ = s.send(Out::Text(t));
            }
        }

        pub fn stop(self) {
            self.stop.store(true, Ordering::Relaxed);
            for s in self.outs.lock().expect("outs").values() {
                let _ = s.send(Out::Close);
            }
        }
    }

    /// What answers plain web requests, if anything (see `Server::set_web`).
    /// A plain web request to the server (a browser, not a WebSocket).
    pub struct WebReq {
        pub method: String,
        /// Path and query.
        pub target: String,
        /// The address the visitor used, as a WebSocket base: `ws://host:port`,
        /// or `wss://` behind a TLS proxy (X-Forwarded-Proto: https).
        pub base: String,
        /// Header names in lower case.
        pub headers: Vec<(String, String)>,
        pub body: Vec<u8>,
    }

    impl WebReq {
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        }
    }

    /// The answer to a `WebReq`.
    pub struct WebResp {
        pub status: u16,
        pub ctype: &'static str,
        /// Extra headers (Set-Cookie, Location, …).
        pub headers: Vec<(String, String)>,
        pub body: Vec<u8>,
    }

    type WebFn = dyn Fn(&WebReq) -> WebResp + Send + Sync;
    type Web = Arc<Mutex<Option<Arc<WebFn>>>>;

    /// Largest request body a web page may send (forms).
    const WEB_BODY_MAX: usize = 1 << 20;

    /// A plain web request (not a WebSocket upgrade): answered by `web` and
    /// closed. True when it was one.
    fn web_request(stream: &mut TcpStream, web: &Web) -> bool {
        use std::io::{Read, Write};
        let Some(page) = web.lock().expect("web").clone() else {
            return false;
        };
        // Look at the request without taking it, until its headers are in.
        let mut peek = [0u8; 8192];
        let start = std::time::Instant::now();
        let n = loop {
            let n = stream.peek(&mut peek).unwrap_or(0);
            if n == peek.len() || peek[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                break n;
            }
            if start.elapsed() > Duration::from_secs(3) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let lower = String::from_utf8_lossy(&peek[..n]).to_ascii_lowercase();
        let method_ok = ["get ", "post ", "head "]
            .iter()
            .any(|m| lower.starts_with(m));
        if !method_ok || lower.contains("upgrade: websocket") {
            return false;
        }
        // It is ours: take the head, then the body (Content-Length).
        let mut raw = Vec::new();
        let mut chunk = [0u8; 8192];
        let head_end = loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => return true,
                Ok(k) => raw.extend_from_slice(&chunk[..k]),
            }
            if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
            if raw.len() > 64 * 1024 {
                return true;
            }
        };
        let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
        let mut lines = head.lines();
        let first = lines.next().unwrap_or("");
        let mut words = first.split_whitespace();
        let method = words.next().unwrap_or("GET").to_ascii_uppercase();
        let target = words.next().unwrap_or("/").to_string();
        let headers: Vec<(String, String)> = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            .collect();
        let header = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        };
        let len: usize = header("content-length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if len > WEB_BODY_MAX {
            return true;
        }
        let mut body = raw[head_end..].to_vec();
        while body.len() < len {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(k) => body.extend_from_slice(&chunk[..k]),
            }
        }
        body.truncate(len);
        let host = header("x-forwarded-host")
            .or_else(|| header("host"))
            .unwrap_or_default();
        let tls = header("x-forwarded-proto").is_some_and(|p| p.eq_ignore_ascii_case("https"));
        let base = format!("{}://{host}", if tls { "wss" } else { "ws" });
        let req = WebReq {
            method: method.clone(),
            target,
            base,
            headers,
            body,
        };
        let resp = page(&req);
        let reason = match resp.status {
            200 => "OK",
            303 => "See Other",
            400 => "Bad Request",
            403 => "Forbidden",
            _ => "Not Found",
        };
        let mut out = format!(
            "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\
             Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
             X-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n",
            resp.status,
            resp.ctype,
            resp.body.len()
        );
        for (k, v) in &resp.headers {
            out += &format!("{k}: {}\r\n", v.replace(['\r', '\n'], ""));
        }
        out += "\r\n";
        let mut out = out.into_bytes();
        if method != "HEAD" {
            out.extend_from_slice(&resp.body);
        }
        let _ = stream.write_all(&out);
        let _ = stream.flush();
        true
    }

    /// The WebSocket handshake; the connection and the path it asked for.
    fn accept(mut stream: TcpStream, web: &Web) -> Result<(WebSocket<TcpStream>, String), String> {
        stream.set_nonblocking(false).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;
        if web_request(&mut stream, web) {
            return Err("web page".into());
        }
        let mut path = String::from("/");
        let cb = |req: &tungstenite::handshake::server::Request,
                  resp: tungstenite::handshake::server::Response| {
            path = req.uri().path().to_string();
            Ok(resp)
        };
        let ws = tungstenite::accept_hdr(stream, cb).map_err(|e| e.to_string())?;
        ws.get_ref()
            .set_read_timeout(Some(Duration::from_millis(15)))
            .map_err(|e| e.to_string())?;
        Ok((ws, path))
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
                    Ok(Out::Text(t)) => {
                        if let Err(e) = ws.send(Message::Text(t.into())) {
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
                // Text frames: the JSON API's (bytes of the UTF-8).
                Ok(Message::Text(t)) => {
                    let _ = tx.send(Ev::Data(id, t.as_bytes().to_vec()));
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
    fn links_parse_in_every_form() {
        let l = parse_link("ws://10.0.0.5:8991/?k=abc").unwrap();
        assert_eq!(
            (l.url.as_str(), l.key.as_str(), l.relay),
            ("ws://10.0.0.5:8991", "abc", false)
        );
        let l = parse_link(" https://x.github.io/OG-paper/app/#join=wss://h.ts.net&k=K1 ").unwrap();
        assert_eq!((l.url.as_str(), l.key.as_str()), ("wss://h.ts.net", "K1"));
        let l = parse_link("wss://go:8993/relay?k=e1&c=ff").unwrap();
        assert!(l.relay && l.canvas == Some(255) && l.key == "e1");
        let l = parse_link("https://a/app/#join=ws://r:1/relay&k=e2&c=10").unwrap();
        assert!(l.relay && l.canvas == Some(16) && l.key == "e2");
        assert_eq!(parse_link("rtc?k=e3").unwrap().url, RTC);
        assert_eq!(parse_link("http://example.com"), None);
    }
}
