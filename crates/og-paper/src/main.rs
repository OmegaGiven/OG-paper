// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn main() {}

/// `og-paper [canvas.ogp]` — opens (or creates) the given canvas.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn main() {
    og_paper::run_desktop(std::env::args_os().nth(1).map(std::path::PathBuf::from));
}
