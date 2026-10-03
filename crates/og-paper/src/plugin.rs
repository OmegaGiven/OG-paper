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
//!   "buttons": [{"id": 1, "label": "…"}]}`.
//! - `og_run(button, ptr, len) -> (ptr << 32) | len`: given the input JSON,
//!   the commands to run (see `script`), as a JSON array, or
//!   `{"commands": [...]}`.
//!
//! The input is `{"button": id, "view": {"x", "y", "w", "h"}, "texts":
//! [...]}`: the part of the canvas on screen, in the home-view points
//! commands use, and the texts on the page. See `docs/PLUGINS.md`.

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
    /// Button id and label.
    pub buttons: Vec<(i32, String)>,
    pub bytes: Vec<u8>,
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
                        Some((
                            b.get("id")?.as_i64()? as i32,
                            b.get("label")?.as_str()?.chars().take(60).collect(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Plugin {
            name,
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
                    self.say(format!(
                        "Plugin {} installed: its buttons are in Plugins",
                        p.name
                    ));
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
            })
            .collect();
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

    /// Press a plugin's button: run it and do what it answers.
    pub(crate) fn plugin_press(&mut self, plugin: usize, button: i32) {
        let Some(p) = self.plugins.get(plugin).cloned() else {
            return;
        };
        let input = self.plugin_input(button);
        match p.press(button, &input) {
            Ok(cmds) => {
                let results = self.run_commands(&cmds);
                let failed = results.as_array().map_or(0, |r| {
                    r.iter()
                        .filter(|x| x.get("ok") == Some(&json!(false)))
                        .count()
                });
                if failed > 0 {
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
}
