// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! `cfg(desktop)`: Windows, macOS and Linux (file dialogs, the system
//! clipboard, the command-line servers). Not the web, Android or iOS.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(desktop)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if arch != "wasm32" && os != "android" && os != "ios" {
        println!("cargo::rustc-cfg=desktop");
    }
}
