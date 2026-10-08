// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Pictures on the canvas (pasted, dropped or inserted). The canvas keeps
//! each picture's file once (PNG, JPEG, GIF or WebP, by content hash); an
//! image object only names it.

use std::sync::Arc;

/// Longest side kept; bigger pictures are scaled down when added.
pub const MAX_SIDE: u32 = 4096;

/// A picture's file and its pixel size.
#[derive(Clone, Debug)]
pub struct Asset {
    pub bytes: Arc<Vec<u8>>,
    pub w: u32,
    pub h: u32,
}

/// Whether `b` is a sound file an audio clip keeps: WebM / Matroska, Ogg,
/// MP4 / M4A, WAV or MP3.
pub fn is_audio(b: &[u8]) -> bool {
    b.starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
        || b.starts_with(b"OggS")
        || (b.len() > 12 && &b[4..8] == b"ftyp")
        || (b.starts_with(b"RIFF") && b.len() > 12 && &b[8..12] == b"WAVE")
        || b.starts_with(b"ID3")
        || (b.len() > 1 && b[0] == 0xff && b[1] & 0xe0 == 0xe0)
}

/// Content hash (FNV-1a, 64 bit): the same picture pasted twice is stored once.
pub fn id_of(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    // 0 means "no picture" nowhere, but keep ids non-zero anyway.
    h.max(1)
}

/// Check a picture file and make it an asset: scaled down past
/// [`MAX_SIDE`] (re-encoded as PNG), otherwise kept as it is.
pub fn prepare(bytes: Vec<u8>) -> Result<Asset, String> {
    let fmt = image::guess_format(&bytes)
        .map_err(|_| "not a picture this app reads (PNG, JPEG, GIF or WebP)".to_string())?;
    if !matches!(
        fmt,
        image::ImageFormat::Png
            | image::ImageFormat::Jpeg
            | image::ImageFormat::Gif
            | image::ImageFormat::WebP
    ) {
        return Err(format!("{fmt:?} pictures are not supported"));
    }
    let img = image::load_from_memory_with_format(&bytes, fmt).map_err(|e| e.to_string())?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err("the picture is empty".into());
    }
    if w.max(h) <= MAX_SIDE {
        return Ok(Asset {
            bytes: Arc::new(bytes),
            w,
            h,
        });
    }
    let small = img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
    from_rgba(small.to_rgba8())
}

/// A stored picture (from a file): only its header is read here.
pub fn load(bytes: Vec<u8>) -> Result<Asset, String> {
    // Audio clips' sounds share this store (no size: nothing to draw).
    if is_audio(&bytes) {
        return Ok(Asset {
            bytes: Arc::new(bytes),
            w: 0,
            h: 0,
        });
    }
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_dimensions()
        .map_err(|e| e.to_string())?;
    Ok(Asset {
        bytes: Arc::new(bytes),
        w,
        h,
    })
}

/// An asset from raw pixels (the desktop clipboard), stored as PNG.
pub fn from_rgba(img: image::RgbaImage) -> Result<Asset, String> {
    let img = if img.width().max(img.height()) > MAX_SIDE {
        image::imageops::resize(
            &img,
            (img.width() as u64 * MAX_SIDE as u64 / img.width().max(img.height()) as u64).max(1)
                as u32,
            (img.height() as u64 * MAX_SIDE as u64 / img.width().max(img.height()) as u64).max(1)
                as u32,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img
    };
    let (w, h) = img.dimensions();
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(Asset {
        bytes: Arc::new(out.into_inner()),
        w,
        h,
    })
}

/// An SVG drawing as a picture, rasterised at twice its size (so it stays
/// sharp when zoomed a little) and capped at [`MAX_SIDE`].
#[cfg(not(target_arch = "wasm32"))]
pub fn from_svg(bytes: &[u8]) -> Result<Asset, String> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default())
        .map_err(|e| format!("not a drawing this app reads ({e})"))?;
    let size = tree.size();
    let (w, h) = (size.width() as f64, size.height() as f64);
    if !(w > 0.0 && h > 0.0) {
        return Err("the drawing is empty".into());
    }
    let k = (2.0f64).min(MAX_SIDE as f64 / w.max(h));
    let (pw, ph) = (
        (w * k).ceil().max(1.0) as u32,
        (h * k).ceil().max(1.0) as u32,
    );
    let mut pix = resvg::tiny_skia::Pixmap::new(pw, ph).ok_or("the drawing is too big")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(k as f32, k as f32),
        &mut pix.as_mut(),
    );
    let mut img = image::RgbaImage::new(pw, ph);
    for (o, c) in img.pixels_mut().zip(pix.pixels()) {
        let c = c.demultiply();
        *o = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    from_rgba(img)
}

/// Whether `text` looks like an SVG document.
#[cfg(not(target_arch = "wasm32"))]
pub fn is_svg(text: &str) -> bool {
    let t = text.trim_start();
    let t = t.strip_prefix('\u{feff}').unwrap_or(t);
    (t.starts_with("<svg") || t.starts_with("<?xml")) && t.contains("<svg")
}

/// Pixels for the GPU, with a mip chain (each level half the last), fitted
/// into `max_side`.
pub fn mips(asset: &Asset, max_side: u32) -> Option<Vec<image::RgbaImage>> {
    let img = image::load_from_memory(&asset.bytes).ok()?;
    let mut base = img.to_rgba8();
    let side = max_side.clamp(64, MAX_SIDE);
    if base.width().max(base.height()) > side {
        let k = side as f64 / base.width().max(base.height()) as f64;
        base = image::imageops::resize(
            &base,
            ((base.width() as f64 * k) as u32).max(1),
            ((base.height() as f64 * k) as u32).max(1),
            image::imageops::FilterType::Triangle,
        );
    }
    let mut levels = vec![base];
    loop {
        let last = levels.last().expect("one level");
        let (w, h) = last.dimensions();
        if w == 1 && h == 1 {
            break;
        }
        let next = image::imageops::resize(
            last,
            (w / 2).max(1),
            (h / 2).max(1),
            image::imageops::FilterType::Triangle,
        );
        levels.push(next);
    }
    Some(levels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 60, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn prepares_and_mips_pictures() {
        let a = prepare(png(40, 20)).unwrap();
        assert_eq!((a.w, a.h), (40, 20));
        let m = mips(&a, 4096).unwrap();
        assert_eq!(m[0].dimensions(), (40, 20));
        assert_eq!(m.last().unwrap().dimensions(), (1, 1));
        assert!(prepare(b"hello".to_vec()).is_err());
        assert_eq!(id_of(&a.bytes), id_of(&png(40, 20)));
    }

    #[test]
    fn big_pictures_are_scaled_down() {
        let a = prepare(png(MAX_SIDE + 100, 10)).unwrap();
        assert_eq!(a.w, MAX_SIDE);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod svg_tests {
    #[test]
    fn svg_becomes_a_picture() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#f00"/></svg>"##;
        assert!(super::is_svg(svg));
        assert!(!super::is_svg("just <b>text</b>"));
        let a = super::from_svg(svg.as_bytes()).unwrap();
        assert_eq!((a.w, a.h), (80, 40));
    }
}
