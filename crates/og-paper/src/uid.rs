// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! UUIDv7 object ids: 48-bit millisecond timestamp + 74 random bits, so ids
//! sort by creation time and never collide across devices (for sync).

use std::sync::atomic::{AtomicU64, Ordering};

static STATE: AtomicU64 = AtomicU64::new(0);

fn random64() -> u64 {
    // splitmix64 over a counter seeded from the clock: plenty for ids.
    let mut s = STATE.load(Ordering::Relaxed);
    if s == 0 {
        s = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
            ^ (&STATE as *const _ as u64);
    }
    s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
    STATE.store(s, Ordering::Relaxed);
    let mut z = s;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

pub fn new() -> u128 {
    let ms = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u128)
        .unwrap_or(0)
        & ((1 << 48) - 1);
    let rand = ((random64() as u128) << 64 | random64() as u128) & ((1u128 << 74) - 1);
    let rand_a = rand >> 62; // 12 bits
    let rand_b = rand & ((1u128 << 62) - 1); // 62 bits
    ms << 80 | 0x7 << 76 | rand_a << 64 | 0b10 << 62 | rand_b
}

#[cfg(test)]
mod tests {
    #[test]
    fn v7_layout_and_unique() {
        let a = super::new();
        let b = super::new();
        assert_ne!(a, b);
        assert_eq!((a >> 76) & 0xF, 7, "version");
        assert_eq!((a >> 62) & 0b11, 0b10, "variant");
    }
}
