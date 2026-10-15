// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Plugins: WebAssembly modules that add buttons. A plugin is run by the
//! same interpreter (wasmi) on desktop, Android, iOS and in the browser, and
//! gets nothing from the app but the input it is handed: no files, no
//! network, no clock. Each run is a fresh instance with a fuel limit (a
//! stuck plugin stops) and a memory cap.
//!
//! A plugin exports its `memory` and three functions (all `i32`/`i64`):
//!
//! - `og_alloc(len) -> ptr`: room for `len` bytes of input.
//! - `og_manifest() -> (ptr << 32) | len`: UTF-8 JSON
//!   `{"name": "…", "version": "…", "description": "…",
//!   "buttons": [{"id": 1, "label": "…", "icon": "mic", "place": true}]}`.
//!   Each button is a tool in the tool fan (and a button in Plugins). With
//!   `"place": true` picking it waits for a tap on the canvas, and the
//!   input says where (`"at"`).
//! - `og_run(button, ptr, len) -> (ptr << 32) | len`: given the input JSON,
//!   the commands to run (see `script`), as a JSON array, or
//!   `{"commands": [...]}`.
//!
//! The input is `{"button": id, "view": {"x", "y", "w", "h"}, "texts":
//! [...]}` (and `"at": {"x", "y"}` for a placing tool): the part of the
//! canvas on screen, in the home-view points commands use, and the texts
//! on the page. See `docs/PLUGINS.md`.

use serde_json::{json, Value};
use wasmi::{Config, Engine, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder};

use crate::App;

/// Steps one run may take.
const FUEL: u64 = 500_000_000;
/// Memory one plugin may grow to.
const MEMORY: usize = 64 << 20;
/// Largest manifest or result read back.
const MAX_OUT: usize = 16 << 20;

/// An installed plugin.
#[derive(Clone)]
pub struct Plugin {
    pub name: String,
    pub version: String,
    pub description: String,
    pub buttons: Vec<Button>,
    /// What it asks to use beyond drawing (only these are known:
    /// "microphone"). Nothing is used until the person allows it.
    pub permissions: Vec<String>,
    pub bytes: Vec<u8>,
}

/// A plugin's button: a tool in the tool fan.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Button {
    pub id: i32,
    pub label: String,
    /// Its icon in the fan: one of `ICONS`, or `symbol:<name>` (a symbol
    /// of `symbols`), else a puzzle piece.
    pub icon: String,
    /// Picking it waits for a tap on the canvas, and runs it there.
    pub place: bool,
}

/// A plugin's key in toolbar slots: a stable hash (FNV-1a) of its name.
pub fn key(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The installed plugins' tools, by (plugin key, button), for drawing and
/// naming toolbar slots that hold them.
static TOOLS: std::sync::Mutex<Vec<(u64, Button)>> = std::sync::Mutex::new(Vec::new());

/// The installed plugin tool a slot holds, if it is installed.
pub fn tool(plugin: u64, button: i32) -> Option<Button> {
    TOOLS
        .lock()
        .ok()?
        .iter()
        .find(|(k, b)| *k == plugin && b.id == button)
        .map(|(_, b)| b.clone())
}

/// Every installed plugin tool.
pub fn tools() -> Vec<(u64, Button)> {
    TOOLS.lock().map(|t| t.clone()).unwrap_or_default()
}

/// Icons a button may name.
pub const ICONS: &[&str] = &["mic", "puzzle", "star", "grid", "spiral", "text", "pin"];

/// Permissions a plugin may ask for, and how they read in the panel.
pub const PERMISSIONS: &[(&str, &str)] =
    &[("microphone", "the microphone (to record audio clips)")];

/// Plugins that come with the app, ready to install: name, file.
pub const BUNDLED: &[(&str, &[u8])] = &[
    (
        "Audio notes",
        include_bytes!("../../../plugins/examples/audio-notes.wasm"),
    ),
    (
        "Diagram symbols",
        include_bytes!("../../../plugins/examples/diagram-symbols.wasm"),
    ),
];

/// Whether the person allowed plugin `name` what it asks for.
pub fn allowed(name: &str) -> bool {
    crate::prefs::load()
        .get(&format!("plugin.allow.{name}"))
        .is_some_and(|v| v == "yes")
}

pub fn set_allowed(name: &str, yes: bool) {
    let mut p = crate::prefs::load();
    let k = format!("plugin.allow.{name}");
    if yes {
        p.insert(k, "yes".into());
    } else {
        p.remove(&k);
    }
    crate::prefs::save(&p);
}

struct Live {
    store: Store<StoreLimits>,
    instance: wasmi::Instance,
    memory: Memory,
}

fn instantiate(bytes: &[u8]) -> Result<Live, String> {
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, bytes).map_err(|e| format!("not a plugin: {e}"))?;
    let limits = StoreLimitsBuilder::new().memory_size(MEMORY).build();
    let mut store = Store::new(&engine, limits);
    store.limiter(|l| l);
    store.set_fuel(FUEL).map_err(|e| e.to_string())?;
    // No imports: a plugin gets nothing from the app but its input.
    let linker = Linker::<StoreLimits>::new(&engine);
    let instance = linker
        .instantiate(&mut store, &module)
        .and_then(|pre| pre.start(&mut store))
        .map_err(|e| format!("the plugin could not start: {e}"))?;
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or("the plugin exports no memory")?;
    Ok(Live {
        store,
        instance,
        memory,
    })
}

impl Live {
    /// Read the (ptr << 32 | len) a plugin function returned.
    fn read(&self, packed: i64) -> Result<Vec<u8>, String> {
        let (ptr, len) = ((packed >> 32) as u32 as usize, packed as u32 as usize);
        if len > MAX_OUT {
            return Err("the plugin's answer is too big".into());
        }
        let mut out = vec![0u8; len];
        self.memory
            .read(&self.store, ptr, &mut out)
            .map_err(|_| "the plugin's answer is outside its memory".to_string())?;
        Ok(out)
    }

    fn manifest(&mut self) -> Result<Value, String> {
        let f = self
            .instance
            .get_typed_func::<(), i64>(&self.store, "og_manifest")
            .map_err(|_| "the plugin has no og_manifest")?;
        let packed = f.call(&mut self.store, ()).map_err(|e| e.to_string())?;
        serde_json::from_slice(&self.read(packed)?).map_err(|e| format!("bad manifest: {e}"))
    }

    fn run(&mut self, button: i32, input: &[u8]) -> Result<Value, String> {
        let alloc = self
            .instance
            .get_typed_func::<i32, i32>(&self.store, "og_alloc")
            .map_err(|_| "the plugin has no og_alloc")?;
        let run = self
            .instance
            .get_typed_func::<(i32, i32, i32), i64>(&self.store, "og_run")
            .map_err(|_| "the plugin has no og_run")?;
        let ptr = alloc
            .call(&mut self.store, input.len() as i32)
            .map_err(|e| e.to_string())?;
        self.memory
            .write(&mut self.store, ptr as u32 as usize, input)
            .map_err(|_| "the plugin gave no room for its input")?;
        let packed = run
            .call(&mut self.store, (button, ptr, input.len() as i32))
            .map_err(|e| format!("the plugin stopped: {e}"))?;
        serde_json::from_slice(&self.read(packed)?).map_err(|e| format!("bad answer: {e}"))
    }
}

impl Plugin {
    /// Read a plugin file: start it once and take its manifest.
    pub fn load(bytes: Vec<u8>) -> Result<Plugin, String> {
        let m = instantiate(&bytes)?.manifest()?;
        let s = |k: &str| {
            m.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let name: String = s("name").chars().take(60).collect();
        if name.is_empty() {
            return Err("the plugin has no name".into());
        }
        let buttons = m
            .get("buttons")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .take(32)
                    .filter_map(|b| {
                        let icon = b.get("icon").and_then(Value::as_str).unwrap_or("");
                        Some(Button {
                            id: b.get("id")?.as_i64()? as i32,
                            label: b.get("label")?.as_str()?.chars().take(60).collect(),
                            icon: if ICONS.contains(&icon)
                                || icon
                                    .strip_prefix("symbol:")
                                    .is_some_and(|k| crate::symbols::find(k).is_some())
                            {
                                icon
                            } else {
                                "puzzle"
                            }
                            .into(),
                            place: b.get("place").and_then(Value::as_bool).unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let permissions = m
            .get("permissions")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .filter(|p| PERMISSIONS.iter().any(|(k, _)| k == p))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        Ok(Plugin {
            name,
            permissions,
            version: s("version"),
            description: s("description").chars().take(300).collect(),
            buttons,
            bytes,
        })
    }

    /// Run button `id` with `input`; the commands it answered.
    pub fn press(&self, id: i32, input: &Value) -> Result<Value, String> {
        let out = instantiate(&self.bytes)?.run(id, input.to_string().as_bytes())?;
        Ok(match out {
            Value::Object(mut o) => o.remove("commands").unwrap_or(Value::Array(vec![])),
            v => v,
        })
    }
}

/// Where plugins are kept on desktop: ~/OG Paper/plugins/<name>.wasm.
#[cfg(not(target_arch = "wasm32"))]
pub fn plugin_dir() -> Option<std::path::PathBuf> {
    Some(crate::canvas_dir()?.join("plugins"))
}

impl App {
    /// Install a plugin from its file (keeping it for next time).
    pub(crate) fn plugin_install(&mut self, bytes: Vec<u8>, quiet: bool) {
        // A copy of a plugin that comes with the app, kept from an older
        // version: this version's instead (new tools, fixes).
        let bytes = match Plugin::load(bytes.clone()) {
            Ok(p) if quiet => BUNDLED
                .iter()
                .find(|(n, b)| *n == p.name && *b != bytes.as_slice())
                .map_or(bytes, |(_, b)| b.to_vec()),
            _ => bytes,
        };
        match Plugin::load(bytes) {
            Ok(p) => {
                #[cfg(not(target_arch = "wasm32"))]
                if !quiet {
                    if let Some(dir) = plugin_dir() {
                        let _ = std::fs::create_dir_all(&dir);
                        let safe: String = p
                            .name
                            .chars()
                            .map(|c| {
                                if c.is_alphanumeric() || c == ' ' || c == '-' {
                                    c
                                } else {
                                    '_'
                                }
                            })
                            .collect();
                        let _ = std::fs::write(dir.join(format!("{safe}.wasm")), &p.bytes);
                    }
                }
                if !quiet {
                    self.say(if p.permissions.is_empty() || allowed(&p.name) {
                        format!("Plugin {} installed: its tools are in the tool menu", p.name)
                    } else {
                        format!(
                            "Plugin {} installed: it asks to use the microphone — allow it in Plugins",
                            p.name
                        )
                    });
                    #[cfg(target_arch = "wasm32")]
                    crate::web::page_request("plugin-store", &p.name);
                }
                self.plugins.retain(|q| q.name != p.name);
                self.plugins.push(p);
                self.plugins_to_ui();
            }
            Err(e) => self.say(format!("Could not install that plugin: {e}")),
        }
    }

    /// Plugins kept on this device (desktop: the plugins folder).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn plugins_load_saved(&mut self) {
        let Some(dir) = plugin_dir() else { return };
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            if e.path().extension().is_some_and(|x| x == "wasm") {
                if let Ok(b) = std::fs::read(e.path()) {
                    self.plugin_install(b, true);
                }
            }
        }
    }

    pub(crate) fn plugin_remove(&mut self, i: usize) {
        if i >= self.plugins.len() {
            return;
        }
        let p = self.plugins.remove(i);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(dir) = plugin_dir() {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    if std::fs::read(e.path()).is_ok_and(|b| b == p.bytes) {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        crate::web::page_request("plugin-remove", &p.name);
        self.say(format!("Plugin {} removed", p.name));
        self.plugins_to_ui();
    }

    pub(crate) fn plugins_to_ui(&mut self) {
        if let Ok(mut t) = TOOLS.lock() {
            *t = self
                .plugins
                .iter()
                .flat_map(|p| p.buttons.iter().map(|b| (key(&p.name), b.clone())))
                .collect();
        }
        self.ui.plugins = self
            .plugins
            .iter()
            .map(|p| crate::ui::PluginView {
                name: p.name.clone(),
                info: if p.version.is_empty() {
                    p.description.clone()
                } else {
                    format!("{} · {}", p.version, p.description)
                },
                buttons: p.buttons.clone(),
                asks: p
                    .permissions
                    .iter()
                    .filter_map(|k| {
                        PERMISSIONS
                            .iter()
                            .find(|(n, _)| n == k)
                            .map(|(_, l)| l.to_string())
                    })
                    .collect(),
                allowed: allowed(&p.name),
            })
            .collect();
        self.ui.plugins_bundled = BUNDLED
            .iter()
            .map(|(n, _)| (n.to_string(), self.plugins.iter().any(|p| p.name == *n)))
            .collect();
    }

    /// Allow (or stop allowing) what plugin `i` asks for.
    pub(crate) fn plugin_allow(&mut self, i: usize, yes: bool) {
        let Some(p) = self.plugins.get(i) else {
            return;
        };
        set_allowed(&p.name, yes);
        self.say(if yes {
            format!("{} may use what it asked for", p.name)
        } else {
            format!("{} may no longer use what it asked for", p.name)
        });
        self.plugins_to_ui();
    }

    /// Install a plugin that comes with the app.
    pub(crate) fn plugin_install_bundled(&mut self, i: usize) {
        if let Some((_, bytes)) = BUNDLED.get(i) {
            self.plugin_install(bytes.to_vec(), false);
            #[cfg(target_arch = "wasm32")]
            crate::web::plugin_bytes(bytes.to_vec());
        }
    }

    /// What a plugin is told: the view (home-view points) and the texts.
    fn plugin_input(&self, button: i32) -> Value {
        let home = crate::home_camera();
        let o = self.cam.cell.origin_in(&home.cell);
        let side = self.cam.cell.side_in(&home.cell);
        let [w, h] = self.size();
        let k = home.ppc();
        let c = [o[0] + self.cam.off[0] * side, o[1] + self.cam.off[1] * side];
        let vw = w / self.cam.ppc() * side * k;
        let vh = h / self.cam.ppc() * side * k;
        let texts = self.texts_at(&home);
        json!({
            "button": button,
            "view": {
                "x": (c[0] - home.off[0]) * k - vw * 0.5,
                "y": (c[1] - home.off[1]) * k - vh * 0.5,
                "w": vw,
                "h": vh,
            },
            "texts": texts,
        })
    }

    /// Press a plugin's button: run it and do what it answers. `at`: the
    /// screen point (px) a placing tool was tapped at.
    pub(crate) fn plugin_press(&mut self, plugin: usize, button: i32, at: Option<[f64; 2]>) {
        let Some(p) = self.plugins.get(plugin).cloned() else {
            return;
        };
        let mut input = self.plugin_input(button);
        if let Some(px) = at {
            // The tapped point in the home-view points commands use.
            let home = crate::home_camera();
            let o = self.cam.cell.origin_in(&home.cell);
            let side = self.cam.cell.side_in(&home.cell);
            let k = home.ppc();
            let q = self.px_to_cam(px);
            let c = [o[0] + q[0] * side, o[1] + q[1] * side];
            input["at"] = json!({"x": (c[0] - home.off[0]) * k, "y": (c[1] - home.off[1]) * k});
        }
        match p.press(button, &input) {
            Ok(cmds) => {
                // What it asked for, if the person allowed it.
                self.cmd_perms = if allowed(&p.name) {
                    p.permissions.clone()
                } else {
                    vec![]
                };
                let results = self.run_commands(&cmds);
                self.cmd_perms.clear();
                let first_error = results.as_array().and_then(|r| {
                    r.iter()
                        .find(|x| x.get("ok") == Some(&json!(false)))
                        .and_then(|x| x.get("error")?.as_str().map(str::to_string))
                });
                let failed = results.as_array().map_or(0, |r| {
                    r.iter()
                        .filter(|x| x.get("ok") == Some(&json!(false)))
                        .count()
                });
                if failed == 1 {
                    if let Some(e) = first_error {
                        self.say(format!("{}: {e}", p.name));
                    }
                } else if failed > 0 {
                    self.say(format!("{}: {failed} of its commands did not work", p.name));
                }
            }
            Err(e) => self.say(format!("{}: {e}", p.name)),
        }
        self.redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::ObjData;

    #[test]
    fn the_example_plugin_loads_and_answers() {
        let bytes = include_bytes!("../../../plugins/examples/starter.wasm").to_vec();
        let p = Plugin::load(bytes).unwrap();
        assert_eq!(p.name, "Starter kit");
        assert_eq!(p.buttons.len(), 3);
        let input =
            json!({"button": 2, "view": {"x": -100, "y": -50, "w": 200, "h": 100}, "texts": []});
        let cmds = p.press(2, &input).unwrap();
        let first = &cmds.as_array().unwrap()[0];
        assert_eq!(first["add"], "stroke");
        assert!(first["points"].as_array().unwrap().len() > 100);
        // Garbage is refused, not run.
        assert!(Plugin::load(b"not wasm".to_vec()).is_err());
    }

    #[test]
    fn audio_notes_asks_for_the_microphone() {
        let p = Plugin::load(BUNDLED[0].1.to_vec()).unwrap();
        assert_eq!(p.name, "Audio notes");
        assert_eq!(p.permissions, vec!["microphone".to_string()]);
        // One tool, a mic, that places the note where it's tapped.
        assert_eq!(p.buttons.len(), 1);
        assert_eq!(
            (p.buttons[0].icon.as_str(), p.buttons[0].place),
            ("mic", true)
        );
        let input = json!({"at": {"x": 300, "y": 40}, "button": 1,
                           "view": {"x": -100, "y": -50, "w": 200, "h": 100}, "texts": []});
        let cmds = p.press(1, &input).unwrap();
        assert_eq!(cmds[0]["add"], "audio");
        assert_eq!(cmds[0]["record"], true);
        let (x, w) = (
            cmds[0]["x"].as_f64().unwrap(),
            cmds[0]["w"].as_f64().unwrap(),
        );
        assert!(
            (x + w / 2.0 - 300.0).abs() < 1e-6,
            "centred on the tap: {cmds}"
        );

        let mut app = App::new(None);
        // Without the microphone allowed, recording is refused.
        let r = app.run_commands(&cmds);
        assert_eq!(r[0]["ok"], false);
        assert!(r[0]["error"].as_str().unwrap().contains("microphone"));
        // A clip from data: a tiny WAV, base64.
        let mut wav = b"RIFF\x24\x00\x00\x00WAVEfmt ".to_vec();
        wav.extend_from_slice(&[
            16, 0, 0, 0, 1, 0, 1, 0, 0x40, 0x1f, 0, 0, 0x80, 0x3e, 0, 0, 2, 0, 16, 0,
        ]);
        wav.extend_from_slice(b"data\x00\x00\x00\x00");
        let b64 = {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut o = String::new();
            for ch in wav.chunks(3) {
                let n = (ch[0] as u32) << 16
                    | (*ch.get(1).unwrap_or(&0) as u32) << 8
                    | *ch.get(2).unwrap_or(&0) as u32;
                for k in 0..4 {
                    if k <= ch.len() {
                        o.push(T[(n >> (18 - 6 * k) & 63) as usize] as char);
                    } else {
                        o.push('=');
                    }
                }
            }
            o
        };
        let r = app.run_commands(
            &json!([{"add": "audio", "data": b64, "duration_ms": 1500, "x": 0, "y": 0, "w": 200}]),
        );
        assert_eq!(r[0]["ok"], true, "{r}");
        let g = app
            .objs
            .groups
            .iter()
            .find(|g| matches!(g.data, ObjData::Audio { .. }))
            .expect("a clip");
        let ObjData::Audio { id, dur_ms, .. } = g.data else {
            unreachable!()
        };
        assert_eq!(dur_ms, 1500);
        assert!(app
            .objs
            .images
            .get(&id)
            .is_some_and(|a| a.bytes.as_slice() == wav.as_slice()));
        // A lone clip's bytes round trip; older apps read a rounded box.
        let bytes = crate::snapshot::data_bytes(&g.data);
        assert!(
            matches!(crate::snapshot::get_data(&bytes).unwrap(), ObjData::Audio { id: i, dur_ms: 1500, .. } if i == id)
        );
        let mut plain = Vec::new();
        crate::snapshot::put_data(&mut plain, &g.data);
        assert!(matches!(
            crate::snapshot::get_data(&plain).unwrap(),
            ObjData::Shape { .. }
        ));
        // Not a sound: refused.
        let r = app.run_commands(&json!([{"add": "audio", "data": "aGVsbG8="}]));
        assert_eq!(r[0]["ok"], false);
    }

    #[test]
    fn diagram_symbols_places_symbols_with_pins() {
        let p = Plugin::load(BUNDLED[1].1.to_vec()).unwrap();
        assert_eq!(p.name, "Diagram symbols");
        assert!(p.buttons.len() >= 8);
        assert!(
            p.buttons
                .iter()
                .all(|b| b.place && b.icon.starts_with("symbol:")),
            "{:?}",
            p.buttons
        );
        let input = json!({"at": {"x": 40, "y": 20}, "button": 3,
                           "view": {"x": -350, "y": -350, "w": 700, "h": 700}, "texts": []});
        let cmds = p.press(3, &input).unwrap();
        assert_eq!(cmds[0]["add"], "symbol");
        assert_eq!(cmds[0]["name"], "resistor");
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        let r = app.run_commands(&cmds);
        assert_eq!(r[0]["ok"], true, "{r}");
        // Centred where it was tapped, ten points a unit.
        let (x, w) = (r[0]["x"].as_f64().unwrap(), r[0]["w"].as_f64().unwrap());
        assert!(
            (x + w / 2.0 - 40.0).abs() < 1.0 && (w - 60.0).abs() < 3.0,
            "{r}"
        );
    }
}
