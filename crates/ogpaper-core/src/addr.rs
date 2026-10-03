// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Exact cell addresses in the infinite quadtree.
//!
//! A cell at level `L` has side `2^-L` world units. Level can be any integer,
//! so the world is unbounded both in extent and in zoom depth. Cell indices
//! are arbitrary-precision integers, so there is no global precision limit:
//! only *differences* between nearby cells are ever converted to floats.

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, ToPrimitive, Zero};

pub type Level = i64;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct CellAddr {
    pub level: Level,
    pub x: BigInt,
    pub y: BigInt,
}

impl CellAddr {
    pub fn new(level: Level, x: impl Into<BigInt>, y: impl Into<BigInt>) -> Self {
        Self {
            level,
            x: x.into(),
            y: y.into(),
        }
    }

    pub fn parent(&self) -> CellAddr {
        let two = BigInt::from(2);
        CellAddr {
            level: self.level - 1,
            x: self.x.div_floor(&two),
            y: self.y.div_floor(&two),
        }
    }

    /// Child by quadrant: bit 0 = right half, bit 1 = bottom half.
    pub fn child(&self, q: u8) -> CellAddr {
        CellAddr {
            level: self.level + 1,
            x: (&self.x << 1u32) + (q & 1) as i32,
            y: (&self.y << 1u32) + (q >> 1) as i32,
        }
    }

    /// Which quadrant of its parent this cell occupies.
    pub fn quadrant(&self) -> u8 {
        let two = BigInt::from(2);
        let qx = if self.x.mod_floor(&two).is_zero() {
            0
        } else {
            1
        };
        let qy = if self.y.mod_floor(&two).is_zero() {
            0
        } else {
            2
        };
        qx | qy
    }

    /// The ancestor at `level` (must be <= self.level).
    pub fn ancestor(&self, level: Level) -> CellAddr {
        let d = self.level - level;
        assert!(d >= 0, "ancestor level must be coarser");
        if d == 0 {
            return self.clone();
        }
        let div = BigInt::one() << d as u64;
        CellAddr {
            level,
            x: self.x.div_floor(&div),
            y: self.y.div_floor(&div),
        }
    }

    /// Neighbor at the same level.
    pub fn offset(&self, dx: i64, dy: i64) -> CellAddr {
        CellAddr {
            level: self.level,
            x: &self.x + dx,
            y: &self.y + dy,
        }
    }

    /// Origin of this cell relative to the origin of `reference`, measured in
    /// units of the reference cell's side. Exact integer math first; only the
    /// (small, for anything near the reference) result becomes a float.
    pub fn origin_in(&self, reference: &CellAddr) -> [f64; 2] {
        let d = self.level - reference.level;
        if d <= 0 {
            // self is coarser (or equal): scale self up to reference level.
            let s = (-d) as u64;
            let dx = (&self.x << s) - &reference.x;
            let dy = (&self.y << s) - &reference.y;
            [big_to_f64(&dx), big_to_f64(&dy)]
        } else {
            // self is finer: scale reference up, then shrink the difference.
            let s = d as u64;
            let dx = &self.x - (&reference.x << s);
            let dy = &self.y - (&reference.y << s);
            [scaled(&dx, -d), scaled(&dy, -d)]
        }
    }

    /// Side of this cell in units of `reference`'s side: 2^(reference.level - level).
    pub fn side_in(&self, reference: &CellAddr) -> f64 {
        pow2(reference.level - self.level)
    }

    /// This cell moved by (dx, dy) cells of level `level`: the cell of the
    /// same level it lands in, and what is left over in its own cell units
    /// (each in [0, 1)). Exact (nothing left over) when this cell is at
    /// `level` or finer, however deep.
    pub fn shifted(&self, dx: &BigInt, dy: &BigInt, level: Level) -> (CellAddr, [f64; 2]) {
        let d = self.level - level;
        if d >= 0 {
            let s = d as u64;
            let c = CellAddr {
                level: self.level,
                x: &self.x + (dx << s),
                y: &self.y + (dy << s),
            };
            (c, [0.0, 0.0])
        } else {
            let s = (-d) as u64;
            // Whole cells (rounded down) and the rest.
            let split = |v: &BigInt| {
                let q = v >> s;
                let r = v - (&q << s);
                (q, scaled(&r, d))
            };
            let (qx, fx) = split(dx);
            let (qy, fy) = split(dy);
            let c = CellAddr {
                level: self.level,
                x: &self.x + qx,
                y: &self.y + qy,
            };
            (c, [fx, fy])
        }
    }
}

/// 2^e as f64 (saturates to 0 / inf far outside the f64 range).
pub fn pow2(e: i64) -> f64 {
    if e > 1023 {
        f64::INFINITY
    } else if e < -1074 {
        0.0
    } else {
        2f64.powi(e as i32)
    }
}

fn big_to_f64(v: &BigInt) -> f64 {
    v.to_f64()
        .unwrap_or(if v.sign() == num_bigint::Sign::Minus {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        })
}

/// v * 2^e without intermediate overflow/underflow for large |v| and |e|.
fn scaled(v: &BigInt, e: i64) -> f64 {
    if v.is_zero() {
        return 0.0;
    }
    let bits = v.bits() as i64;
    if bits > 900 {
        // Drop low bits first so the f64 conversion stays finite.
        let drop = bits - 900;
        let t = v >> drop as u64;
        return big_to_f64(&t) * pow2(e + drop);
    }
    big_to_f64(v) * pow2(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifting_is_exact_when_finer_and_splits_when_coarser() {
        // 3 + 1/4 cells of level 2, right; -1/2, up.
        let (dx, dy) = (BigInt::from(13), BigInt::from(-2));
        // A deep cell (level 40) moves by a whole number of its own cells.
        let deep = CellAddr::new(40, 5, 5);
        let (c, f) = deep.shifted(&dx, &dy, 4);
        assert_eq!(f, [0.0, 0.0]);
        assert_eq!(c.x, BigInt::from(5) + (BigInt::from(13) << 36u64));
        assert_eq!(c.y, BigInt::from(5) - (BigInt::from(2) << 36u64));
        // A level-2 cell: 13/4 = 3 + 1/4, and -2/4 = -1 + 1/2.
        let coarse = CellAddr::new(2, 0, 0);
        let (c, f) = coarse.shifted(&dx, &dy, 4);
        assert_eq!(
            (c.x.clone(), c.y.clone()),
            (BigInt::from(3), BigInt::from(-1))
        );
        assert_eq!(f, [0.25, 0.5]);
        // Either way the cell's origin lands in the same place.
        let o = c.origin_in(&CellAddr::new(4, 0, 0));
        assert_eq!([o[0] + f[0] * 4.0, o[1] + f[1] * 4.0], [13.0, -2.0]);
    }

    #[test]
    fn parent_child_roundtrip_negative() {
        let c = CellAddr::new(5, -7, 3);
        for q in 0..4 {
            let ch = c.child(q);
            assert_eq!(ch.parent(), c);
            assert_eq!(ch.quadrant(), q);
        }
        assert_eq!(CellAddr::new(1, -1, -1).parent(), CellAddr::new(0, -1, -1));
    }

    #[test]
    fn origin_is_exact_at_huge_depth() {
        // A camera cell 400 levels deep with a gigantic index.
        let big: BigInt = (BigInt::one() << 399u32) + 12345;
        let cam = CellAddr::new(400, big.clone(), big.clone());
        // Its neighbour 3 cells right and a grandchild inside it.
        let n = cam.offset(3, -2);
        assert_eq!(n.origin_in(&cam), [3.0, -2.0]);
        let g = cam.child(3).child(1);
        assert_eq!(g.origin_in(&cam), [0.75, 0.5]);
        // An ancestor 10 levels up: origin is minus our position within it.
        let a = cam.ancestor(390);
        let o = a.origin_in(&cam);
        assert_eq!(o, [-((12345 % 1024) as f64), -((12345 % 1024) as f64)]);
    }
}
