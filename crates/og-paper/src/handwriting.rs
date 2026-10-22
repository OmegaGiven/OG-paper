//! Handwriting to text (the Write tool): write with it like a pen, and a
//! moment after you stop, what you wrote turns into a text in its place
//! (one undo brings the ink back). Read on the device only: Apple's Vision
//! on iPhone, iPad and Mac (from a picture of the ink), the browser's
//! handwriting recognition on the web where it has one (from the strokes).
//! Elsewhere the tool says it can't read here and leaves the ink.

use std::sync::mpsc;
use std::time::Duration;

use crate::objects::{ObjData, TextStyle};
use crate::shapes::Geom;
use crate::{App, Instant};

/// How long after the last stroke the writing is read.
const PAUSE: Duration = Duration::from_millis(1200);

/// Ink written with the Write tool, waiting to be read.
#[derive(Default)]
pub struct Writing {
    /// The strokes (scene ids) and their points (camera units, with the
    /// time each was drawn, ms from the first).
    pub strokes: Vec<(u32, Vec<[f64; 3]>)>,
    pub last: Option<Instant>,
    pub started: Option<Instant>,
    /// Being read: the reply comes here (apps); on the web it comes back
    /// through `web::Cmd::Handwritten`.
    pub reading: bool,
    pub reply: Option<mpsc::Receiver<Result<String, String>>>,
    /// The camera the points are in.
    pub cam: Option<ogpaper_core::Camera>,
}

/// Whether this device can read handwriting.
pub fn available() -> bool {
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    return true;
    #[cfg(target_arch = "wasm32")]
    return crate::web::handwriting_supported();
    #[allow(unreachable_code)]
    false
}

/// The strokes drawn as black ink on white, as a PNG about 160 px tall per
/// line of writing (what Vision reads best), with a margin.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn picture(strokes: &[Vec<[f64; 2]>]) -> Option<Vec<u8>> {
    let pts = strokes.iter().flatten();
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in pts {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
    if !(w.is_finite() && h.is_finite()) || w.max(h) <= 0.0 {
        return None;
    }
    // Most writing is a line or two: about 160 px a line, at most 4000
    // px across.
    let k = (160.0 / h.max(w / 12.0)).min(4000.0 / w.max(1e-300));
    let margin = 24.0;
    let (iw, ih) = (
        (w * k + 2.0 * margin).ceil().clamp(16.0, 4096.0) as u32,
        (h * k + 2.0 * margin).ceil().clamp(16.0, 4096.0) as u32,
    );
    let mut img = image::GrayImage::from_pixel(iw, ih, image::Luma([255]));
    let r = (h.max(w / 12.0) * k / 40.0).clamp(2.0, 8.0);
    let mut dot = |x: f64, y: f64| {
        let (x0, x1) = ((x - r).floor().max(0.0) as u32, (x + r).ceil().min(iw as f64 - 1.0) as u32);
        let (y0, y1) = ((y - r).floor().max(0.0) as u32, (y + r).ceil().min(ih as f64 - 1.0) as u32);
        for yy in y0..=y1 {
            for xx in x0..=x1 {
                if (xx as f64 - x).powi(2) + (yy as f64 - y).powi(2) <= r * r {
                    img.put_pixel(xx, yy, image::Luma([0]));
                }
            }
        }
    };
    for s in strokes {
        let at = |p: &[f64; 2]| ((p[0] - lo[0]) * k + margin, (p[1] - lo[1]) * k + margin);
        if s.len() == 1 {
            let (x, y) = at(&s[0]);
            dot(x, y);
        }
        for pair in s.windows(2) {
            let (a, b) = (at(&pair[0]), at(&pair[1]));
            let n = ((b.0 - a.0).hypot(b.1 - a.1) / (r * 0.5)).ceil().max(1.0) as usize;
            for i in 0..=n {
                let t = i as f64 / n as f64;
                dot(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            }
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Read the words in a PNG with Apple's Vision (on the device), lines
/// joined by newlines.
#[cfg(any(target_os = "ios", target_os = "macos"))]
pub fn read_picture(png: &[u8]) -> Result<String, String> {
    use objc2::rc::Retained;
    use objc2::AllocAnyThread;
    use objc2_foundation::{NSArray, NSData, NSDictionary};
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel,
    };
    objc2::rc::autoreleasepool(|_| {
        let data = NSData::with_bytes(png);
        let handler = VNImageRequestHandler::initWithData_options(
            VNImageRequestHandler::alloc(),
            &data,
            &NSDictionary::new(),
        );
        let req = VNRecognizeTextRequest::new();
        req.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        req.setUsesLanguageCorrection(true);
        let base: Retained<VNRequest> = Retained::into_super(Retained::into_super(req.clone()));
        let reqs = NSArray::from_retained_slice(&[base]);
        handler
            .performRequests_error(&reqs)
            .map_err(|e| e.localizedDescription().to_string())?;
        let mut lines = Vec::new();
        if let Some(results) = req.results() {
            for i in 0..results.count() {
                let obs = results.objectAtIndex(i);
                if let Some(best) = obs.topCandidates(1).firstObject() {
                    lines.push(best.string().to_string());
                }
            }
        }
        Ok(lines.join("\n"))
    })
}

impl App {
    /// A stroke drawn with the Write tool: keep it to read after a pause.
    pub(crate) fn writing_add(&mut self, id: u32, cam_pts: &[[f64; 2]]) {
        let now = Instant::now();
        let w = &mut self.writing;
        if w.reading {
            return;
        }
        let t0 = *w.started.get_or_insert(now);
        let ms = now.duration_since(t0).as_secs_f64() * 1000.0;
        let n = cam_pts.len().max(1) as f64;
        // Points spread over the moment the stroke was drawn.
        let pts = cam_pts
            .iter()
            .enumerate()
            .map(|(i, p)| [p[0], p[1], (ms - 300.0 * (1.0 - i as f64 / n)).max(0.0)])
            .collect();
        w.strokes.push((id, pts));
        w.last = Some(now);
        if w.cam.is_none() {
            w.cam = Some(self.cam.clone());
        }
    }

    /// Each frame: once the writing has paused, read it; take a reply.
    pub(crate) fn writing_tick(&mut self) {
        if let Some(rx) = &self.writing.reply {
            match rx.try_recv() {
                Ok(r) => {
                    self.writing.reply = None;
                    self.writing_done(r);
                }
                Err(mpsc::TryRecvError::Empty) => self.redraw(),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.writing.reply = None;
                    self.writing_done(Err("the reader stopped".into()));
                }
            }
            return;
        }
        let w = &self.writing;
        if w.reading || w.strokes.is_empty() {
            return;
        }
        let Some(last) = w.last else {
            return;
        };
        // Still writing (or the pen is down): wait.
        if last.elapsed() < PAUSE || self.gesture != crate::Gesture::None {
            self.redraw();
            return;
        }
        if !available() {
            self.say(
                "Handwriting can't be read here: it needs an iPhone, iPad or Mac, or a browser that reads handwriting"
                    .to_string(),
            );
            self.writing = Writing::default();
            return;
        }
        self.writing.reading = true;
        let strokes: Vec<Vec<[f64; 2]>> = self
            .writing
            .strokes
            .iter()
            .map(|(_, p)| p.iter().map(|q| [q[0], q[1]]).collect())
            .collect();
        #[cfg(target_arch = "wasm32")]
        {
            // The browser reads strokes: points in px with times.
            let ppc = self.cam.ppc();
            let json: Vec<String> = self
                .writing
                .strokes
                .iter()
                .map(|(_, p)| {
                    let pts: Vec<String> = p
                        .iter()
                        .map(|q| format!("[{:.1},{:.1},{:.0}]", q[0] * ppc, q[1] * ppc, q[2]))
                        .collect();
                    format!("[{}]", pts.join(","))
                })
                .collect();
            let _ = strokes;
            crate::web::handwriting_request(format!("[{}]", json.join(",")));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (tx, rx) = mpsc::channel();
            self.writing.reply = Some(rx);
            let job = move || {
                let r = match picture(&strokes) {
                    None => Err("nothing to read".to_string()),
                    #[cfg(any(target_os = "ios", target_os = "macos"))]
                    Some(png) => read_picture(&png),
                    #[cfg(not(any(target_os = "ios", target_os = "macos")))]
                    Some(_) => Err("no reader here".to_string()),
                };
                let _ = tx.send(r);
            };
            if std::thread::Builder::new()
                .name("handwriting".into())
                .spawn(job)
                .is_err()
            {
                self.writing = Writing::default();
            }
        }
        self.redraw();
    }

    /// The writing read: put a text where the ink was (one undo step), or
    /// say why not and leave the ink.
    pub(crate) fn writing_done(&mut self, read: Result<String, String>) {
        let w = std::mem::take(&mut self.writing);
        let text = match read {
            Ok(t) if !t.trim().is_empty() => t.trim().to_string(),
            Ok(_) => {
                self.say("Couldn't read that: try writing a little larger".to_string());
                return;
            }
            Err(e) => {
                self.say(format!("Couldn't read the writing: {e}"));
                return;
            }
        };
        // Only the strokes still there (not erased or undone meanwhile).
        let ids: Vec<u32> = w
            .strokes
            .iter()
            .map(|(id, _)| *id)
            .filter(|&id| (id as usize) < self.scene.strokes.len() && !self.scene.strokes[id as usize].deleted)
            .collect();
        if ids.is_empty() {
            return;
        }
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for (_, p) in &w.strokes {
            for q in p {
                lo = [lo[0].min(q[0]), lo[1].min(q[1])];
                hi = [hi[0].max(q[0]), hi[1].max(q[1])];
            }
        }
        let lines = text.lines().count().max(1) as f64;
        // Letters about as tall as the writing's (a cap height is about
        // 0.6 of a written line, with its tails).
        let size = ((hi[1] - lo[1]) / lines * 0.6).max(1e-300);
        let mut style: TextStyle = self.ui.text;
        let [r, g, b, _] = self.ui.pen.color.to_array();
        style.color = crate::shapes::rgba(r, g, b, 255);
        let b = crate::objects::text_box(&text, &style, size);
        let half = [b[0] * 0.5, b[1] * 0.5];
        let data = ObjData::Text {
            text,
            style,
            geom: Geom {
                center: [lo[0] + half[0], lo[1] + half[1]],
                half,
                rot: 0.0,
                pts: vec![],
            },
            size,
            seed: self.next_seed_pub(),
        };
        // In the camera the ink was written in.
        let now = w.cam.map(|c| std::mem::replace(&mut self.cam, c));
        let (_, added) = self.add_group(&data, None);
        if let Some(c) = now {
            self.cam = c;
        }
        for &id in &ids {
            self.scene.delete(id);
        }
        self.record_edit(ids, added);
        self.redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_becomes_a_picture_to_read() {
        // An "l" and an "o", roughly.
        let l = vec![[0.0, 0.0], [0.0, 10.0]];
        let o: Vec<[f64; 2]> = (0..=16)
            .map(|i| {
                let a = i as f64 / 16.0 * std::f64::consts::TAU;
                [5.0 + 2.5 * a.cos(), 7.5 + 2.5 * a.sin()]
            })
            .collect();
        let png = picture(&[l, o]).expect("a picture");
        let img = image::load_from_memory(&png).unwrap().to_luma8();
        assert!(img.height() >= 150 && img.height() <= 260, "{}", img.height());
        let dark = img.pixels().filter(|p| p.0[0] < 128).count();
        assert!(dark > 100, "ink drawn: {dark}");
        assert!(picture(&[]).is_none());
    }

    #[test]
    fn read_writing_replaces_the_ink_with_a_text_in_one_undo() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        app.size_override = Some([800.0, 600.0]);
        app.run_commands(&serde_json::json!([
            {"add": "stroke", "points": [[0, 0], [0, 40]], "width": 3},
            {"add": "stroke", "points": [[10, 20], [30, 20]], "width": 3}
        ]));
        let ids: Vec<u32> = (0..app.scene.strokes.len() as u32)
            .filter(|&i| !app.scene.strokes[i as usize].deleted)
            .collect();
        for &id in &ids {
            app.writing.strokes.push((id, vec![[0.0, 0.0, 0.0], [0.03, 0.04, 10.0]]));
        }
        app.writing.cam = Some(app.cam.clone());
        app.writing_done(Ok("Hi".into()));
        let list = app.run_commands(&serde_json::json!([{"get": "objects"}]))[0]["objects"].clone();
        let kinds: Vec<&str> = list.as_array().unwrap().iter().map(|o| o["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["text"], "{list}");
        assert_eq!(list[0]["text"], "Hi");
        app.undo_redo(false);
        let list = app.run_commands(&serde_json::json!([{"get": "objects"}]))[0]["objects"].clone();
        assert!(list.as_array().unwrap().iter().all(|o| o["kind"] != "text"), "the ink is back: {list}");
    }
}
