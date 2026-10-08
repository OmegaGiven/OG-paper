// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! The selection panel's Details tab: what the selected things are and
//! their history, from the canvas's sync log (every item is stamped with
//! when and on which device it was made; moving or restyling makes a new
//! item that replaces the old, so following replacements back finds when
//! a thing was first made and how often it changed), with the timeline's
//! times for strokes the log doesn't have.

use std::collections::HashMap;

use ogpaper_core::sync::Hlc;

use crate::objects::{ObjData, ObjRef};
use crate::App;

/// One thing's history: first made, last changed (if it was), how many
/// edits in between.
struct History {
    made: Hlc,
    changed: Option<Hlc>,
    edits: usize,
}

impl App {
    /// Label, value rows describing the selection.
    pub(crate) fn sel_details(&self, sel: &[ObjRef]) -> Vec<(String, String)> {
        if sel.is_empty() {
            return vec![];
        }
        // When each item became shown, and what each replaced.
        let mut shown: HashMap<u128, Hlc> = HashMap::new();
        for e in self.share.log.events() {
            if e.alive {
                shown
                    .entry(e.item)
                    .and_modify(|h| *h = (*h).min(e.at))
                    .or_insert(e.at);
            }
        }
        let replaced: HashMap<u128, (u128, Hlc)> = self
            .share
            .log
            .replacements()
            .map(|r| (r.item, (r.replaces, r.edit)))
            .collect();
        let history = |uid: u128| -> Option<History> {
            let mut item = uid;
            let mut changed = None;
            let mut edits = 0;
            let mut seen = 0;
            while let Some(&(old, edit)) = replaced.get(&item) {
                changed.get_or_insert(edit);
                edits += 1;
                item = old;
                seen += 1;
                if seen > 10_000 {
                    break;
                }
            }
            let made = shown.get(&item).copied().or(changed)?;
            Some(History {
                made,
                changed,
                edits,
            })
        };
        let mut rows = Vec::new();
        let mut kinds: Vec<String> = Vec::new();
        let mut made: Vec<Hlc> = Vec::new();
        let mut changed: Vec<Hlc> = Vec::new();
        let mut people: Vec<u64> = Vec::new();
        let mut edits = 0;
        let mut facts: Vec<(String, String)> = Vec::new();
        for r in sel {
            let (uid, kind, fact) = match r {
                ObjRef::Ink(id) => {
                    let s = &self.scene.strokes[*id as usize];
                    let n = self.scene.stroke_points(*id).len();
                    (
                        s.uid,
                        "stroke".to_string(),
                        ("Points".to_string(), n.to_string()),
                    )
                }
                ObjRef::Group(g) => {
                    let grp = &self.objs.groups[*g as usize];
                    let uid = grp
                        .strokes
                        .first()
                        .map_or(0, |&s| self.scene.strokes[s as usize].uid);
                    let (kind, fact) = match &grp.data {
                        ObjData::Shape { style, .. } => (
                            "shape".to_string(),
                            ("Shape".to_string(), style.kind.name().to_string()),
                        ),
                        ObjData::Text { text, .. } => (
                            "text".to_string(),
                            ("Characters".to_string(), text.chars().count().to_string()),
                        ),
                        ObjData::Table { cells, .. } => (
                            "table".to_string(),
                            (
                                "Size".to_string(),
                                format!(
                                    "{} rows × {} columns",
                                    cells.len(),
                                    cells.iter().map(Vec::len).max().unwrap_or(0)
                                ),
                            ),
                        ),
                        ObjData::Image { id, .. } => (
                            "picture".to_string(),
                            (
                                "Picture".to_string(),
                                self.objs.images.get(id).map_or("not here yet".into(), |a| {
                                    format!("{} × {} px", a.w, a.h)
                                }),
                            ),
                        ),
                        ObjData::Audio { dur_ms, id, .. } => (
                            "audio clip".to_string(),
                            (
                                "Length".to_string(),
                                format!(
                                    "{}{}",
                                    crate::audio::clock(*dur_ms),
                                    self.objs.images.get(id).map_or(String::new(), |a| format!(
                                        " · {} KB",
                                        a.bytes.len().div_ceil(1024)
                                    ))
                                ),
                            ),
                        ),
                        ObjData::Portal { view, .. } => (
                            "portal".to_string(),
                            (
                                "Shows".to_string(),
                                if view.name.is_empty() {
                                    "a view".into()
                                } else {
                                    view.name.clone()
                                },
                            ),
                        ),
                    };
                    (uid, kind, fact)
                }
            };
            kinds.push(kind);
            if sel.len() == 1 {
                facts.push(fact);
            }
            match history(uid) {
                Some(h) => {
                    made.push(h.made);
                    if !people.contains(&h.made.peer) {
                        people.push(h.made.peer);
                    }
                    if let Some(c) = h.changed {
                        changed.push(c);
                        if !people.contains(&c.peer) {
                            people.push(c.peer);
                        }
                    }
                    edits += h.edits;
                }
                None => {
                    // Not in the sync log: the timeline's time, if any.
                    if let ObjRef::Ink(id) = r {
                        if let Some(e) =
                            self.timeline.events.iter().find(|e| e.id == *id && e.alive)
                        {
                            made.push(Hlc {
                                ms: e.t.max(0) as u64,
                                n: 0,
                                peer: self.share.clock.peer(),
                            });
                        }
                    }
                }
            }
        }
        rows.push(("What".into(), count_kinds(&kinds)));
        made.sort();
        changed.sort();
        match (made.first(), made.last()) {
            (Some(a), Some(b)) if sel.len() == 1 || a.ms / 60_000 == b.ms / 60_000 => {
                rows.push((
                    "Made".into(),
                    format!("{} · {}", when(a.ms), self.who(a.peer)),
                ));
            }
            (Some(a), Some(b)) => {
                rows.push(("Made".into(), format!("{} – {}", when(a.ms), when(b.ms))));
            }
            _ => rows.push(("Made".into(), "not recorded".into())),
        }
        match changed.last() {
            Some(c) => rows.push((
                "Changed".into(),
                format!(
                    "{} {} · last {} · {}",
                    edits,
                    if edits == 1 { "time" } else { "times" },
                    when(c.ms),
                    self.who(c.peer)
                ),
            )),
            None if !made.is_empty() => rows.push(("Changed".into(), "never".into())),
            None => {}
        }
        if sel.len() > 1 && !people.is_empty() {
            let names: Vec<String> = people.iter().map(|&p| self.who(p)).collect();
            rows.push(("By".into(), names.join(", ")));
        }
        rows.extend(facts);
        rows
    }

    /// Who a device is: you, someone here now by name, or its short id.
    fn who(&self, peer: u64) -> String {
        if peer == self.share.clock.peer() {
            return "you".into();
        }
        match self
            .peers
            .get(&peer)
            .map(|p| p.name.trim())
            .filter(|n| !n.is_empty())
        {
            Some(n) => n.to_string(),
            None => format!("device {:04x}", peer & 0xffff),
        }
    }
}

/// "1 shape", "3 strokes, 2 texts".
fn count_kinds(kinds: &[String]) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for k in kinds {
        match counts.iter_mut().find(|(n, _)| *n == k.as_str()) {
            Some(c) => c.1 += 1,
            None => counts.push((k.as_str(), 1)),
        }
    }
    counts
        .iter()
        .map(|(k, n)| {
            if *n == 1 {
                format!("1 {k}")
            } else if k.ends_with('s') || k.ends_with('x') {
                format!("{n} {k}es")
            } else {
                format!("{n} {k}s")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A time (ms since the epoch) in local time: "Oct 8, 2026, 1:32 PM".
pub fn when(ms: u64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_millis_opt(ms as i64).single() {
        Some(t) => t.format("%b %-d, %Y, %-I:%M %p").to_string(),
        None => "unknown time".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_counted() {
        let k = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(count_kinds(&k(&["shape"])), "1 shape");
        assert_eq!(
            count_kinds(&k(&["stroke", "stroke", "text"])),
            "2 strokes, 1 text"
        );
        assert_eq!(
            count_kinds(&k(&["audio clip", "audio clip"])),
            "2 audio clips"
        );
    }

    #[test]
    fn a_new_shape_was_made_by_you_and_never_changed() {
        let mut app = App::new(None);
        let r = app.run_commands(&serde_json::json!([{"add": "shape", "kind": "rect", "x": 0, "y": 0, "w": 50, "h": 30}]));
        assert_eq!(r[0]["ok"], true);
        let g = (app.objs.groups.len() - 1) as u32;
        let rows = app.sel_details(&[ObjRef::Group(g)]);
        let get = |k: &str| {
            rows.iter()
                .find(|(l, _)| l == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        assert_eq!(get("What"), "1 shape");
        assert!(get("Made").ends_with("· you"), "{rows:?}");
        assert_eq!(get("Changed"), "never");
        assert_eq!(get("Shape"), "Rectangle");
    }
}
