// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn main() {}

#[cfg(target_os = "ios")]
fn main() {
    og_paper::run_ios();
}

/// `og-paper [canvas.ogp]` — opens (or creates) the given canvas.
/// `og-paper --serve canvas.ogp [--port N] [--public wss://host]` — hosts
/// it with no window (`--public`: the address guests reach it by, for links).
/// `og-paper --relay [--port N] [--data DIR]` — a relay for shared canvases.
/// `og-paper --serve-dir DIR [--port N] [--public wss://host]` — a server
/// for many pages (a NAS, a container).
#[cfg(desktop)]
fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let opt = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .and_then(|p| p.to_str().map(String::from))
    };
    if args.first().is_some_and(|a| a == "--serve-dir") {
        let Some(dir) = args.get(1) else {
            eprintln!("usage: og-paper --serve-dir DIR [--port N] [--public wss://host]");
            std::process::exit(2);
        };
        let port = opt("--port").and_then(|p| p.parse().ok()).unwrap_or(8991);
        og_paper::serve_dir(dir.into(), port, opt("--public"));
        return;
    }
    if args.first().is_some_and(|a| a == "--relay") {
        let port = opt("--port").and_then(|p| p.parse().ok()).unwrap_or(8993);
        let dir = opt("--data")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "og-paper-relay".into());
        og_paper::relay(port, dir);
        return;
    }
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
        let public = args
            .iter()
            .position(|a| a == "--public")
            .and_then(|i| args.get(i + 1))
            .and_then(|p| p.to_str().map(String::from));
        og_paper::serve(path.into(), port, public);
        return;
    }
    og_paper::run_desktop(args.into_iter().next().map(std::path::PathBuf::from));
}
