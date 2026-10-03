// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Merging copies of a canvas that changed apart (offline, on other devices).
//!
//! Items (strokes, objects) are never edited in place: an edit hides the old
//! item and adds a new one. So a canvas is a set of items plus, for each, a
//! log of "now shown" / "now hidden" events, and two copies merge by taking
//! the union of both logs: an item shows if its latest event says so. Every
//! event carries a hybrid logical clock stamp ([`Hlc`]: wall time, a counter,
//! the peer), which orders events the same way on every device whatever their
//! clocks say, so the result does not depend on who merges what, in what
//! order, or how often.
//!
//! An item may say which item it replaced (a moved stroke replaces the one it
//! was moved from). When copies made different edits of the same item, those
//! are rival branches: the branch with the newest edit anywhere in it wins
//! and the others are hidden ([`Log::conflicts`]), never shown doubled. They
//! stay in the log, so the timeline still has them.

use std::collections::{BTreeMap, HashMap, HashSet};

/// A hybrid logical clock stamp. Ordered by wall time, then counter, then
/// peer, so any two stamps compare the same everywhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hlc {
    /// Milliseconds since the Unix epoch (never behind any stamp seen).
    pub ms: u64,
    /// Orders stamps within one millisecond.
    pub n: u32,
    /// The device that made it.
    pub peer: u64,
}

/// Makes stamps for one device.
#[derive(Clone, Debug)]
pub struct Clock {
    peer: u64,
    last: Hlc,
}

impl Clock {
    pub fn new(peer: u64) -> Self {
        Self {
            peer,
            last: Hlc { ms: 0, n: 0, peer },
        }
    }

    pub fn peer(&self) -> u64 {
        self.peer
    }

    /// A new stamp, later than every stamp made or seen here.
    pub fn now(&mut self, wall_ms: u64) -> Hlc {
        let (ms, n) = if wall_ms > self.last.ms {
            (wall_ms, 0)
        } else {
            (self.last.ms, self.last.n + 1)
        };
        self.last = Hlc {
            ms,
            n,
            peer: self.peer,
        };
        self.last
    }

    /// Note a stamp from elsewhere, so later stamps here come after it.
    pub fn observe(&mut self, h: Hlc) {
        if (h.ms, h.n) > (self.last.ms, self.last.n) {
            self.last.ms = h.ms;
            self.last.n = h.n;
        }
    }
}

/// Item `item` became shown (`alive`) or hidden at `at`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Event {
    pub item: u128,
    pub alive: bool,
    pub at: Hlc,
}

/// An item that replaced another, in the edit stamped `edit` (all items
/// made by one edit share it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Replace {
    pub item: u128,
    pub replaces: u128,
    pub edit: Hlc,
}

/// The latest stamp seen from each peer: what a copy already has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Version(pub BTreeMap<u64, Hlc>);

impl Version {
    /// As text: `peer:ms.n` per peer (hex peer), comma separated.
    pub fn to_text(&self) -> String {
        self.0
            .iter()
            .map(|(p, h)| format!("{p:x}:{}.{}", h.ms, h.n))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Read [`Version::to_text`] (bad parts are skipped).
    pub fn parse(s: &str) -> Version {
        let mut v = Version::default();
        for part in s.split(',') {
            let Some((p, rest)) = part.split_once(':') else {
                continue;
            };
            let Some((ms, n)) = rest.split_once('.') else {
                continue;
            };
            if let (Ok(peer), Ok(ms), Ok(n)) = (
                u64::from_str_radix(p, 16),
                ms.parse::<u64>(),
                n.parse::<u32>(),
            ) {
                v.0.insert(peer, Hlc { ms, n, peer });
            }
        }
        v
    }

    pub fn has(&self, h: &Hlc) -> bool {
        self.0.get(&h.peer).is_some_and(|m| h <= m)
    }

    fn note(&mut self, h: Hlc) {
        let e = self.0.entry(h.peer).or_insert(h);
        if h > *e {
            *e = h;
        }
    }
}

/// What one merge brought in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Merged {
    /// New events, by the peer that made them.
    pub events_from: BTreeMap<u64, usize>,
    /// Items whose shown / hidden state changed.
    pub changed: Vec<u128>,
}

/// Every event and replacement a copy knows of.
#[derive(Clone, Debug, Default)]
pub struct Log {
    events: BTreeMap<(Hlc, u128), bool>,
    replaces: HashMap<u128, Replace>,
    version: Version,
}

impl Log {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn version(&self) -> &Version {
        &self.version
    }

    /// All events, oldest first.
    pub fn events(&self) -> impl Iterator<Item = Event> + '_ {
        self.events
            .iter()
            .map(|(&(at, item), &alive)| Event { item, alive, at })
    }

    pub fn replacements(&self) -> impl Iterator<Item = &Replace> {
        self.replaces.values()
    }

    /// Record an event. False if it was already known.
    pub fn add(&mut self, e: Event) -> bool {
        self.version.note(e.at);
        self.events.insert((e.at, e.item), e.alive).is_none()
    }

    pub fn add_replace(&mut self, r: Replace) {
        self.replaces.entry(r.item).or_insert(r);
    }

    /// The events a copy at `v` lacks, oldest first.
    pub fn missing(&self, v: &Version) -> Vec<Event> {
        self.events().filter(|e| !v.has(&e.at)).collect()
    }

    /// Shown or hidden, by each item's latest event (before conflicts).
    pub fn state(&self) -> HashMap<u128, bool> {
        let mut s = HashMap::new();
        for e in self.events() {
            s.insert(e.item, e.alive);
        }
        s
    }

    /// Items hidden because a rival edit of the same item won.
    pub fn conflicts(&self) -> HashSet<u128> {
        // Edits of each item, grouped into branches by edit stamp.
        let mut kids: HashMap<u128, Vec<&Replace>> = HashMap::new();
        for r in self.replaces.values() {
            kids.entry(r.replaces).or_default().push(r);
        }
        // Newest edit anywhere at or below an item.
        let mut memo: HashMap<u128, Hlc> = HashMap::new();
        fn newest(
            item: u128,
            own: Hlc,
            kids: &HashMap<u128, Vec<&Replace>>,
            memo: &mut HashMap<u128, Hlc>,
            depth: usize,
        ) -> Hlc {
            if let Some(&h) = memo.get(&item) {
                return h;
            }
            let mut best = own;
            if depth < 10_000 {
                for k in kids.get(&item).map(|v| v.as_slice()).unwrap_or(&[]) {
                    best = best.max(newest(k.item, k.edit, kids, memo, depth + 1));
                }
            }
            memo.insert(item, best);
            best
        }
        let mut hidden = HashSet::new();
        let mut parents: Vec<&u128> = kids.keys().collect();
        parents.sort();
        for p in parents {
            let mut branches: BTreeMap<Hlc, (Hlc, Vec<u128>)> = BTreeMap::new();
            for r in &kids[p] {
                let score = newest(r.item, r.edit, &kids, &mut memo, 0);
                let b = branches.entry(r.edit).or_insert((score, Vec::new()));
                b.0 = b.0.max(score);
                b.1.push(r.item);
            }
            if branches.len() < 2 {
                continue;
            }
            let win = branches
                .iter()
                .max_by_key(|(edit, (score, _))| (*score, **edit))
                .map(|(e, _)| *e);
            for (edit, (_, items)) in &branches {
                if Some(*edit) != win {
                    for &i in items {
                        hide_below(i, &kids, &mut hidden);
                    }
                }
            }
        }
        hidden
    }

    /// Shown items: latest event says shown, and no rival edit won.
    pub fn visible(&self) -> HashSet<u128> {
        let lost = self.conflicts();
        self.state()
            .into_iter()
            .filter(|(i, alive)| *alive && !lost.contains(i))
            .map(|(i, _)| i)
            .collect()
    }

    /// Take in another copy's events and replacements.
    pub fn merge(&mut self, events: &[Event], replaces: &[Replace]) -> Merged {
        let before = self.visible();
        let mut m = Merged::default();
        for e in events {
            if self.add(*e) {
                *m.events_from.entry(e.at.peer).or_default() += 1;
            }
        }
        for r in replaces {
            self.add_replace(*r);
        }
        let after = self.visible();
        let mut changed: Vec<u128> = before.symmetric_difference(&after).copied().collect();
        changed.sort();
        m.changed = changed;
        m
    }
}

fn hide_below(item: u128, kids: &HashMap<u128, Vec<&Replace>>, out: &mut HashSet<u128>) {
    let mut stack = vec![item];
    while let Some(i) = stack.pop() {
        if out.insert(i) {
            if let Some(ks) = kids.get(&i) {
                stack.extend(ks.iter().map(|k| k.item));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(ms: u64, peer: u64) -> Hlc {
        Hlc { ms, n: 0, peer }
    }

    fn ev(item: u128, alive: bool, ms: u64, peer: u64) -> Event {
        Event {
            item,
            alive,
            at: h(ms, peer),
        }
    }

    /// A small deterministic random source for the property tests.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    fn log_of(events: &[Event], replaces: &[Replace]) -> Log {
        let mut l = Log::new();
        l.merge(events, replaces);
        l
    }

    #[test]
    fn versions_round_trip_as_text() {
        let mut v = Version::default();
        v.note(Hlc {
            ms: 12,
            n: 3,
            peer: u64::MAX,
        });
        v.note(Hlc {
            ms: 7,
            n: 0,
            peer: 0,
        });
        assert_eq!(Version::parse(&v.to_text()), v);
        assert_eq!(Version::parse("junk,1:2"), Version::default());
    }

    #[test]
    fn clock_never_goes_back_and_follows_what_it_sees() {
        let mut c = Clock::new(7);
        let a = c.now(1000);
        let b = c.now(900); // the wall clock stepped back
        assert!(b > a && b.ms == 1000 && b.n == 1);
        c.observe(Hlc {
            ms: 5000,
            n: 3,
            peer: 9,
        });
        let d = c.now(1200);
        assert_eq!((d.ms, d.n, d.peer), (5000, 4, 7));
    }

    #[test]
    fn latest_event_decides_and_missing_finds_what_a_copy_lacks() {
        let mut a = Log::new();
        a.add(ev(1, true, 10, 1));
        a.add(ev(1, false, 20, 1));
        a.add(ev(2, true, 15, 1));
        assert_eq!(a.visible(), [2].into_iter().collect());
        let mut b = Log::new();
        b.add(ev(1, true, 10, 1));
        b.add(ev(3, true, 12, 2));
        let need = a.missing(b.version());
        assert_eq!(need.len(), 2); // stroke 1 erased, stroke 2 drawn
        let m = b.merge(&need, &[]);
        assert_eq!(m.events_from.get(&1), Some(&2));
        assert_eq!(b.visible(), [2, 3].into_iter().collect());
    }

    #[test]
    fn rival_edits_keep_the_newest_branch_only() {
        // Stroke 1 moved by two people offline: 2 (peer 1) and 3 (peer 2,
        // later). Then peer 1 moves its copy again, 2 -> 4, latest of all.
        let base = [ev(1, true, 1, 1)];
        let p1 = [ev(1, false, 10, 1), ev(2, true, 10, 1)];
        let p2 = [ev(1, false, 20, 2), ev(3, true, 20, 2)];
        let r12 = Replace {
            item: 2,
            replaces: 1,
            edit: h(10, 1),
        };
        let r13 = Replace {
            item: 3,
            replaces: 1,
            edit: h(20, 2),
        };
        let mut l = log_of(&base, &[]);
        l.merge(&p1, &[r12]);
        l.merge(&p2, &[r13]);
        assert_eq!(l.visible(), [3].into_iter().collect());
        // Peer 1's later edit makes its branch the newest.
        let r24 = Replace {
            item: 4,
            replaces: 2,
            edit: h(30, 1),
        };
        l.merge(&[ev(2, false, 30, 1), ev(4, true, 30, 1)], &[r24]);
        assert_eq!(l.visible(), [4].into_iter().collect());
        assert!(l.conflicts().contains(&3));
    }

    #[test]
    fn one_edit_making_several_items_is_one_branch() {
        let r = |item| Replace {
            item,
            replaces: 1,
            edit: h(10, 1),
        };
        let l = log_of(
            &[
                ev(1, true, 1, 1),
                ev(1, false, 10, 1),
                ev(2, true, 10, 1),
                ev(3, true, 10, 1),
            ],
            &[r(2), r(3)],
        );
        assert_eq!(l.visible(), [2, 3].into_iter().collect());
    }

    /// Random histories from three peers, merged in every order and twice:
    /// always the same shown set.
    #[test]
    fn merging_is_order_free_and_idempotent() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for round in 0..200 {
            let mut copies: Vec<(Vec<Event>, Vec<Replace>)> = vec![Default::default(); 3];
            let mut next_item = 100u128;
            for (p, copy) in copies.iter_mut().enumerate() {
                let peer = p as u64 + 1;
                let mut clock = Clock::new(peer);
                let mut alive: Vec<u128> = vec![1, 2, 3];
                for _ in 0..rng.below(12) {
                    let at = clock.now(rng.below(50));
                    match rng.below(3) {
                        0 => {
                            next_item += 1;
                            copy.0.push(Event {
                                item: next_item,
                                alive: true,
                                at,
                            });
                            alive.push(next_item);
                        }
                        1 if !alive.is_empty() => {
                            let i = alive.remove(rng.below(alive.len() as u64) as usize);
                            copy.0.push(Event {
                                item: i,
                                alive: false,
                                at,
                            });
                        }
                        _ if !alive.is_empty() => {
                            // A move: hide one, add its replacement.
                            let k = rng.below(alive.len() as u64) as usize;
                            let old = alive.remove(k);
                            next_item += 1;
                            copy.0.push(Event {
                                item: old,
                                alive: false,
                                at,
                            });
                            copy.0.push(Event {
                                item: next_item,
                                alive: true,
                                at,
                            });
                            copy.1.push(Replace {
                                item: next_item,
                                replaces: old,
                                edit: at,
                            });
                            alive.push(next_item);
                        }
                        _ => {}
                    }
                }
            }
            let base: Vec<Event> = (1..=3).map(|i| ev(i, true, 0, 9)).collect();
            let orders = [
                [0, 1, 2],
                [0, 2, 1],
                [1, 0, 2],
                [1, 2, 0],
                [2, 0, 1],
                [2, 1, 0],
            ];
            let mut results = Vec::new();
            for o in orders {
                let mut l = log_of(&base, &[]);
                for &i in &o {
                    l.merge(&copies[i].0, &copies[i].1);
                }
                // Merging again changes nothing.
                let once = l.visible();
                for &i in &o {
                    let m = l.merge(&copies[i].0, &copies[i].1);
                    assert!(m.events_from.is_empty() && m.changed.is_empty());
                }
                assert_eq!(l.visible(), once);
                let mut v: Vec<u128> = once.into_iter().collect();
                v.sort();
                results.push(v);
            }
            assert!(
                results.windows(2).all(|w| w[0] == w[1]),
                "round {round}: {results:?}"
            );
            // Pairwise sync through `missing` reaches the same place.
            let mut a = log_of(&base, &[]);
            a.merge(&copies[0].0, &copies[0].1);
            let mut b = log_of(&base, &[]);
            b.merge(&copies[1].0, &copies[1].1);
            b.merge(&copies[2].0, &copies[2].1);
            let to_a = b.missing(a.version());
            let to_b = a.missing(b.version());
            let ra: Vec<Replace> = b.replacements().copied().collect();
            let rb: Vec<Replace> = a.replacements().copied().collect();
            a.merge(&to_a, &ra);
            b.merge(&to_b, &rb);
            assert_eq!(a.visible(), b.visible());
            let mut v: Vec<u128> = a.visible().into_iter().collect();
            v.sort();
            assert_eq!(v, results[0]);
        }
    }
}
