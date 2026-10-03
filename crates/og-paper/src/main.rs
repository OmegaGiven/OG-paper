// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn main() {}

/// `og-paper [canvas.ogp]` — opens (or creates) the given canvas.
/// `og-paper --serve canvas.ogp [--port N]` — hosts it with no window.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "--serve") {
        let Some(path) = args.get(1) else {
            eprintln!("usage: og-paper --serve canvas.ogp [--port N]");
            std::process::exit(2);
        };
        let port = args
            .iter()
            .position(|a| a == "--port")
            .and_then(|i| args.get(i + 1))
            .and_then(|p| p.to_str()?.parse().ok())
            .unwrap_or(8991);
        og_paper::serve(path.into(), port);
        return;
    }
    og_paper::run_desktop(args.into_iter().next().map(std::path::PathBuf::from));
}
