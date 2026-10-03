// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! End-to-end protection for live sharing: whoever carries the frames (a
//! relay, a proxy, the network) can neither read them nor forge edits.
//!
//! A share link carries a key. An edit key (`e` + 32 bytes, base64url) is a
//! secret from which both a frame key and an Ed25519 signing key derive;
//! a view key (`v` + the frame key + the signing key's public half) holds
//! only what reading and checking need. Every frame is sealed with
//! ChaCha20-Poly1305 under the frame key (a 12-byte nonce in front); a
//! frame that changes the canvas also carries a signature by the signing
//! key, which every receiver checks, so a view key cannot make edits that
//! anyone accepts.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

/// The keys one share link gives.
#[derive(Clone)]
pub struct Keys {
    frame: [u8; 32],
    sign: Option<SigningKey>,
    verify: VerifyingKey,
}

fn derive(label: &str, secret: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"og-paper ");
    h.update(label.as_bytes());
    h.update(secret);
    h.finalize().into()
}

fn b64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..=c.len() {
            s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    s
}

fn unb64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32)
    };
    let s = s.trim().trim_end_matches('=').as_bytes();
    let mut out = Vec::new();
    for c in s.chunks(4) {
        let mut n = 0u32;
        for (i, &ch) in c.iter().enumerate() {
            n |= val(ch)? << (18 - 6 * i);
        }
        for i in 0..c.len().saturating_sub(1) {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

/// 32 random bytes for a new edit key (the OS on desktop; the web page
/// passes its own from crypto.getRandomValues).
#[cfg(not(target_arch = "wasm32"))]
pub fn random_secret() -> [u8; 32] {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("system randomness");
    b
}

impl Keys {
    /// The keys of an edit secret.
    pub fn from_secret(secret: &[u8; 32]) -> Keys {
        let sign = SigningKey::from_bytes(&derive("sign", secret));
        Keys {
            frame: derive("frame", secret),
            verify: sign.verifying_key(),
            sign: Some(sign),
        }
    }

    /// Read a link key (`e…` or `v…`).
    pub fn parse(token: &str) -> Option<Keys> {
        let token = token.trim();
        let (kind, rest) = token.split_at_checked(1)?;
        let b = unb64(rest)?;
        match kind {
            "e" => Some(Keys::from_secret(&b.try_into().ok()?)),
            "v" if b.len() == 64 => Some(Keys {
                frame: b[..32].try_into().ok()?,
                sign: None,
                verify: VerifyingKey::from_bytes(&b[32..].try_into().ok()?).ok()?,
            }),
            _ => None,
        }
    }

    /// The edit key for a secret, as a link carries it.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn edit_token(secret: &[u8; 32]) -> String {
        format!("e{}", b64(secret))
    }

    /// The view key: reads and checks, cannot sign.
    pub fn view_token(&self) -> String {
        let mut b = self.frame.to_vec();
        b.extend_from_slice(self.verify.as_bytes());
        format!("v{}", b64(&b))
    }

    pub fn can_edit(&self) -> bool {
        self.sign.is_some()
    }

    /// A name for the room these keys open, that says nothing about them
    /// (what a relay files frames under).
    pub fn room(&self) -> [u8; 32] {
        derive("room", &self.frame)
    }

    /// Seal a frame: nonce, then the ciphertext. `signed` frames (changes
    /// to the canvas) carry a signature inside; without a signing key they
    /// go unsigned and receivers drop them.
    pub fn seal(&self, msg: &[u8], signed: bool) -> Vec<u8> {
        let mut plain = Vec::with_capacity(msg.len() + 65);
        match (&self.sign, signed) {
            (Some(k), true) => {
                plain.push(1);
                plain.extend_from_slice(&k.sign(msg).to_bytes());
            }
            _ => plain.push(0),
        }
        plain.extend_from_slice(msg);
        let mut nonce = [0u8; 12];
        let a = crate::uid::new();
        nonce.copy_from_slice(&a.to_le_bytes()[..12]);
        let c = ChaCha20Poly1305::new(Key::from_slice(&self.frame));
        let ct = c
            .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
            .expect("encrypt");
        let mut out = nonce.to_vec();
        out.extend_from_slice(&ct);
        out
    }

    /// Open a sealed frame: the message and whether its signature checked
    /// out (None if it is not ours, or was changed on the way).
    pub fn open(&self, sealed: &[u8]) -> Option<(Vec<u8>, bool)> {
        if sealed.len() < 12 + 16 + 1 {
            return None;
        }
        let c = ChaCha20Poly1305::new(Key::from_slice(&self.frame));
        let plain = c
            .decrypt(Nonce::from_slice(&sealed[..12]), &sealed[12..])
            .ok()?;
        match plain.first()? {
            1 if plain.len() >= 65 => {
                let sig = Signature::from_bytes(plain[1..65].try_into().ok()?);
                let msg = plain[65..].to_vec();
                let ok = self.verify.verify(&msg, &sig).is_ok();
                Some((msg, ok))
            }
            0 => Some((plain[1..].to_vec(), false)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_and_view_keys_seal_open_and_sign() {
        let secret = random_secret();
        let edit = Keys::parse(&Keys::edit_token(&secret)).unwrap();
        let view = Keys::parse(&edit.view_token()).unwrap();
        assert!(edit.can_edit() && !view.can_edit());
        assert_eq!(edit.room(), view.room());
        // An edit frame from the editor: readable and verified by a viewer.
        let f = edit.seal(b"changes", true);
        assert_eq!(view.open(&f), Some((b"changes".to_vec(), true)));
        // A viewer cannot sign: its "edit" arrives unverified.
        let g = view.seal(b"forged", true);
        assert_eq!(edit.open(&g), Some((b"forged".to_vec(), false)));
        // Someone else's key, or a changed byte: nothing.
        let other = Keys::from_secret(&random_secret());
        assert_eq!(other.open(&f), None);
        let mut bad = f.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert_eq!(edit.open(&bad), None);
        // base64url round trips at every length.
        for n in 0..10 {
            let b: Vec<u8> = (0..n as u8).collect();
            assert_eq!(unb64(&b64(&b)).unwrap(), b);
        }
        assert!(Keys::parse("x123").is_none() && Keys::parse("").is_none());
    }
}
