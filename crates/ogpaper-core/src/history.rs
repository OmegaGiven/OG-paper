// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Undo/redo. Every edit is recorded as a change to a set of strokes; undo
//! applies the inverse. Nothing is ever freed, so undo works across any number
//! of steps (and deleted strokes remain as tombstones for files and sync).

use crate::scene::Scene;

#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Added(Vec<u32>),
    Deleted(Vec<u32>),
}

#[derive(Default)]
pub struct History {
    undo: Vec<Change>,
    redo: Vec<Change>,
}

impl History {
    /// Record an edit that has already been applied to the scene.
    pub fn record(&mut self, change: Change) {
        self.undo.push(change);
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undo the last edit. Returns the stroke ids whose state changed.
    pub fn undo(&mut self, scene: &mut Scene) -> Option<Vec<u32>> {
        let c = self.undo.pop()?;
        let ids = apply(scene, &c, true);
        self.redo.push(c);
        Some(ids)
    }

    /// Redo the last undone edit. Returns the stroke ids whose state changed.
    pub fn redo(&mut self, scene: &mut Scene) -> Option<Vec<u32>> {
        let c = self.redo.pop()?;
        let ids = apply(scene, &c, false);
        self.undo.push(c);
        Some(ids)
    }
}

fn apply(scene: &mut Scene, c: &Change, inverse: bool) -> Vec<u32> {
    let (ids, delete) = match c {
        Change::Added(ids) => (ids, inverse),
        Change::Deleted(ids) => (ids, !inverse),
    };
    ids.iter()
        .copied()
        .filter(|&id| {
            if delete {
                scene.delete(id)
            } else {
                scene.restore(id)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr::CellAddr;

    #[test]
    fn undo_redo_add_and_delete() {
        let mut s = Scene::new();
        let a = s.add_stroke(&CellAddr::new(3, 1, 1), &[[0.1, 0.1], [0.5, 0.5]], 0.01, 0);
        let mut h = History::default();
        h.record(Change::Added(vec![a]));
        s.delete(a);
        h.record(Change::Deleted(vec![a]));
        let root = s.roots[0];
        assert_eq!(s.node(root).subtree, 0);
        h.undo(&mut s); // undo delete
        assert!(!s.strokes[a as usize].deleted);
        h.undo(&mut s); // undo add
        assert!(s.strokes[a as usize].deleted);
        assert_eq!(s.node(root).subtree, 0);
        h.redo(&mut s);
        assert!(!s.strokes[a as usize].deleted);
        assert_eq!(s.node(root).subtree, 1);
        assert!(h.can_redo());
        h.record(Change::Added(vec![]));
        assert!(!h.can_redo(), "a new edit clears redo");
    }
}
