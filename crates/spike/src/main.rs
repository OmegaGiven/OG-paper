// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

fn main() {
    let mut opts = og_spike::Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--bench" => opts.bench = true,
            "--mass" => opts.mass = args.next().and_then(|v| v.parse().ok()).expect("--mass N"),
            "--start-chain" => opts.start_chain = args.next().and_then(|v| v.parse().ok()),
            "--start-mass" => opts.start_mass = args.next().and_then(|v| v.parse().ok()),
            "--depth" => opts.depth = args.next().and_then(|v| v.parse().ok()).expect("--depth N"),
            _ => {
                eprintln!("usage: og-spike [--bench] [--mass N] [--depth N] [--start-chain K] [--start-mass LEVEL]");
                std::process::exit(2);
            }
        }
    }
    og_spike::run_desktop(opts);
}
