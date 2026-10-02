// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Export what is on screen (or just the selection) as SVG, PDF, PNG or
//! JPEG. Everything is first turned into a small vector list in screen
//! pixels; SVG and PDF are written from that list, and PNG/JPEG are the SVG
//! rasterised (by `resvg` on desktop, by the browser on the web).

use std::fmt::Write as _;

use ogpaper_core::{Brush, Dash};

use crate::objects::ObjRef;
use crate::App;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Svg,
    Pdf,
    Png,
    Jpg,
}

impl Format {
    pub fn from_key(k: &str) -> Option<Self> {
        Some(match k.trim().to_ascii_lowercase().as_str() {
            "svg" => Format::Svg,
            "pdf" => Format::Pdf,
            "png" => Format::Png,
            "jpg" | "jpeg" => Format::Jpg,
            _ => return None,
        })
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn ext(self) -> &'static str {
        match self {
            Format::Svg => "svg",
            Format::Pdf => "pdf",
            Format::Png => "png",
            Format::Jpg => "jpg",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Opts {
    /// Only what is selected (if anything is).
    pub selection: bool,
    /// Paint the paper color behind (always on for JPEG).
    pub background: bool,
    /// Raster pixels per CSS pixel (PNG/JPEG; the page picks it on the web).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub scale: f64,
}

/// Paper color (matches the renderer's clear color).
const PAPER: [u8; 3] = [245, 242, 235];

/// One run of a stroke at one width (px), with the distance along the
/// stroke where it starts (for dash phase).
struct Run {
    pts: Vec<[f64; 2]>,
    width: f64,
    along: f64,
}

enum Item {
    Ink {
        rgb: [u8; 3],
        alpha: f64,
        darken: bool,
        dash: Dash,
        /// Dash unit (px): the stroke's own width.
        unit: f64,
        runs: Vec<Run>,
    },
    Fill {
        rgb: [u8; 3],
        alpha: f64,
        pts: Vec<[f64; 2]>,
    },
    Image {
        id: u64,
        /// Top-left, top-right, bottom-right, bottom-left (px).
        corners: [[f64; 2]; 4],
        opacity: f64,
    },
}

/// The vector list and its frame: origin and size in screen px.
pub struct Page {
    items: Vec<Item>,
    origin: [f64; 2],
    size: [f64; 2],
    /// Screen px per CSS px.
    ppp: f64,
    background: bool,
}

fn rgb_of(c: u32) -> ([u8; 3], f64) {
    let [r, g, b, a] = c.to_le_bytes();
    ([r, g, b], a as f64 / 255.0)
}

fn pressure_factor(brush: Brush, p: f32) -> f64 {
    if brush == Brush::Pen {
        0.25 + 0.75 * p.clamp(0.0, 1.0) as f64
    } else {
        1.0
    }
}

/// Split a stroke into runs whose width stays within a few percent.
fn runs_of(pts: &[[f64; 3]], brush: Brush, width: f64) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut along = 0.0;
    if pts.len() == 1 {
        let w = width * pressure_factor(brush, pts[0][2] as f32);
        return vec![Run {
            pts: vec![[pts[0][0], pts[0][1]], [pts[0][0], pts[0][1]]],
            width: w,
            along: 0.0,
        }];
    }
    for s in pts.windows(2) {
        let (a, b) = (s[0], s[1]);
        let w = width
            * 0.5
            * (pressure_factor(brush, a[2] as f32) + pressure_factor(brush, b[2] as f32));
        let pa = [a[0], a[1]];
        let pb = [b[0], b[1]];
        match runs.last_mut() {
            Some(r) if (r.width - w).abs() <= 0.06 * r.width.max(0.5) => r.pts.push(pb),
            _ => runs.push(Run {
                pts: vec![pa, pb],
                width: w,
                along,
            }),
        }
        along += crate::dist(pa, pb);
    }
    runs
}

impl App {
    /// What to export, in screen px.
    pub(crate) fn export_page(&self, opts: &Opts) -> Result<Page, String> {
        let only: Option<Vec<u32>> =
            (opts.selection && !self.edit.selection.is_empty()).then(|| {
                let mut v: Vec<u32> = self
                    .edit
                    .selection
                    .iter()
                    .flat_map(|r: &ObjRef| self.objs.strokes(r).iter().copied())
                    .collect();
                v.sort_unstable();
                v
            });
        let mut items = Vec::new();
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        let mut grow = |q: [f64; 2], r: f64| {
            lo = [lo[0].min(q[0] - r), lo[1].min(q[1] - r)];
            hi = [hi[0].max(q[0] + r), hi[1].max(q[1] + r)];
        };
        let mut insts: Vec<_> = self.draw.strokes.iter().collect();
        insts.sort_by(|a, b| {
            let (za, zb) = (
                self.scene.strokes[a.stroke as usize].z,
                self.scene.strokes[b.stroke as usize].z,
            );
            za.total_cmp(&zb).then(a.stroke.cmp(&b.stroke))
        });
        for inst in insts {
            let id = inst.stroke;
            let s = &self.scene.strokes[id as usize];
            if s.deleted || only.as_ref().is_some_and(|o| o.binary_search(&id).is_err()) {
                continue;
            }
            let pts: Vec<[f64; 3]> = self
                .scene
                .stroke_points(id)
                .iter()
                .map(|p| {
                    [
                        inst.ox as f64 + p[0] as f64 * inst.scale as f64,
                        inst.oy as f64 + p[1] as f64 * inst.scale as f64,
                        p[2] as f64,
                    ]
                })
                .collect();
            if pts.is_empty() {
                continue;
            }
            let (rgb, alpha) = rgb_of(s.color);
            if s.brush == Brush::Fill {
                let corners =
                    || -> [[f64; 2]; 4] { std::array::from_fn(|k| [pts[k][0], pts[k][1]]) };
                if s.color == 0 {
                    // A picture's corners, if it places one.
                    if let Some(&(pic, op)) = self.objs.image_of.get(&id) {
                        if pts.len() == 4 {
                            let c = corners();
                            c.iter().for_each(|&q| grow(q, 0.0));
                            items.push(Item::Image {
                                id: pic,
                                corners: c,
                                opacity: op as f64 / 255.0,
                            });
                        }
                    }
                    continue;
                }
                pts.iter().for_each(|q| grow([q[0], q[1]], 0.0));
                items.push(Item::Fill {
                    rgb,
                    alpha,
                    pts: pts.iter().map(|q| [q[0], q[1]]).collect(),
                });
                continue;
            }
            let width = (s.width * inst.scale) as f64;
            if width < 0.05 {
                continue;
            }
            pts.iter().for_each(|q| grow([q[0], q[1]], width * 0.5));
            let hl = s.brush == Brush::Highlighter;
            let rgb = if hl {
                // As the shader: the ink mixed 55% into white, darken blend.
                rgb.map(|c| (255.0 + (c as f64 - 255.0) * 0.55).round() as u8)
            } else {
                rgb
            };
            items.push(Item::Ink {
                rgb,
                alpha: if hl { 1.0 } else { alpha },
                darken: hl,
                dash: if hl { Dash::Solid } else { s.dash },
                unit: width.max(1.5),
                runs: runs_of(&pts, s.brush, width),
            });
        }
        let [w, h] = self.size();
        let (origin, size) = if only.is_some() {
            if lo[0] > hi[0] {
                return Err("The selection has nothing on screen to export".into());
            }
            let pad = 12.0 * self.ppp();
            (
                [lo[0] - pad, lo[1] - pad],
                [hi[0] - lo[0] + 2.0 * pad, hi[1] - lo[1] + 2.0 * pad],
            )
        } else {
            ([0.0, 0.0], [w, h])
        };
        Ok(Page {
            items,
            origin,
            size,
            ppp: self.ppp(),
            background: opts.background,
        })
    }

    /// The exported file's bytes.
    pub(crate) fn export(&self, format: Format, opts: &Opts) -> Result<Vec<u8>, String> {
        let page = self.export_page(opts)?;
        match format {
            Format::Svg => Ok(page.svg(&self.objs.images).into_bytes()),
            Format::Pdf => page.pdf(&self.objs.images),
            #[cfg(not(target_arch = "wasm32"))]
            Format::Png | Format::Jpg => page.raster(&self.objs.images, format, opts.scale),
            #[cfg(target_arch = "wasm32")]
            Format::Png | Format::Jpg => {
                // The page shell rasterises the SVG with the browser.
                Ok(page
                    .svg_bg(&self.objs.images, page.background || format == Format::Jpg)
                    .into_bytes())
            }
        }
    }
}

type Images = std::collections::HashMap<u64, crate::images::Asset>;

fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

fn mime(bytes: &[u8]) -> &'static str {
    match image::guess_format(bytes) {
        Ok(image::ImageFormat::Jpeg) => "image/jpeg",
        Ok(image::ImageFormat::Gif) => "image/gif",
        Ok(image::ImageFormat::WebP) => "image/webp",
        _ => "image/png",
    }
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() {
                out.push(T[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A number for SVG/PDF: short, no exponent.
fn num(v: f64) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

impl Page {
    /// Screen px to output units (`k` per CSS px).
    fn map(&self, q: [f64; 2], k: f64) -> [f64; 2] {
        [
            (q[0] - self.origin[0]) / self.ppp * k,
            (q[1] - self.origin[1]) / self.ppp * k,
        ]
    }

    fn out_size(&self, k: f64) -> [f64; 2] {
        [self.size[0] / self.ppp * k, self.size[1] / self.ppp * k]
    }

    pub fn svg(&self, images: &Images) -> String {
        self.svg_bg(images, self.background)
    }

    fn svg_bg(&self, images: &Images, background: bool) -> String {
        let k = 1.0;
        let [w, h] = self.out_size(k);
        let mut s = String::new();
        let _ = write!(
            s,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
            num(w),
            num(h),
            num(w),
            num(h)
        );
        let _ = writeln!(s, "<!-- Made with OG Paper -->");
        if background {
            let _ = writeln!(
                s,
                "<rect width=\"100%\" height=\"100%\" fill=\"{}\"/>",
                hex(PAPER)
            );
        }
        for it in &self.items {
            match it {
                Item::Ink {
                    rgb,
                    alpha,
                    darken,
                    dash,
                    unit,
                    runs,
                } => {
                    let u = unit / self.ppp * k;
                    let mut g = String::from(
                        "<g fill=\"none\" stroke-linecap=\"round\" stroke-linejoin=\"round\"",
                    );
                    let _ = write!(g, " stroke=\"{}\"", hex(*rgb));
                    if *alpha < 0.999 {
                        let _ = write!(g, " opacity=\"{}\"", num(*alpha));
                    }
                    if *darken {
                        g.push_str(" style=\"mix-blend-mode:darken\"");
                    }
                    match dash {
                        Dash::Solid => {}
                        Dash::Dashed => {
                            let _ = write!(
                                g,
                                " stroke-dasharray=\"{} {}\"",
                                num(3.0 * u),
                                num(3.0 * u)
                            );
                        }
                        Dash::Dotted => {
                            let _ = write!(g, " stroke-dasharray=\"0 {}\"", num(2.4 * u));
                        }
                    }
                    s.push_str(&g);
                    s.push('>');
                    for r in runs {
                        s.push_str("<polyline points=\"");
                        for (i, &q) in r.pts.iter().enumerate() {
                            let p = self.map(q, k);
                            if i > 0 {
                                s.push(' ');
                            }
                            let _ = write!(s, "{},{}", num(p[0]), num(p[1]));
                        }
                        let _ = write!(s, "\" stroke-width=\"{}\"", num(r.width / self.ppp * k));
                        if *dash != Dash::Solid && r.along > 0.0 {
                            let _ = write!(
                                s,
                                " stroke-dashoffset=\"{}\"",
                                num(-r.along / self.ppp * k)
                            );
                        }
                        s.push_str("/>");
                    }
                    s.push_str("</g>\n");
                }
                Item::Fill { rgb, alpha, pts } => {
                    s.push_str("<path d=\"");
                    for (i, &q) in pts.iter().enumerate() {
                        let p = self.map(q, k);
                        let _ = write!(
                            s,
                            "{}{},{} ",
                            if i == 0 { "M" } else { "L" },
                            num(p[0]),
                            num(p[1])
                        );
                    }
                    let _ = write!(s, "Z\" fill=\"{}\"", hex(*rgb));
                    if *alpha < 0.999 {
                        let _ = write!(s, " fill-opacity=\"{}\"", num(*alpha));
                    }
                    s.push_str("/>\n");
                }
                Item::Image {
                    id,
                    corners,
                    opacity,
                } => {
                    let Some(a) = images.get(id) else { continue };
                    let [tl, tr, _, bl] = corners.map(|q| self.map(q, k));
                    let _ = write!(
                        s,
                        "<image x=\"0\" y=\"0\" width=\"1\" height=\"1\" preserveAspectRatio=\"none\" transform=\"matrix({} {} {} {} {} {})\"",
                        num6(tr[0] - tl[0]),
                        num6(tr[1] - tl[1]),
                        num6(bl[0] - tl[0]),
                        num6(bl[1] - tl[1]),
                        num(tl[0]),
                        num(tl[1])
                    );
                    if *opacity < 0.999 {
                        let _ = write!(s, " opacity=\"{}\"", num(*opacity));
                    }
                    let _ = writeln!(
                        s,
                        " href=\"data:{};base64,{}\"/>",
                        mime(&a.bytes),
                        base64(&a.bytes)
                    );
                }
            }
        }
        s.push_str("</svg>\n");
        s
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn raster(&self, images: &Images, format: Format, scale: f64) -> Result<Vec<u8>, String> {
        let svg = self.svg_bg(images, self.background || format == Format::Jpg);
        let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default())
            .map_err(|e| e.to_string())?;
        let size = tree.size();
        let (w, h) = (
            (size.width() as f64 * scale).ceil().max(1.0) as u32,
            (size.height() as f64 * scale).ceil().max(1.0) as u32,
        );
        if (w as u64) * (h as u64) > 16_000 * 16_000 {
            return Err("That is too big to export as a picture; try a smaller scale".into());
        }
        let mut pix = resvg::tiny_skia::Pixmap::new(w, h).ok_or("could not make the picture")?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale as f32, scale as f32),
            &mut pix.as_mut(),
        );
        // tiny-skia is premultiplied; image wants straight alpha.
        let mut rgba = image::RgbaImage::new(w, h);
        for (o, c) in rgba.pixels_mut().zip(pix.pixels()) {
            let c = c.demultiply();
            *o = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
        }
        let mut out = std::io::Cursor::new(Vec::new());
        match format {
            Format::Jpg => image::DynamicImage::ImageRgba8(rgba)
                .to_rgb8()
                .write_to(&mut out, image::ImageFormat::Jpeg),
            _ => rgba.write_to(&mut out, image::ImageFormat::Png),
        }
        .map_err(|e| e.to_string())?;
        Ok(out.into_inner())
    }

    /// A one-page PDF in points (72 per inch, 96 CSS px = 72 pt).
    pub fn pdf(&self, images: &Images) -> Result<Vec<u8>, String> {
        let k = 0.75;
        let [w, h] = self.out_size(k);
        let mut pdf = Pdf::default();
        let catalog = pdf.reserve();
        let pages = pdf.reserve();
        let page = pdf.reserve();
        let mut gs: Vec<(String, String)> = Vec::new(); // (name, dict)
        let mut xobj: Vec<(String, usize)> = Vec::new();
        let gs_name = |ca: f64, darken: bool, gs: &mut Vec<(String, String)>| -> String {
            let dict = format!(
                "<< /Type /ExtGState /CA {} /ca {}{} >>",
                num(ca),
                num(ca),
                if darken { " /BM /Darken" } else { "" }
            );
            if let Some((n, _)) = gs.iter().find(|(_, d)| *d == dict) {
                return n.clone();
            }
            let n = format!("G{}", gs.len());
            gs.push((n.clone(), dict));
            n
        };
        let mut c = String::new();
        // y down, like the screen.
        let _ = writeln!(c, "1 0 0 -1 0 {} cm", num(h));
        if self.background {
            let [r, g, b] = PAPER.map(|v| v as f64 / 255.0);
            let _ = writeln!(
                c,
                "{} {} {} rg 0 0 {} {} re f",
                num(r),
                num(g),
                num(b),
                num(w),
                num(h)
            );
        }
        let mut img_obj: std::collections::HashMap<u64, usize> = Default::default();
        for it in &self.items {
            match it {
                Item::Ink {
                    rgb,
                    alpha,
                    darken,
                    dash,
                    unit,
                    runs,
                } => {
                    let u = unit / self.ppp * k;
                    let [r, g, b] = rgb.map(|v| v as f64 / 255.0);
                    let mut body = String::new();
                    let _ = writeln!(body, "{} {} {} RG 1 J 1 j", num(r), num(g), num(b));
                    for run in runs {
                        let dashop = match dash {
                            Dash::Solid => String::new(),
                            Dash::Dashed => format!(
                                "[{} {}] {} d ",
                                num(3.0 * u),
                                num(3.0 * u),
                                num(run.along / self.ppp * k)
                            ),
                            Dash::Dotted => {
                                format!("[0 {}] {} d ", num(2.4 * u), num(run.along / self.ppp * k))
                            }
                        };
                        let _ = write!(body, "{}{} w ", dashop, num(run.width / self.ppp * k));
                        for (i, &q) in run.pts.iter().enumerate() {
                            let p = self.map(q, k);
                            let _ = write!(
                                body,
                                "{} {} {} ",
                                num(p[0]),
                                num(p[1]),
                                if i == 0 { "m" } else { "l" }
                            );
                        }
                        body.push_str("S\n");
                    }
                    if *alpha < 0.999 || *darken {
                        let n = gs_name(*alpha, *darken, &mut gs);
                        if runs.len() > 1 && !*darken {
                            // A transparency group, so overlapping runs do not darken.
                            let id = pdf.add_stream(
                                &format!(
                                    "/Type /XObject /Subtype /Form /BBox [-100000 -100000 100000 100000] /Group << /S /Transparency >> /Resources << >>"
                                ),
                                body.as_bytes(),
                            );
                            let xn = format!("X{}", xobj.len());
                            xobj.push((xn.clone(), id));
                            let _ = writeln!(c, "q /{n} gs /{xn} Do Q");
                        } else {
                            let _ = write!(c, "q /{n} gs\n{body}Q\n");
                        }
                    } else {
                        let _ = write!(c, "q\n{body}Q\n");
                    }
                }
                Item::Fill { rgb, alpha, pts } => {
                    let [r, g, b] = rgb.map(|v| v as f64 / 255.0);
                    c.push_str("q ");
                    if *alpha < 0.999 {
                        let n = gs_name(*alpha, false, &mut gs);
                        let _ = write!(c, "/{n} gs ");
                    }
                    let _ = write!(c, "{} {} {} rg ", num(r), num(g), num(b));
                    for (i, &q) in pts.iter().enumerate() {
                        let p = self.map(q, k);
                        let _ = write!(
                            c,
                            "{} {} {} ",
                            num(p[0]),
                            num(p[1]),
                            if i == 0 { "m" } else { "l" }
                        );
                    }
                    c.push_str("h f Q\n");
                }
                Item::Image {
                    id,
                    corners,
                    opacity,
                } => {
                    let Some(asset) = images.get(id) else {
                        continue;
                    };
                    let obj = match img_obj.get(id) {
                        Some(&o) => o,
                        None => {
                            let o = pdf_image(&mut pdf, &asset.bytes)?;
                            img_obj.insert(*id, o);
                            o
                        }
                    };
                    let xn = format!("X{}", xobj.len());
                    xobj.push((xn.clone(), obj));
                    let [tl, tr, _, bl] = corners.map(|q| self.map(q, k));
                    // Image space: (0,0) bottom-left, (1,1) top-right.
                    c.push_str("q ");
                    if *opacity < 0.999 {
                        let n = gs_name(*opacity, false, &mut gs);
                        let _ = write!(c, "/{n} gs ");
                    }
                    let _ = writeln!(
                        c,
                        "{} {} {} {} {} {} cm /{xn} Do Q",
                        num6(tr[0] - tl[0]),
                        num6(tr[1] - tl[1]),
                        num6(tl[0] - bl[0]),
                        num6(tl[1] - bl[1]),
                        num(bl[0]),
                        num(bl[1])
                    );
                }
            }
        }
        let content = pdf.add_stream("", c.as_bytes());
        let mut res = String::from("<< ");
        if !gs.is_empty() {
            res.push_str("/ExtGState << ");
            for (n, d) in &gs {
                let _ = write!(res, "/{n} {d} ");
            }
            res.push_str(">> ");
        }
        if !xobj.is_empty() {
            res.push_str("/XObject << ");
            for (n, id) in &xobj {
                let _ = write!(res, "/{n} {id} 0 R ");
            }
            res.push_str(">> ");
        }
        res.push_str(">>");
        pdf.set(catalog, format!("<< /Type /Catalog /Pages {pages} 0 R >>"));
        pdf.set(
            pages,
            format!("<< /Type /Pages /Kids [{page} 0 R] /Count 1 >>"),
        );
        pdf.set(
            page,
            format!(
                "<< /Type /Page /Parent {pages} 0 R /MediaBox [0 0 {} {}] /Resources {res} /Contents {content} 0 R /Group << /S /Transparency /CS /DeviceRGB >> >>",
                num(w),
                num(h)
            ),
        );
        let info = pdf.add("<< /Producer (OG Paper) /Creator (OG Paper) >>".into());
        Ok(pdf.finish(catalog, info))
    }
}

/// More digits, for transforms of very small pictures.
fn num6(v: f64) -> String {
    let s = format!("{:.6}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

/// An image XObject: JPEGs as they are, anything else decoded to RGB with
/// its alpha as a soft mask, both deflated.
fn pdf_image(pdf: &mut Pdf, bytes: &[u8]) -> Result<usize, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let (w, h) = (img.width(), img.height());
    if matches!(image::guess_format(bytes), Ok(image::ImageFormat::Jpeg))
        && matches!(img.color(), image::ColorType::Rgb8 | image::ColorType::L8)
    {
        let cs = if img.color() == image::ColorType::L8 {
            "/DeviceGray"
        } else {
            "/DeviceRGB"
        };
        return Ok(pdf.add_stream(
            &format!("/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace {cs} /BitsPerComponent 8 /Filter /DCTDecode"),
            bytes,
        ));
    }
    let rgba = img.to_rgba8();
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    let mut alpha = Vec::with_capacity((w * h) as usize);
    let mut opaque = true;
    for p in rgba.pixels() {
        rgb.extend_from_slice(&p.0[..3]);
        alpha.push(p.0[3]);
        opaque &= p.0[3] == 255;
    }
    let smask = (!opaque).then(|| {
        pdf.add_stream(
            &format!("/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode"),
            &miniz_oxide::deflate::compress_to_vec_zlib(&alpha, 6),
        )
    });
    let mask = smask.map_or(String::new(), |m| format!(" /SMask {m} 0 R"));
    Ok(pdf.add_stream(
        &format!("/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode{mask}"),
        &miniz_oxide::deflate::compress_to_vec_zlib(&rgb, 6),
    ))
}

/// A minimal PDF writer: numbered objects, then the cross-reference table.
#[derive(Default)]
struct Pdf {
    objs: Vec<Vec<u8>>,
}

impl Pdf {
    fn reserve(&mut self) -> usize {
        self.objs.push(Vec::new());
        self.objs.len()
    }

    fn set(&mut self, id: usize, dict: String) {
        self.objs[id - 1] = dict.into_bytes();
    }

    fn add(&mut self, dict: String) -> usize {
        self.objs.push(dict.into_bytes());
        self.objs.len()
    }

    /// A stream; content streams (no `dict`) are deflated here.
    fn add_stream(&mut self, dict: &str, data: &[u8]) -> usize {
        let (dict, data) =
            if dict.is_empty() || (dict.contains("/Form") && !dict.contains("/Filter")) {
                let z = miniz_oxide::deflate::compress_to_vec_zlib(data, 6);
                (format!("{dict} /Filter /FlateDecode"), z)
            } else {
                (dict.to_string(), data.to_vec())
            };
        let mut o = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        o.extend_from_slice(&data);
        o.extend_from_slice(b"\nendstream");
        self.objs.push(o);
        self.objs.len()
    }

    fn finish(self, root: usize, info: usize) -> Vec<u8> {
        let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offs = Vec::new();
        for (i, o) in self.objs.iter().enumerate() {
            offs.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(o);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        let mut t = format!("xref\n0 {}\n0000000000 65535 f \n", self.objs.len() + 1);
        for o in offs {
            let _ = write!(t, "{o:010} 00000 n \n");
        }
        let _ = write!(
            t,
            "trailer\n<< /Size {} /Root {root} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            self.objs.len() + 1
        );
        out.extend_from_slice(t.as_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Page {
        Page {
            items: vec![
                Item::Ink {
                    rgb: [10, 20, 30],
                    alpha: 0.5,
                    darken: false,
                    dash: Dash::Dashed,
                    unit: 4.0,
                    runs: vec![
                        Run {
                            pts: vec![[10.0, 10.0], [50.0, 20.0]],
                            width: 4.0,
                            along: 0.0,
                        },
                        Run {
                            pts: vec![[50.0, 20.0], [90.0, 10.0]],
                            width: 6.0,
                            along: 41.2,
                        },
                    ],
                },
                Item::Fill {
                    rgb: [200, 0, 0],
                    alpha: 1.0,
                    pts: vec![[0.0, 0.0], [20.0, 0.0], [10.0, 20.0]],
                },
            ],
            origin: [0.0, 0.0],
            size: [200.0, 100.0],
            ppp: 2.0,
            background: true,
        }
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn svg_has_the_items_in_css_px() {
        let s = page().svg(&Default::default());
        assert!(s.starts_with("<svg") && s.trim_end().ends_with("</svg>"));
        assert!(s.contains("width=\"100\" height=\"50\""));
        assert!(s.contains("stroke=\"#0a141e\""));
        assert!(s.contains("opacity=\"0.5\""));
        assert!(s.contains("stroke-dasharray=\"6 6\""));
        assert!(s.contains("<path d=\"M0,0 L10,0 L5,10 Z\" fill=\"#c80000\""));
    }

    #[test]
    fn pdf_is_well_formed() {
        let b = page().pdf(&Default::default()).unwrap();
        let t = String::from_utf8_lossy(&b);
        assert!(t.starts_with("%PDF-1.4"));
        assert!(t.trim_end().ends_with("%%EOF"));
        // The xref offset points at "xref".
        let sx: usize = t
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(&b[sx..sx + 4], b"xref");
        // Every object offset in the table points at "N 0 obj".
        let table = String::from_utf8(b[sx..].to_vec()).unwrap();
        for (i, line) in table
            .lines()
            .skip(3)
            .take_while(|l| l.ends_with(" n "))
            .enumerate()
        {
            let off: usize = line[..10].parse().unwrap();
            assert!(b[off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()));
        }
        assert!(t.contains("/MediaBox [0 0 75 37.5]"));
        assert!(t.contains("/Group << /S /Transparency >>"));
    }

    #[test]
    fn pictures_embed_and_rasterise() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(4, 2, image::Rgba([255, 0, 0, 128]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let asset = crate::images::load(png.into_inner()).unwrap();
        let mut imgs: Images = Default::default();
        imgs.insert(7, asset);
        let mut p = page();
        p.items.push(Item::Image {
            id: 7,
            corners: [[100.0, 20.0], [180.0, 20.0], [180.0, 60.0], [100.0, 60.0]],
            opacity: 1.0,
        });
        let svg = p.svg(&imgs);
        assert!(svg.contains("href=\"data:image/png;base64,"));
        let pdf = String::from_utf8_lossy(&p.pdf(&imgs).unwrap()).into_owned();
        assert!(pdf.contains("/Subtype /Image /Width 4 /Height 2"));
        assert!(pdf.contains("/SMask"));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let out = p.raster(&imgs, Format::Png, 2.0).unwrap();
            let im = image::load_from_memory(&out).unwrap();
            assert_eq!((im.width(), im.height()), (200, 100));
            // The picture's middle is reddish over paper.
            let px = im.to_rgba8().get_pixel(140, 40).0;
            assert!(px[0] > 200 && px[1] < 160, "{px:?}");
        }
    }
}
