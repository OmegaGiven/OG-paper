// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! The page a page server shows in a browser (its address on port 8991):
//! how to connect, the link for the address the visitor used, and the
//! server's pages. It shows the view-only server link; the full link (make,
//! rename and delete pages) only when the page is opened with it
//! (`/?k=<server key>`), so reaching the server never grants editing.
//! `OGP_WEB=off` turns the page off; `OGP_SERVER_NAME` names the server.

/// What the page needs to know about the server.
pub struct Info {
    pub name: String,
    pub edit_key: String,
    pub view_key: String,
    /// The public wss:// address, if the server was given one.
    pub public: Option<String>,
    /// Page names and when they last changed (ms since the Unix epoch).
    pub pages: Vec<(String, u64)>,
}

/// Where the web app lives (it adds a server from `#server=<link>`).
const WEB_APP: &str = "https://omegagiven.github.io/OG-paper/app/";
/// The App Store listing (iPhone, iPad and Mac).
const APP_STORE: &str = "https://apps.apple.com/app/id6818948230";
/// The TestFlight beta, while the App Store version is in review.
const TESTFLIGHT: &str = "https://testflight.apple.com/join/8XNBfTZ9";
/// Builds for Windows, Linux, Android and the Mac (outside the App Store).
const RELEASES: &str = "https://github.com/OmegaGiven/OG-paper/releases";

/// The ways to get OG Paper: platform, how, and links (label, url).
const CLIENTS: &[(&str, &str, &[(&str, &str)])] = &[
    ("Any browser", "Nothing to install: works on phones, tablets and computers, and can be added to the home screen.",
     &[("Open the web app", WEB_APP)]),
    ("iPhone and iPad", "The App Store app (or the TestFlight beta).",
     &[("App Store", APP_STORE), ("TestFlight beta", TESTFLIGHT)]),
    ("Mac", "The Mac App Store, or the download (og-paper-macos-universal.zip).",
     &[("Mac App Store", APP_STORE), ("Download", RELEASES)]),
    ("Windows", "Download og-paper-windows-x86_64.zip, unzip it and run og-paper.exe.",
     &[("Download", RELEASES)]),
    ("Linux", "Download og-paper-linux-x86_64.tar.gz, unpack it and run og-paper.",
     &[("Download", RELEASES)]),
    ("Android", "Download og-paper-android-arm64.apk on the phone and open it to install.",
     &[("Download", RELEASES)]),
];
const ICON: &str = include_str!("../../../packaging/icon.svg");

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn query_key(target: &str) -> Option<&str> {
    target
        .split_once('?')?
        .1
        .split('&')
        .find_map(|kv| kv.strip_prefix("k="))
}

/// Answer `target` (path and query) for a visitor who reached the server at
/// `base` (`ws://host:port`, or `wss://` behind a TLS proxy).
pub fn page(target: &str, base: &str, info: &Info) -> (u16, &'static str, Vec<u8>) {
    let path = target.split('?').next().unwrap_or("/");
    match path {
        "/icon.svg" | "/favicon.ico" => (200, "image/svg+xml", ICON.as_bytes().to_vec()),
        "/health" => (200, "text/plain; charset=utf-8", b"ok\n".to_vec()),
        "/" | "/index.html" => (
            200,
            "text/html; charset=utf-8",
            html(target, base, info).into_bytes(),
        ),
        _ => (404, "text/plain; charset=utf-8", b"not found\n".to_vec()),
    }
}

fn link_row(label: &str, note: &str, link: &str, open: bool) -> String {
    let web = if open && link.starts_with("wss://") {
        format!(
            r#"<a class="btn primary" href="{WEB_APP}#server={}" target="_blank" rel="noopener">Open in the web app</a>"#,
            esc(&url_encode(link))
        )
    } else {
        String::new()
    };
    format!(
        r#"<div class="link"><div class="label">{label}<small>{note}</small></div>
<div class="row"><code>{l}</code><button class="btn" data-copy="{l}">Copy</button>{web}</div></div>"#,
        l = esc(link)
    )
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn html(target: &str, base: &str, info: &Info) -> String {
    let admin = query_key(target).is_some_and(|k| k == info.edit_key);
    let base = base.trim_end_matches('/');
    let mut links = String::new();
    let mut bases: Vec<String> = Vec::new();
    if let Some(p) = &info.public {
        bases.push(p.trim_end_matches('/').to_string());
    }
    if !base.ends_with("://") && !bases.iter().any(|b| b == base) {
        bases.push(base.to_string());
    }
    for b in &bases {
        let via = if b.starts_with("wss://") {
            "Secure (works from the web app and anywhere)"
        } else {
            "On this network (the desktop and mobile apps; the web app needs a wss:// address)"
        };
        links += &link_row(
            &format!("View link · {}", esc(b)),
            via,
            &format!("{b}/?k={}", info.view_key),
            true,
        );
        if admin {
            links += &link_row(
                "Full link (make, rename and delete pages)",
                "Only for people you trust: it opens and changes every page.",
                &format!("{b}/?k={}", info.edit_key),
                true,
            );
        }
    }
    let mut pages = String::new();
    for (name, t) in &info.pages {
        pages += &format!(
            r#"<li><span>{}</span><time data-t="{t}"></time></li>"#,
            esc(name)
        );
    }
    if pages.is_empty() {
        pages = "<li><span>No pages yet</span></li>".into();
    }
    let mut clients = String::new();
    for (name, how, links) in CLIENTS {
        let buttons: String = links
            .iter()
            .enumerate()
            .map(|(i, (label, url))| {
                format!(
                    r#"<a class="btn{}" href="{url}" target="_blank" rel="noopener">{label}</a>"#,
                    if i == 0 { " primary" } else { "" }
                )
            })
            .collect();
        clients += &format!(
            r#"<div class="client"><b>{name}</b><small>{how}</small><div class="row">{buttons}</div></div>"#
        );
    }
    let access = if admin {
        r#"<p class="note ok">You opened this page with the server key, so the full links show. Don't share this page's address.</p>"#
    } else {
        r#"<p class="note">The view link lets people open and follow the pages. To make and change pages, ask the server's owner for the full link.</p>"#
    };
    format!(
        r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>{name} · OG Paper server</title>
<link rel="icon" href="/icon.svg">
<style>
:root {{ --bg:#101216; --card:#191c22; --ink:#eef0e8; --muted:#a0a3ab; --edge:#2b2f37; --pink:#e2457a; }}
* {{ box-sizing:border-box; }}
body {{ margin:0; background:var(--bg); color:var(--ink); font:16px/1.55 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif; }}
main {{ max-width:860px; margin:0 auto; padding:32px 18px 60px; }}
header {{ display:flex; align-items:center; gap:16px; margin-bottom:26px; }}
header svg {{ width:64px; height:64px; flex:none; }}
h1 {{ margin:0; font-size:1.7rem; line-height:1.2; }}
header p {{ margin:2px 0 0; color:var(--muted); }}
h2 {{ font-size:1.05rem; margin:0 0 12px; letter-spacing:.2px; }}
.card {{ background:var(--card); border:1px solid var(--edge); border-radius:16px; padding:18px 20px; margin:16px 0; }}
.link {{ padding:10px 0; border-top:1px solid var(--edge); }}
.link:first-of-type {{ border-top:0; padding-top:0; }}
.label {{ font-weight:650; }} .label small {{ display:block; font-weight:400; color:var(--muted); font-size:.85rem; }}
.row {{ display:flex; flex-wrap:wrap; gap:8px; align-items:center; margin-top:8px; }}
code {{ flex:1 1 320px; min-width:0; overflow-wrap:anywhere; background:#0c0e11; border:1px solid var(--edge); border-radius:10px; padding:9px 11px; font-size:.86rem; }}
.btn {{ border:1px solid var(--edge); background:#22262e; color:var(--ink); border-radius:10px; padding:9px 14px; font:inherit; font-weight:600; cursor:pointer; text-decoration:none; white-space:nowrap; }}
.btn:hover {{ border-color:#454b56; }} .btn.primary {{ background:var(--pink); border-color:var(--pink); color:#fff; }}
ol {{ margin:0; padding-left:22px; }} ol li {{ margin:6px 0; }}
ul.pages {{ list-style:none; margin:0; padding:0; }} ul.pages li {{ display:flex; justify-content:space-between; gap:12px; padding:7px 0; border-top:1px solid var(--edge); }}
ul.pages li:first-child {{ border-top:0; }} time {{ color:var(--muted); font-size:.88rem; }}
.clients {{ display:grid; grid-template-columns:repeat(auto-fill,minmax(240px,1fr)); gap:12px; }}
.client {{ border:1px solid var(--edge); border-radius:12px; padding:12px 14px; background:#15181d; }}
.client small {{ display:block; color:var(--muted); font-size:.85rem; margin:2px 0 4px; }}
.note {{ color:var(--muted); font-size:.9rem; margin:12px 0 0; }} .note.ok {{ color:#9fd8a8; }}
a {{ color:var(--ink); }} footer {{ color:var(--muted); font-size:.85rem; margin-top:26px; }}
</style></head><body><main>
<header>{icon}<div><h1>{name}</h1><p>An OG Paper server: shared pages anyone with the link can draw on together.</p></div></header>
<section class="card"><h2>Connect</h2>{links}{access}</section>
<section class="card"><h2>Get OG Paper</h2><div class="clients">{clients}</div></section>
<section class="card"><h2>How to join</h2><ol>
<li>Get OG Paper for your device (above). The web app needs nothing installed.</li>
<li>Copy a link above. In OG Paper open <b>Settings ⚙ › Pages</b>, paste it under <b>Servers</b> and press <b>Add</b>.</li>
<li>Open a page. You keep a copy on your device: draw offline, and it syncs when you're back.</li>
</ol></section>
<section class="card"><h2>Pages on this server</h2><ul class="pages">{pages}</ul></section>
<footer>OG Paper · free and open source · <a href="https://github.com/OmegaGiven/OG-paper" target="_blank" rel="noopener">github.com/OmegaGiven/OG-paper</a></footer>
</main>
<script>
document.querySelectorAll('[data-copy]').forEach(b => b.onclick = async () => {{
  try {{ await navigator.clipboard.writeText(b.dataset.copy); }} catch (e) {{
    const t = document.createElement('textarea'); t.value = b.dataset.copy; document.body.append(t); t.select(); document.execCommand('copy'); t.remove(); }}
  const was = b.textContent; b.textContent = 'Copied'; setTimeout(() => b.textContent = was, 1400);
}});
const ago = t => {{ if (!t) return ''; const s = (Date.now() - t) / 1000;
  return s < 60 ? 'just now' : s < 3600 ? Math.floor(s / 60) + ' min ago' : s < 86400 ? Math.floor(s / 3600) + ' h ago' : Math.floor(s / 86400) + ' days ago'; }};
document.querySelectorAll('time[data-t]').forEach(e => e.textContent = ago(+e.dataset.t));
</script></body></html>"##,
        name = esc(&info.name),
        icon = ICON.trim(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> Info {
        Info {
            name: "Test <server>".into(),
            edit_key: "eSECRET".into(),
            view_key: "vVIEW".into(),
            public: Some("wss://paper.example.ts.net:8992".into()),
            pages: vec![("First page".into(), 0)],
        }
    }

    #[test]
    fn the_page_shows_the_view_link_but_never_the_full_one_without_the_key() {
        let (status, _, body) = page("/", "ws://10.0.0.5:8991", &info());
        let html = String::from_utf8(body).unwrap();
        assert_eq!(status, 200);
        assert!(html.contains("ws://10.0.0.5:8991/?k=vVIEW"));
        assert!(html.contains("wss://paper.example.ts.net:8992/?k=vVIEW"));
        assert!(html.contains("#server=wss%3A%2F%2Fpaper.example.ts.net%3A8992%2F%3Fk%3DvVIEW"));
        assert!(!html.contains("eSECRET"));
        assert!(html.contains("Test &lt;server&gt;"));
        // With the key, the full link shows too.
        let (_, _, body) = page("/?k=eSECRET", "ws://10.0.0.5:8991", &info());
        assert!(String::from_utf8(body)
            .unwrap()
            .contains("ws://10.0.0.5:8991/?k=eSECRET"));
        // A wrong key is no key.
        let (_, _, body) = page("/?k=eWRONG", "ws://10.0.0.5:8991", &info());
        assert!(!String::from_utf8(body).unwrap().contains("eSECRET"));
        assert_eq!(page("/nope", "ws://x", &info()).0, 404);
        // Every client has a way in.
        let (_, _, body) = page("/", "ws://10.0.0.5:8991", &info());
        let html = String::from_utf8(body).unwrap();
        for want in [
            WEB_APP,
            APP_STORE,
            RELEASES,
            "Windows",
            "Linux",
            "Android",
            "iPhone and iPad",
        ] {
            assert!(html.contains(want), "{want}");
        }
    }
}
