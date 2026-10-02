// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! PDF import: each page becomes a picture, stacked top to bottom, ready to
//! write on, one page per frame so a long document never freezes the app;
//! the whole import is one undo step. Native builds render pages with
//! `hayro` (pure Rust); the web page renders them with pdf.js, loaded only
//! when needed, and hands them over one by one, which keeps the web app
//! small.

use std::collections::VecDeque;

#[cfg(not(target_arch = "wasm32"))]
use hayro::{hayro_interpret::InterpreterSettings, hayro_syntax::Pdf, RenderCache, RenderSettings};
use ogpaper_core::Camera;

use crate::images::Asset;

use crate::objects::{ObjData, ObjRef};
use crate::shapes::Geom;
use crate::App;

/// Most pages taken from one document.
const MAX_PAGES: usize = 200;
/// Longest side of a rendered page (px): sharp when zoomed in a little.
#[cfg(not(target_arch = "wasm32"))]
const PAGE_PX: f32 = 2200.0;

/// Where the pages come from.
enum Source {
    #[cfg(not(target_arch = "wasm32"))]
    Hayro(Pdf),
    /// Rendered elsewhere (the web page), arriving in order.
    Pages(VecDeque<Asset>),
}

pub struct PdfJob {
    source: Source,
    next: usize,
    total: usize,
    /// The camera at the start: pages are laid out in its frame, so panning
    /// or zooming during the import doesn't scatter them.
    cam: Camera,
    /// Centre x, top of the next page, page width and gap (camera units).
    x: f64,
    y: f64,
    w: f64,
    gap: f64,
    groups: Vec<ObjRef>,
    name: String,
}

/// Render page `i` as an RGBA picture on white.
#[cfg(not(target_arch = "wasm32"))]
fn render_page(pdf: &Pdf, i: usize) -> Option<image::RgbaImage> {
    let page = pdf.pages().get(i)?;
    let (pw, ph) = page.render_dimensions();
    if !(pw > 0.0 && ph > 0.0) {
        return None;
    }
    let k = (PAGE_PX / pw.max(ph)).min(4.0);
    let pix = hayro::render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings {
            x_scale: k,
            y_scale: k,
            bg_color: hayro::vello_cpu::color::palette::css::WHITE,
            ..Default::default()
        },
    );
    let (w, h) = (pix.width() as u32, pix.height() as u32);
    // Opaque (white behind), so premultiplied is the same as straight.
    image::RgbaImage::from_raw(w, h, pix.data_as_u8_slice().to_vec())
}

/// The page count, or why the file can't be read.
#[cfg(not(target_arch = "wasm32"))]
fn open(bytes: Vec<u8>) -> Result<(Pdf, usize), String> {
    let pdf = Pdf::new(bytes).map_err(|e| match format!("{e:?}") {
        s if s.contains("Encrypt") => "it is password-protected".to_string(),
        _ => "it is not a PDF this app can read".to_string(),
    })?;
    let n = pdf.pages().len();
    if n == 0 {
        return Err("it has no pages".into());
    }
    Ok((pdf, n))
}

impl App {
    /// Start importing a PDF at `at` (screen px; the middle if `None`).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn import_pdf(&mut self, bytes: Vec<u8>, at: Option<[f64; 2]>, name: &str) {
        let (pdf, n) = match open(bytes) {
            Ok(v) => v,
            Err(e) => return self.say(format!("Could not import {name}: {e}")),
        };
        let (pw, ph) = pdf
            .pages()
            .get(0)
            .map_or((612.0, 792.0), |p| p.render_dimensions());
        self.pdf_start(Source::Hayro(pdf), n, pw as f64 / ph as f64, at, name);
    }

    /// A page rendered by the web page: page `index` of `total`. Page 0
    /// starts a new import.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn pdf_page(
        &mut self,
        page: Asset,
        index: usize,
        total: usize,
        at: Option<[f64; 2]>,
        name: &str,
    ) {
        if index == 0 {
            let aspect = page.w as f64 / page.h.max(1) as f64;
            self.pdf_start(Source::Pages(VecDeque::new()), total, aspect, at, name);
        }
        if let Some(PdfJob {
            source: Source::Pages(q),
            ..
        }) = self.pdf.as_mut()
        {
            q.push_back(page);
        }
        self.redraw();
    }

    fn pdf_start(
        &mut self,
        source: Source,
        n: usize,
        aspect: f64,
        at: Option<[f64; 2]>,
        name: &str,
    ) {
        let at = self.drop_point(at);
        // First page: as wide as fits 80% of the width and 70% of the height.
        let s = self.size();
        let wpx = (s[0] * 0.8).min(s[1] * 0.7 * aspect);
        let hpx = wpx / aspect;
        let c = self.px_to_cam(at);
        let ppc = self.cam.ppc();
        let w = wpx / ppc;
        self.pdf = Some(PdfJob {
            source,
            next: 0,
            total: n.min(MAX_PAGES),
            cam: self.cam.clone(),
            x: c[0],
            y: c[1] - hpx * 0.5 / ppc,
            w,
            gap: w * 0.04,
            groups: Vec::new(),
            name: name.to_string(),
        });
        if n > MAX_PAGES {
            self.say(format!(
                "{name} has {n} pages; importing the first {MAX_PAGES}"
            ));
        }
        self.redraw();
    }

    /// One page of a running import (call once per frame). True while
    /// there is more to do.
    pub(crate) fn pdf_step(&mut self) -> bool {
        let Some(mut job) = self.pdf.take() else {
            return false;
        };
        let i = job.next;
        let page: Option<Result<Asset, String>> = match &mut job.source {
            #[cfg(not(target_arch = "wasm32"))]
            Source::Hayro(pdf) => Some(
                render_page(pdf, i)
                    .ok_or_else(|| "could not draw the page".to_string())
                    .and_then(crate::images::from_rgba),
            ),
            Source::Pages(q) => match q.pop_front() {
                Some(a) => Some(Ok(a)),
                // Not here yet: wait for the page to hand it over.
                None => {
                    self.pdf = Some(job);
                    return false;
                }
            },
        };
        job.next += 1;
        if let Some(page) = page {
            match page {
                Ok(asset) => {
                    let (iw, ih) = (asset.w as f64, asset.h as f64);
                    let h = job.w * ih / iw;
                    let id = crate::images::id_of(&asset.bytes);
                    self.objs.images.entry(id).or_insert(asset);
                    let data = ObjData::Image {
                        id,
                        geom: Geom {
                            center: [job.x, job.y + h * 0.5],
                            half: [job.w * 0.5, h * 0.5],
                            rot: 0.0,
                            pts: vec![],
                        },
                        opacity: 255,
                        crop: crate::objects::FULL_CROP,
                    };
                    let cur = std::mem::replace(&mut self.cam, job.cam.clone());
                    let (g, ids) = self.add_group(&data, None);
                    self.cam = cur;
                    if job.groups.is_empty() {
                        self.record_edit(vec![], ids);
                    } else {
                        self.record_added_merged(ids);
                    }
                    job.groups.push(ObjRef::Group(g));
                    job.y += h + job.gap;
                }
                Err(e) => log::warn!("pdf page {i}: {e}"),
            }
        }
        #[cfg(target_arch = "wasm32")]
        crate::web::touch();
        if job.next < job.total {
            self.say(format!(
                "Importing {}: page {} of {}…",
                job.name,
                job.next + 1,
                job.total
            ));
            self.pdf = Some(job);
            self.redraw();
            true
        } else {
            let n = job.groups.len();
            self.edit.selection = job.groups;
            if !self.ui.tool.selects() {
                self.ui.tool = crate::ui::Tool::Select;
            }
            self.say(if n == 1 {
                format!("Imported {} — write on it, or drag to move", job.name)
            } else {
                format!(
                    "Imported {n} pages of {} — write on them, or drag to move",
                    job.name
                )
            });
            self.redraw();
            false
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    /// A one-page PDF with a filled square, written by hand.
    fn tiny_pdf() -> Vec<u8> {
        let content = b"1 0 0 rg 20 20 60 60 re f";
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 200] /Contents 4 0 R >>".to_string(),
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                content.len(),
                std::str::from_utf8(content).unwrap()
            ),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offs = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offs.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let x = out.len();
        let mut t = format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1);
        for o in offs {
            t.push_str(&format!("{o:010} 00000 n \n"));
        }
        t.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n",
            objs.len() + 1
        ));
        out.extend_from_slice(t.as_bytes());
        out
    }

    #[test]
    fn renders_a_page() {
        let (pdf, n) = super::open(tiny_pdf()).unwrap();
        assert_eq!(n, 1);
        let img = super::render_page(&pdf, 0).unwrap();
        // 100 x 200 pt, longest side scaled to 800 (4x cap).
        assert_eq!((img.width(), img.height()), (400, 800));
        // White margin, red square (PDF y is up: the square is near the bottom).
        assert_eq!(img.get_pixel(5, 5).0, [255, 255, 255, 255]);
        let p = img.get_pixel(200, 800 - 200).0;
        assert!(p[0] > 240 && p[1] < 20 && p[2] < 20, "{p:?}");
    }

    #[test]
    fn rejects_non_pdf() {
        assert!(super::open(b"hello".to_vec()).is_err());
    }
}
