// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! The page server's Model Context Protocol endpoint, `POST /mcp`, so AI
//! assistants (Claude and others) can list, read, look at, draw on and
//! edit pages, and help people with them.
//!
//! Streamable HTTP without streams: every request is one JSON-RPC message
//! and gets one JSON answer (no sessions, no server-sent events). The
//! server key goes in the address (`/mcp?k=<key>`, for clients that only
//! take a URL) or as `Authorization: Bearer <key>`; the view key gives the
//! read-only tools. Tools turn into the automation API's own requests (see
//! `hub::api` and `script`), handed to the server's main thread, where the
//! pages are.

use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::net::native::{WebReq, WebResp};

/// An API request for the main thread, and where its answer goes.
pub(crate) struct Job {
    pub msg: Value,
    pub can_edit: bool,
    pub reply: Sender<Value>,
}

/// Protocol versions this speaks, newest first.
const VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];

/// How long a tool waits for the main thread (big pages load slowly).
const WAIT: Duration = Duration::from_secs(60);

/// What an assistant is told when it connects.
const INSTRUCTIONS: &str = "\
OG Paper is an open-source endless canvas: infinite pan and infinite zoom. \
A server holds pages; people draw on them together live, and anything you \
add or change appears for everyone on the page at once (and each change \
can be undone in the app).

Coordinates are points at the page's home view, measured from its centre: \
x to the right, y down. The home view shows roughly x -600..600, \
y -400..400 on a laptop screen. Because zoom is endless, things can be far \
smaller or bigger than that: read_page gives `bounds`, the box around \
everything, so look there first.

Working well:
1. list_pages, then read_page (what is there, with ids and boxes) and \
view_page (a picture) before changing anything.
2. Place new things in empty space unless asked otherwise; keep to a grid \
and leave room between things (24-48 points). Text: titles 36-64, headings \
24-32, body 16-20. Use a small palette (2-4 colours) and neat shapes.
3. add_to_page takes many items in one call (one undo step): build a whole \
layout at once. It answers with each new thing's id.
4. edit_page moves, resizes, restyles, reorders and deletes by id. Editing \
a thing gives it a new id; the answer lists them.
5. view_page again to check your work, and fix what looks off: overlaps, \
text running out of boxes, uneven spacing.

A text's box grows with its words (in the default hand-drawn font about \
0.8 x size per character wide and 1.6 x size per line tall); add_to_page \
answers with each new thing's box, so check that text fits. Put text inside \
a shape by making the shape first, then the text over it with padding. Lines and arrows run from \
(x, y) to (x + w, y + h), so w and h may be negative.";

/// Answer a request to `/mcp`.
pub(crate) fn handle(req: &WebReq, keys: (&str, &str), jobs: &Sender<Job>, name: &str) -> WebResp {
    let Some(can_edit) = key_of(req).and_then(|k| {
        if k == keys.0 {
            Some(true)
        } else if k == keys.1 {
            Some(false)
        } else {
            None
        }
    }) else {
        return resp(
            401,
            json!({"error": "add the server key: /mcp?k=<key>, or Authorization: Bearer <key>"}),
        );
    };
    if req.method != "POST" {
        let mut r = resp(405, json!({"error": "POST JSON-RPC messages here"}));
        r.headers.push(("Allow".into(), "POST".into()));
        return r;
    }
    let Ok(msg) = serde_json::from_slice::<Value>(&req.body) else {
        return resp(
            400,
            rpc_error(Value::Null, -32700, "send one JSON-RPC message"),
        );
    };
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        // An answer to us, or a batch (gone since 2025-06-18).
        return if msg.is_array() {
            resp(400, rpc_error(Value::Null, -32600, "batches are not taken"))
        } else {
            accepted()
        };
    };
    let Some(id) = msg.get("id").cloned() else {
        return accepted(); // A notification.
    };
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    let out = match method {
        "initialize" => Ok(initialize(&params, name)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools(can_edit) })),
        "tools/call" => call_tool(&params, can_edit, jobs),
        "prompts/list" => Ok(json!({ "prompts": prompts() })),
        "prompts/get" => get_prompt(&params),
        _ => Err((-32601, format!("no method {method}"))),
    };
    resp(
        200,
        match out {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, m)) => rpc_error(id, code, &m),
        },
    )
}

/// The key from `?k=` or `Authorization: Bearer`.
fn key_of(req: &WebReq) -> Option<String> {
    if let Some(b) = req
        .header("authorization")
        .and_then(|a| a.strip_prefix("Bearer ").or(a.strip_prefix("bearer ")))
    {
        return Some(b.trim().to_string());
    }
    let (_, query) = req.target.split_once('?')?;
    query
        .split('&')
        .find_map(|kv| kv.strip_prefix("k="))
        .map(percent_decode)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = || u8::from_str_radix(std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?, 16).ok();
        match (b[i], hex()) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                i += 1;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn resp(status: u16, body: Value) -> WebResp {
    WebResp {
        status,
        ctype: "application/json",
        headers: vec![("Cache-Control".into(), "no-store".into())],
        body: body.to_string().into_bytes(),
    }
}

fn accepted() -> WebResp {
    WebResp {
        status: 202,
        ctype: "application/json",
        headers: vec![],
        body: vec![],
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn initialize(params: &Value, name: &str) -> Value {
    let asked = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("");
    let version = VERSIONS
        .iter()
        .find(|v| **v == asked)
        .unwrap_or(&VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": false}, "prompts": {"listChanged": false}},
        "serverInfo": {"name": "og-paper", "title": name, "version": env!("CARGO_PKG_VERSION")},
        "instructions": INSTRUCTIONS,
    })
}

// ---- tools -------------------------------------------------------------

/// The page argument every page tool takes.
fn page_arg() -> Value {
    json!({"type": "string", "description": "The page: its id (from list_pages) or its exact name"})
}

fn region_args(m: &mut Map<String, Value>, what: &str) {
    for (k, d) in [("x", "left"), ("y", "top"), ("w", "width"), ("h", "height")] {
        m.insert(
            k.into(),
            json!({"type": "number", "description": format!("{what}: {d} (points); give all four")}),
        );
    }
}

fn tools(can_edit: bool) -> Vec<Value> {
    let read_only = json!({"readOnlyHint": true, "openWorldHint": false});
    let mut read = Map::new();
    read.insert("page".into(), page_arg());
    region_args(&mut read, "Only things touching this region");
    read.insert(
        "kinds".into(),
        json!({"type": "array", "items": {"type": "string",
               "enum": ["text", "shape", "table", "ink", "fill", "image", "audio", "portal"]},
               "description": "Only these kinds"}),
    );
    read.insert(
        "limit".into(),
        json!({"type": "integer", "description": "Most things to list (default 500, at most 5000)"}),
    );
    let mut view = Map::new();
    view.insert("page".into(), page_arg());
    region_args(&mut view, "The part to show (default: everything)");
    view.insert(
        "ids".into(),
        json!({"type": "array", "items": {"type": "string"}, "description": "Show only these things"}),
    );
    view.insert(
        "px".into(),
        json!({"type": "integer", "description": "Longer side of the picture in pixels (default 1024, at most 2048)"}),
    );
    let mut list = vec![
        json!({
            "name": "list_pages",
            "title": "List pages",
            "description": "The pages on this server: id, name, and when each last changed (ms since 1970).",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": read_only,
        }),
        json!({
            "name": "read_page",
            "title": "Read a page",
            "description": "What is on a page, bottom to top: each thing's id, kind, box (x, y, w, h in points) \
                and details (text, colours, sizes, shape kind, table cells, …), plus `bounds`, the box around \
                everything, and `total`.",
            "inputSchema": {"type": "object", "properties": read, "required": ["page"]},
            "annotations": read_only,
        }),
        json!({
            "name": "view_page",
            "title": "Look at a page",
            "description": "A picture (PNG) of a page: all of it, a region, or some things. Use it to see a \
                layout and to check your changes.",
            "inputSchema": {"type": "object", "properties": view, "required": ["page"]},
            "annotations": read_only,
        }),
        json!({
            "name": "list_fonts",
            "title": "List fonts",
            "description": "The fonts texts can use on this server.",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": read_only,
        }),
    ];
    if !can_edit {
        return list;
    }
    list.extend([
        json!({
            "name": "add_to_page",
            "title": "Add to a page",
            "description": "Add things to a page, all in one undo step; answers with each new thing's id and box. Items:\n\
                {\"add\": \"text\", \"x\", \"y\" (top left), \"text\", \"size\" (cap height, default 24), \"color\", \"font\", \"align\": left|center|right, \"letter_spacing\" (extra room per letter, in letter heights), \"line_height\" (times normal), \"path\": [[x, y], …] and \"smooth\" (the words run along it)}\n\
                {\"add\": \"shape\", \"kind\": rect|ellipse|diamond|triangle|star|polygon|line|arrow|path, \"x\", \"y\", \"w\", \"h\", \
                \"color\" (outline), \"fill\" (#rrggbb or none), \"width\" (outline, default 3), \"dash\": solid|dashed|dotted, \
                \"round\" (rounded corners), \"opacity\" (0-1), \"gradient\": {\"to\": #rrggbb, \"angle\" (degrees, 90 = down), \"radial\"} (fill from \"fill\" to \"to\"), \"neat\" (false: hand-drawn look). Lines and arrows run from x,y to x+w,y+h.\n\
                {\"add\": \"shape\", \"kind\": \"path\", \"points\": [[x, y], …], \"smooth\" (curves through the points), \"closed\", plus the shape options above} (a pen path)\n\
                {\"add\": \"table\", \"x\", \"y\", \"cells\": [[\"Name\", \"Qty\"], [\"Pens\", \"4\"]], \"size\" (default 18), \"color\"}\n\
                {\"add\": \"stroke\", \"points\": [[x, y], …], \"width\", \"color\", \"dash\"} (freehand ink)\n\
                Colours are #rrggbb or #rrggbbaa.",
            "inputSchema": {"type": "object", "properties": {
                "page": page_arg(),
                "items": {"type": "array", "items": {"type": "object", "properties": {
                    "add": {"type": "string", "enum": ["text", "shape", "table", "stroke"]}}, "required": ["add"]},
                    "description": "The things to add"},
            }, "required": ["page", "items"]},
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "openWorldHint": false},
        }),
        json!({
            "name": "edit_page",
            "title": "Edit a page",
            "description": "Change things on a page by id (from read_page or add_to_page). Edits, done in order:\n\
                {\"move\": [ids], \"dx\", \"dy\"}\n\
                {\"set\": id or [ids], then any of: \"x\", \"y\" (new top left), \"w\", \"h\" (new size), \"rotate\" (degrees), \
                \"text\", \"cells\", \"size\", \"letter_spacing\", \"line_height\", \"name\" (a frame's), \"brightness\" (-1 to 1), \"contrast\", \"saturation\" (0-3, 1 as it is; pictures), \"color\", \"fill\" (#rrggbb or none), \"width\", \"font\", \"align\", \"dash\", \
                \"round\", \"opacity\", \"kind\" (shape kind), \"gradient\" (as in add)}\n\
                {\"order\": [ids], \"to\": front|back}\n\
                {\"combine\": [ids], \"op\": union|subtract|intersect|exclude} (closed shapes into one path; subtract cuts the upper from the lowest)\n\
                {\"group\": [ids]} / {\"ungroup\": [ids]} (a group moves and changes as one)\n\
                {\"component\": [ids]} (make them a component's main; its copies, made in the app, follow it)\n\
                {\"remove_background\": [ids]} (pictures: their plain background made see-through)\n\
                {\"delete\": [ids]}\n\
                Editing a thing gives it a new id: each answer lists the new ids in order.",
            "inputSchema": {"type": "object", "properties": {
                "page": page_arg(),
                "edits": {"type": "array", "items": {"type": "object"}, "description": "The edits"},
            }, "required": ["page", "edits"]},
            "annotations": {"readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": false},
        }),
        json!({
            "name": "create_page",
            "title": "Make a page",
            "description": "Make a new, empty page; answers with its id.",
            "inputSchema": {"type": "object", "properties": {
                "name": {"type": "string", "description": "The page's name"}}, "required": ["name"]},
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "openWorldHint": false},
        }),
        json!({
            "name": "rename_page",
            "title": "Rename a page",
            "description": "Give a page a new name.",
            "inputSchema": {"type": "object", "properties": {
                "page": page_arg(), "name": {"type": "string", "description": "The new name"}},
                "required": ["page", "name"]},
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
        }),
        json!({
            "name": "delete_page",
            "title": "Delete a page",
            "description": "Take a page off the server (its file is kept as <id>.ogp.deleted, so an admin can bring it back). \
                Only when the person asked for it.",
            "inputSchema": {"type": "object", "properties": {"page": page_arg()}, "required": ["page"]},
            "annotations": {"readOnlyHint": false, "destructiveHint": true, "openWorldHint": false},
        }),
    ]);
    list
}

type ToolResult = Result<Value, (i64, String)>;

fn call_tool(params: &Value, can_edit: bool, jobs: &Sender<Job>) -> ToolResult {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let known = tools(true)
        .iter()
        .any(|t| t.get("name").and_then(Value::as_str) == Some(name));
    if !known {
        return Err((-32602, format!("no tool {name}")));
    }
    let api = Api { jobs, can_edit };
    let out = match name {
        "list_pages" => api.call(json!({"cmd": "pages"})).map(|r| text(&r["pages"])),
        "read_page" => api.page(&args).and_then(|page| {
            let mut c = pick(&args, &["x", "y", "w", "h", "kinds", "limit"]);
            c.insert("get".into(), "objects".into());
            api.run1(&page, Value::Object(c)).map(|mut r| {
                strip_ok(&mut r);
                text(&r)
            })
        }),
        "view_page" => api.page(&args).and_then(|page| {
            let mut c = pick(&args, &["x", "y", "w", "h", "ids", "px"]);
            c.insert("get".into(), "picture".into());
            api.run1(&page, Value::Object(c)).map(|r| {
                let frame = &r["frame"];
                json!({"content": [
                    {"type": "image", "data": r["png"], "mimeType": "image/png"},
                    {"type": "text", "text": format!(
                        "Shows x {} to {}, y {} to {} ({} x {} px).",
                        frame["x"],
                        tidy_sum(&frame["x"], &frame["w"]),
                        frame["y"],
                        tidy_sum(&frame["y"], &frame["h"]),
                        r["width"],
                        r["height"])},
                ]})
            })
        }),
        "list_fonts" => api
            .call(json!({"cmd": "pages"}))
            .and_then(|r| {
                let page = r["pages"][0]["page"]
                    .as_str()
                    .ok_or("the server has no pages")?
                    .to_string();
                api.run1(&page, json!({"get": "fonts"}))
            })
            .map(|r| text(&r["fonts"])),
        "add_to_page" | "edit_page" => api.page(&args).and_then(|page| {
            let (key, ok): (&str, fn(&Value) -> bool) = if name == "add_to_page" {
                ("items", |c| c.get("add").is_some())
            } else {
                ("edits", |c| {
                    ["move", "set", "order", "delete"]
                        .iter()
                        .any(|k| c.get(*k).is_some())
                })
            };
            let list = args
                .get(key)
                .and_then(Value::as_array)
                .filter(|l| !l.is_empty())
                .ok_or(format!("give \"{key}\": a list"))?;
            if let Some(bad) = list.iter().find(|c| !ok(c)) {
                return Err(format!(
                    "{bad} is not one of this tool's {key} (see its description)"
                ));
            }
            let r = api.call(json!({"cmd": "run", "page": page, "commands": list}))?;
            let results = r["results"].as_array().cloned().unwrap_or_default();
            let failed = results.iter().all(|r| r["ok"] == json!(false));
            let mut out = text(&Value::Array(results));
            out["isError"] = failed.into();
            Ok(out)
        }),
        "create_page" => api
            .call(json!({"cmd": "new_page", "name": args["name"]}))
            .map(|r| text(&json!({"page": r["page"]}))),
        "rename_page" => api.page(&args).and_then(|page| {
            api.call(json!({"cmd": "rename_page", "page": page, "name": args["name"]}))
                .map(|_| text(&json!("Renamed.")))
        }),
        "delete_page" => api.page(&args).and_then(|page| {
            api.call(json!({"cmd": "delete_page", "page": page}))
                .map(|_| text(&json!("Deleted.")))
        }),
        _ => unreachable!("checked above"),
    };
    Ok(out.unwrap_or_else(|e| json!({"content": [{"type": "text", "text": e}], "isError": true})))
}

/// The automation API, from the web thread.
struct Api<'a> {
    jobs: &'a Sender<Job>,
    can_edit: bool,
}

impl Api<'_> {
    /// One API request; its answer, or its error.
    fn call(&self, msg: Value) -> Result<Value, String> {
        let (tx, rx) = mpsc::channel();
        self.jobs
            .send(Job {
                msg,
                can_edit: self.can_edit,
                reply: tx,
            })
            .map_err(|_| "the server is stopping".to_string())?;
        let r = rx
            .recv_timeout(WAIT)
            .map_err(|_| "the server took too long".to_string())?;
        if r["ok"] == json!(false) {
            return Err(r["error"].as_str().unwrap_or("that failed").to_string());
        }
        Ok(r)
    }

    /// Run one command on a page; its result.
    fn run1(&self, page: &str, command: Value) -> Result<Value, String> {
        let r = self.call(json!({"cmd": "run", "page": page, "commands": [command]}))?;
        let first = r["results"][0].clone();
        if first["ok"] == json!(false) {
            return Err(first["error"].as_str().unwrap_or("that failed").to_string());
        }
        Ok(first)
    }

    /// The page an argument names: an id, or a page's name.
    fn page(&self, args: &Value) -> Result<String, String> {
        let p = args["page"]
            .as_str()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .ok_or("say which page")?;
        if p.len() == 32 && p.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(p.to_ascii_lowercase());
        }
        let pages = self.call(json!({"cmd": "pages"}))?;
        let all = pages["pages"].as_array().cloned().unwrap_or_default();
        let named: Vec<&Value> = all
            .iter()
            .filter(|q| {
                q["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(p))
            })
            .collect();
        match named[..] {
            [one] => Ok(one["page"].as_str().unwrap_or_default().to_string()),
            [] => Err(format!("no page called {p} (list_pages lists them)")),
            _ => Err(format!("more than one page is called {p}: use its id")),
        }
    }
}

/// The arguments named in `keys` that were given.
fn pick(args: &Value, keys: &[&str]) -> Map<String, Value> {
    keys.iter()
        .filter_map(|k| Some((k.to_string(), args.get(*k)?.clone())))
        .collect()
}

fn strip_ok(v: &mut Value) {
    if let Some(o) = v.as_object_mut() {
        o.remove("ok");
    }
}

fn tidy_sum(a: &Value, b: &Value) -> String {
    let s = a.as_f64().unwrap_or(0.0) + b.as_f64().unwrap_or(0.0);
    format!("{}", (s * 100.0).round() / 100.0)
}

/// A tool result holding `v` as JSON text (a plain string as itself).
fn text(v: &Value) -> Value {
    let t = match v {
        Value::String(s) => s.clone(),
        v => v.to_string(),
    };
    json!({"content": [{"type": "text", "text": t}]})
}

// ---- prompts -----------------------------------------------------------

fn prompts() -> Vec<Value> {
    vec![
        json!({
            "name": "design",
            "title": "Design on a page",
            "description": "Make a design (a diagram, a poster, a plan, a wireframe, …) on a page from a short brief.",
            "arguments": [
                {"name": "page", "description": "The page (id or name)", "required": true},
                {"name": "brief", "description": "What to make", "required": true},
            ],
        }),
        json!({
            "name": "review",
            "title": "Review a page",
            "description": "Look over a page, explain what is on it and suggest improvements.",
            "arguments": [{"name": "page", "description": "The page (id or name)", "required": true}],
        }),
    ]
}

fn get_prompt(params: &Value) -> ToolResult {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arg = |k: &str| {
        params["arguments"][k]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    let (description, text) = match name {
        "design" => (
            "Design on a page",
            format!(
                "On the OG Paper page \"{}\", make this: {}\n\n\
                First read_page and view_page to see what is there and where the free space is. \
                Plan the layout on a grid before adding anything, then build it with add_to_page in as few \
                calls as you can. view_page to check it, fix overlaps, crowding and uneven spacing with \
                edit_page, and finish with a short summary of what you made and where it is.",
                arg("page"),
                arg("brief")
            ),
        ),
        "review" => (
            "Review a page",
            format!(
                "Look over the OG Paper page \"{}\" with read_page and view_page. Explain what is on it \
                and how it is organised, then suggest a few concrete improvements (layout, clarity, \
                consistency, missing pieces). Ask before changing anything.",
                arg("page")
            ),
        ),
        _ => return Err((-32602, format!("no prompt {name}"))),
    };
    Ok(json!({
        "description": description,
        "messages": [{"role": "user", "content": {"type": "text", "text": text}}],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, target: &str, auth: Option<&str>, body: Value) -> WebReq {
        let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
        if let Some(a) = auth {
            headers.push(("authorization".into(), a.into()));
        }
        WebReq {
            method: method.into(),
            target: target.into(),
            base: String::new(),
            headers,
            body: body.to_string().into_bytes(),
        }
    }

    /// A main thread that answers every request with `answer(msg)`.
    fn server(answer: fn(&Value) -> Value) -> Sender<Job> {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::spawn(move || {
            for job in rx {
                let _ = job.reply.send(answer(&job.msg));
            }
        });
        tx
    }

    fn body(r: &WebResp) -> Value {
        serde_json::from_slice(&r.body).unwrap()
    }

    #[test]
    fn keys_decide_who_gets_in_and_what_they_may_do() {
        let jobs = server(|_| json!({"ok": true}));
        let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
        let names = |r: &WebResp| -> Vec<String> {
            body(r)["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string())
                .collect()
        };
        let no = handle(
            &req("POST", "/mcp", None, list.clone()),
            ("e1", "v1"),
            &jobs,
            "S",
        );
        assert_eq!(no.status, 401);
        let wrong = handle(
            &req("POST", "/mcp?k=nope", None, list.clone()),
            ("e1", "v1"),
            &jobs,
            "S",
        );
        assert_eq!(wrong.status, 401);
        let edit = handle(
            &req("POST", "/mcp?k=e1", None, list.clone()),
            ("e1", "v1"),
            &jobs,
            "S",
        );
        assert!(names(&edit).contains(&"edit_page".to_string()));
        let view = handle(
            &req("POST", "/mcp", Some("Bearer v1"), list),
            ("e1", "v1"),
            &jobs,
            "S",
        );
        let v = names(&view);
        assert!(v.contains(&"view_page".to_string()) && !v.contains(&"add_to_page".to_string()));
        let get = handle(
            &req("GET", "/mcp?k=e1", None, json!({})),
            ("e1", "v1"),
            &jobs,
            "S",
        );
        assert_eq!(get.status, 405);
    }

    #[test]
    fn it_speaks_the_protocol() {
        let jobs = server(|_| json!({"ok": true}));
        let call = |m: Value| {
            handle(
                &req("POST", "/mcp?k=e%31", None, m),
                ("e1", "v1"),
                &jobs,
                "Home",
            )
        };
        let init = body(&call(
            json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}}}),
        ));
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(init["result"]["serverInfo"]["title"], "Home");
        let odd = body(&call(
            json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "1999-01-01"}}),
        ));
        assert_eq!(odd["result"]["protocolVersion"], VERSIONS[0]);
        assert_eq!(
            call(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).status,
            202
        );
        let missing = body(&call(json!({"jsonrpc": "2.0", "id": 2, "method": "nope"})));
        assert_eq!(missing["error"]["code"], -32601);
        let prompt = body(&call(
            json!({"jsonrpc": "2.0", "id": 3, "method": "prompts/get",
            "params": {"name": "design", "arguments": {"page": "Wall", "brief": "a kanban board"}}}),
        ));
        let t = prompt["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap();
        assert!(t.contains("Wall") && t.contains("kanban"));
    }

    #[test]
    fn tools_become_api_requests() {
        // Pages "Wall" and "Plan"; runs echo their commands back.
        let jobs = server(|m| match m["cmd"].as_str() {
            Some("pages") => json!({"ok": true, "pages": [
                {"page": "0000000000000000000000000000000a", "name": "Wall"},
                {"page": "0000000000000000000000000000000b", "name": "Plan"}]}),
            Some("run") => json!({"ok": true, "results": m["commands"].as_array().unwrap().iter()
                .map(|c| json!({"ok": true, "page": m["page"], "echo": c})).collect::<Vec<_>>()}),
            _ => json!({"ok": false, "error": "?"}),
        });
        let call = |name: &str, args: Value| {
            let m = json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call",
                           "params": {"name": name, "arguments": args}});
            body(&handle(
                &req("POST", "/mcp?k=e1", None, m),
                ("e1", "v1"),
                &jobs,
                "S",
            ))["result"]
                .clone()
        };
        // A page by name; only the arguments the command takes.
        let r = call(
            "read_page",
            json!({"page": "plan", "kinds": ["text"], "junk": 1}),
        );
        let out: Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(out["page"], "0000000000000000000000000000000b");
        assert_eq!(out["echo"], json!({"get": "objects", "kinds": ["text"]}));
        let r = call("read_page", json!({"page": "Nowhere"}));
        assert_eq!(r["isError"], true);
        // add_to_page takes adds only; edit_page edits only.
        let r = call(
            "add_to_page",
            json!({"page": "Wall", "items": [{"delete": ["1"]}]}),
        );
        assert_eq!(r["isError"], true);
        let r = call(
            "edit_page",
            json!({"page": "Wall", "edits": [{"move": ["1"], "dx": 5}]}),
        );
        assert_eq!(r["isError"], false);
    }
}
