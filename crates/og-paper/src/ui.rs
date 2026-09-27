// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Screen-size-independent controls: two round buttons in the bottom-right
//! corner (Tool and Color) that fan out into radial menus, small undo/redo
//! buttons, and a ☰ menu top-left. Icons are drawn, not taken from a font.

use std::f32::consts::{FRAC_PI_2, PI};

use egui::{pos2, vec2, Align2, Color32, Id, Order, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use ogpaper_core::Brush;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Pen,
    Marker,
    Highlighter,
    Eraser,
    Hand,
    /// Eyedropper: take the color of the ink under the finger.
    Picker,
}

const TOOLS: [Tool; 6] = [
    Tool::Pen,
    Tool::Marker,
    Tool::Highlighter,
    Tool::Picker,
    Tool::Eraser,
    Tool::Hand,
];

impl Tool {
    pub fn brush(self) -> Option<Brush> {
        match self {
            Tool::Pen => Some(Brush::Pen),
            Tool::Marker => Some(Brush::Marker),
            Tool::Highlighter => Some(Brush::Highlighter),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Tool::Pen => "Pen",
            Tool::Marker => "Marker",
            Tool::Highlighter => "Highlighter",
            Tool::Eraser => "Eraser",
            Tool::Hand => "Pan",
            Tool::Picker => "Picker",
        }
    }
}

/// Per-brush settings; width is in screen pixels at the zoom you draw at.
#[derive(Clone, Copy)]
pub struct InkSettings {
    pub color: Color32,
    pub width: f32,
    /// Pen only: width follows pressure.
    pub pressure: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Menu {
    None,
    Tools,
    Colors,
    Settings,
}

pub struct UiState {
    pub tool: Tool,
    /// The ink tool the picker hands its color to (the last one used).
    pub last_ink: Tool,
    /// Picker loupe while dragging: position (points) and the color under it.
    pub pick_preview: Option<(Pos2, Option<Color32>)>,
    /// Hue kept while the color is grey, so the dial remembers it.
    pub dial_hue: f32,
    pub pen: InkSettings,
    pub marker: InkSettings,
    pub highlighter: InkSettings,
    pub menu: Menu,
    // Read-only status, filled in by the app each frame.
    pub file_name: String,
    pub zoom_log10: f64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub strokes: usize,
    pub message: Option<String>,
    pub touch_ui: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            tool: Tool::Pen,
            last_ink: Tool::Pen,
            pick_preview: None,
            dial_hue: 0.0,
            pen: InkSettings {
                color: Color32::from_rgb(28, 28, 36),
                width: 3.0,
                pressure: true,
            },
            marker: InkSettings {
                color: Color32::from_rgb(30, 90, 200),
                width: 8.0,
                pressure: false,
            },
            highlighter: InkSettings {
                color: Color32::from_rgb(255, 214, 0),
                width: 22.0,
                pressure: false,
            },
            menu: Menu::None,
            file_name: "Untitled".into(),
            zoom_log10: 0.0,
            can_undo: false,
            can_redo: false,
            strokes: 0,
            message: None,
            touch_ui: false,
        }
    }
}

impl UiState {
    /// Settings of the brush the current tool draws with.
    pub fn ink(&mut self) -> Option<&mut InkSettings> {
        ink_of(self, self.tool)
    }

    pub fn menu_open(&self) -> bool {
        self.menu != Menu::None
    }
}

fn ink_of(st: &mut UiState, t: Tool) -> Option<&mut InkSettings> {
    match t {
        Tool::Pen => Some(&mut st.pen),
        Tool::Marker => Some(&mut st.marker),
        Tool::Highlighter => Some(&mut st.highlighter),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Undo,
    Redo,
    New,
    Open,
    SaveAs,
    Home,
}

/// Inner ring of the color dial: the basics.
const BASIC: [Color32; 8] = [
    Color32::from_rgb(28, 28, 36),
    Color32::from_rgb(210, 40, 60),
    Color32::from_rgb(240, 130, 20),
    Color32::from_rgb(250, 210, 30),
    Color32::from_rgb(20, 140, 80),
    Color32::from_rgb(30, 90, 200),
    Color32::from_rgb(130, 60, 190),
    Color32::from_rgb(250, 250, 250),
];

/// Second ring: greys, earth tones, pastels.
const EXTRA: [Color32; 12] = [
    Color32::from_rgb(90, 90, 100),
    Color32::from_rgb(160, 160, 168),
    Color32::from_rgb(120, 70, 40),
    Color32::from_rgb(220, 60, 150),
    Color32::from_rgb(255, 150, 170),
    Color32::from_rgb(255, 200, 140),
    Color32::from_rgb(170, 200, 60),
    Color32::from_rgb(0, 160, 170),
    Color32::from_rgb(120, 200, 255),
    Color32::from_rgb(20, 40, 110),
    Color32::from_rgb(190, 160, 240),
    Color32::from_rgb(110, 20, 40),
];

const HIGHLIGHT: [Color32; 6] = [
    Color32::from_rgb(255, 214, 0),
    Color32::from_rgb(120, 230, 90),
    Color32::from_rgb(255, 120, 200),
    Color32::from_rgb(90, 200, 255),
    Color32::from_rgb(255, 160, 60),
    Color32::from_rgb(190, 140, 255),
];

const FACE: Color32 = Color32::from_rgb(252, 251, 248);
const INKY: Color32 = Color32::from_rgb(45, 45, 55);
const EDGE: Color32 = Color32::from_rgb(200, 198, 190);
const ACCENT: Color32 = Color32::from_rgb(200, 40, 90);

/// Layout, in points, scaled for touch.
struct Geo {
    /// Radius of the two main buttons.
    r: f32,
    tool: Pos2,
    color: Pos2,
    undo: Pos2,
    redo: Pos2,
}

fn geo(ctx: &egui::Context, touch: bool) -> Geo {
    let screen = ctx.content_rect();
    let r = if touch { 30.0 } else { 24.0 };
    let m = if touch { 18.0 } else { 16.0 };
    let tool = pos2(screen.right() - m - r, screen.bottom() - m - r);
    let color = tool - vec2(0.0, 2.0 * r + 14.0);
    let small = r * 0.72;
    // Redo sits next to the tool button, undo to its left.
    let redo = tool - vec2(r + 14.0 + small, r - small);
    let undo = redo - vec2(2.0 * small + 10.0, 0.0);
    Geo {
        r,
        tool,
        color,
        undo,
        redo,
    }
}

/// Draw the UI; returns actions for the app to perform.
pub fn draw(ctx: &egui::Context, st: &mut UiState) -> Vec<Action> {
    let mut actions = Vec::new();
    let g = geo(ctx, st.touch_ui);
    let t = ctx.animate_bool_with_time(Id::new("tools_open"), st.menu == Menu::Tools, 0.12);
    let c = ctx.animate_bool_with_time(Id::new("colors_open"), st.menu == Menu::Colors, 0.12);

    // The controls layer covers exactly the buttons plus any open fan, so
    // "pointer over UI" is right and the rest of the screen stays canvas.
    let sq = |c: Pos2, r: f32| Rect::from_center_size(c, Vec2::splat(2.0 * r));
    let small = g.r * 0.72;
    let mut bbox = sq(g.tool, g.r)
        .union(sq(g.color, g.r))
        .union(sq(g.undo, small))
        .union(sq(g.redo, small));
    if t > 0.0 {
        let reach = g.r * (5.4 + 0.9 * (TOOLS.len() as f32 - 5.0)) * t + g.r * 1.2;
        bbox = bbox.union(Rect::from_min_max(
            g.tool - vec2(reach, reach + 16.0),
            g.tool + vec2(g.r, g.r),
        ));
    }
    let bbox = bbox.expand(4.0);
    let screen = ctx.content_rect();
    egui::Area::new(Id::new("controls"))
        .order(Order::Foreground)
        .fixed_pos(bbox.min)
        .show(ctx, |ui| {
            ui.set_clip_rect(screen);
            ui.allocate_exact_size(bbox.size(), Sense::hover());
            let p = ui.painter().clone();

            // ---- tool fan: a quarter circle up and left of the tool button
            if t > 0.0 {
                let n = TOOLS.len();
                let radius = g.r * (5.4 + 0.9 * (n as f32 - 5.0)) * t;
                for (i, &tool) in TOOLS.iter().enumerate() {
                    let a = PI + FRAC_PI_2 * (i as f32 / (n - 1) as f32);
                    let pc = g.tool + Vec2::angled(a) * radius;
                    let rr = g.r * 0.9 * t.max(0.3);
                    let resp = ui.interact(
                        Rect::from_center_size(pc, Vec2::splat(2.0 * rr)),
                        Id::new(("tool", i)),
                        Sense::click(),
                    );
                    let selected = st.tool == tool;
                    disc(
                        &p,
                        pc,
                        rr,
                        if resp.hovered() { Color32::WHITE } else { FACE },
                        selected,
                    );
                    let ic = ink_of(st, tool).map(|i| i.color).unwrap_or(INKY);
                    tool_icon(&p, pc, rr, tool, ic);
                    if t > 0.9 {
                        // Label on the outer side of the circle, away from its neighbours.
                        let out = Vec2::angled(a);
                        label(&p, pc + out * (rr + 16.0) + vec2(0.0, 2.0), tool.name());
                    }
                    if resp.clicked() {
                        if selected && tool.brush().is_some() {
                            st.menu = Menu::Settings;
                        } else {
                            st.tool = tool;
                            if tool.brush().is_some() {
                                st.last_ink = tool;
                            }
                            st.menu = Menu::None;
                        }
                    }
                }
            }

            // ---- main buttons
            let tool_resp = ui.interact(
                Rect::from_center_size(g.tool, Vec2::splat(2.0 * g.r)),
                Id::new("tool_btn"),
                Sense::click(),
            );
            disc(&p, g.tool, g.r, FACE, st.menu == Menu::Tools);
            let cur_color = st.ink().map(|i| i.color).unwrap_or(INKY);
            tool_icon(&p, g.tool, g.r, st.tool, cur_color);
            if tool_resp.clicked() {
                st.menu = if st.menu == Menu::Tools {
                    Menu::None
                } else {
                    Menu::Tools
                };
            }
            if (tool_resp.long_touched() || tool_resp.secondary_clicked())
                && st.tool.brush().is_some()
            {
                st.menu = Menu::Settings;
            }

            // While the tool fan is open, its items use the space of the other buttons.
            let fan_open = st.menu == Menu::Tools;
            if st.tool.brush().is_some() && !fan_open {
                let resp = ui.interact(
                    Rect::from_center_size(g.color, Vec2::splat(2.0 * g.r)),
                    Id::new("color_btn"),
                    Sense::click(),
                );
                disc(&p, g.color, g.r, FACE, st.menu == Menu::Colors);
                p.circle_filled(g.color, g.r * 0.62, cur_color);
                p.circle_stroke(g.color, g.r * 0.62, Stroke::new(1.0, EDGE));
                if resp.clicked() {
                    st.menu = if st.menu == Menu::Colors {
                        Menu::None
                    } else {
                        Menu::Colors
                    };
                }
            }

            // ---- undo / redo
            let undo_row = if st.menu == Menu::None || st.menu == Menu::Settings {
                vec![(g.undo, false, st.can_undo), (g.redo, true, st.can_redo)]
            } else {
                vec![]
            };
            for (pc, redo, enabled) in undo_row {
                let resp = ui.interact(
                    Rect::from_center_size(pc, Vec2::splat(2.0 * small)),
                    Id::new(("ur", redo)),
                    Sense::click(),
                );
                disc(&p, pc, small, FACE, false);
                undo_icon(
                    &p,
                    pc,
                    small,
                    redo,
                    if enabled {
                        INKY
                    } else {
                        Color32::from_gray(190)
                    },
                );
                if enabled && resp.clicked() {
                    actions.push(if redo { Action::Redo } else { Action::Undo });
                }
            }
        });

    if c > 0.0 {
        color_dial(ctx, st, c);
    }

    // ---- tool settings popover, beside the buttons
    if st.menu == Menu::Settings {
        let tool = st.tool;
        let touch = st.touch_ui;
        egui::Area::new(Id::new("settings"))
            .order(Order::Foreground)
            .anchor(
                Align2::RIGHT_BOTTOM,
                vec2(-(g.r * 2.0 + 30.0), -(g.r * 2.0 + 30.0)),
            )
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(if touch { 240.0 } else { 210.0 });
                    if touch {
                        ui.style_mut().spacing.interact_size.y = 36.0;
                        ui.style_mut().spacing.slider_width = 180.0;
                    }
                    let Some(ink) = ink_of(st, tool) else { return };
                    let mut done = false;
                    ui.horizontal(|ui| {
                        ui.strong(tool.name());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            done = ui.button("Done").clicked();
                        });
                    });
                    // Live preview of the stroke.
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 60.0), Sense::hover());
                    let w = ink.width.min(52.0);
                    let a = if tool == Tool::Highlighter { 140 } else { 255 };
                    let col = Color32::from_rgba_unmultiplied(
                        ink.color.r(),
                        ink.color.g(),
                        ink.color.b(),
                        a,
                    );
                    let pts: Vec<Pos2> = (0..24)
                        .map(|i| {
                            let x = i as f32 / 23.0;
                            pos2(
                                rect.left() + 18.0 + x * (rect.width() - 36.0),
                                rect.center().y + (x * 6.0).sin() * 10.0,
                            )
                        })
                        .collect();
                    ui.painter().add(Shape::line(pts, Stroke::new(w, col)));
                    let max = if tool == Tool::Highlighter {
                        80.0
                    } else {
                        48.0
                    };
                    ui.add(
                        egui::Slider::new(&mut ink.width, 0.5..=max)
                            .logarithmic(true)
                            .text("size")
                            .suffix(" px"),
                    );
                    if tool == Tool::Pen {
                        ui.checkbox(&mut ink.pressure, "Width follows pen pressure");
                    }
                    let [r, g2, b, _] = ink.color.to_array();
                    let mut c3 = [r, g2, b];
                    ui.horizontal(|ui| {
                        ui.label("Custom color");
                        if ui.color_edit_button_srgb(&mut c3).changed() {
                            ink.color = Color32::from_rgb(c3[0], c3[1], c3[2]);
                        }
                    });
                    if done {
                        st.menu = Menu::None;
                    }
                });
            });
    }

    // ---- ☰ menu, top left
    egui::Area::new(Id::new("file"))
        .anchor(Align2::LEFT_TOP, vec2(10.0, 10.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.menu_button(if st.touch_ui { "  ☰  " } else { "☰" }, |ui| {
                    ui.label(format!(
                        "{} · zoom 10^{:.1} · {} strokes",
                        st.file_name, st.zoom_log10, st.strokes
                    ));
                    ui.separator();
                    if ui.button("New canvas  (Ctrl+N)").clicked() {
                        actions.push(Action::New);
                    }
                    if ui.button("Open…  (Ctrl+O)").clicked() {
                        actions.push(Action::Open);
                    }
                    if ui.button("Save as…  (Ctrl+S)").clicked() {
                        actions.push(Action::SaveAs);
                    }
                    ui.separator();
                    if ui.button("Back to start  (Home)").clicked() {
                        actions.push(Action::Home);
                    }
                });
            });
        });

    // Picker loupe: a disc above the finger showing the color under it.
    if let Some((at, col)) = st.pick_preview {
        let r = if st.touch_ui { 34.0 } else { 26.0 };
        let c = at - vec2(0.0, r + 40.0);
        let p = ctx.layer_painter(egui::LayerId::new(Order::Tooltip, Id::new("loupe")));
        p.circle_filled(c + vec2(0.0, 2.0), r + 4.0, Color32::from_black_alpha(50));
        p.circle_filled(c, r + 3.0, FACE);
        match col {
            Some(col) => {
                p.circle_filled(c, r, col);
            }
            None => {
                p.circle_stroke(c, r * 0.8, Stroke::new(2.0, EDGE));
                p.line_segment(
                    [c + vec2(-r * 0.5, r * 0.5), c + vec2(r * 0.5, -r * 0.5)],
                    Stroke::new(2.0, EDGE),
                );
            }
        }
        p.circle_stroke(at, 6.0, Stroke::new(2.0, INKY));
    }

    if let Some(msg) = &st.message {
        egui::Area::new(Id::new("msg"))
            .anchor(Align2::CENTER_TOP, vec2(0.0, 14.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(msg);
                });
            });
    }
    actions
}

fn label(p: &egui::Painter, at: Pos2, text: &str) {
    let font = egui::FontId::proportional(11.0);
    let galley = p.layout_no_wrap(text.to_string(), font, INKY);
    let rect = Rect::from_center_size(at, galley.size() + vec2(8.0, 2.0));
    p.rect_filled(rect, 4.0, Color32::from_white_alpha(220));
    p.galley(rect.min + vec2(4.0, 1.0), galley, INKY);
}

fn disc(p: &egui::Painter, c: Pos2, r: f32, fill: Color32, active: bool) {
    p.circle_filled(c + vec2(0.0, 1.5), r + 1.0, Color32::from_black_alpha(30));
    p.circle_filled(c, r, fill);
    p.circle_stroke(
        c,
        r,
        Stroke::new(
            if active { 2.5 } else { 1.0 },
            if active { ACCENT } else { EDGE },
        ),
    );
}

/// Simple drawn icons, sized relative to the button radius.
fn tool_icon(p: &egui::Painter, c: Pos2, r: f32, tool: Tool, ink: Color32) {
    let s = r * 0.5;
    let d = vec2(s, -s) * 0.9;
    match tool {
        Tool::Pen => {
            p.line_segment([c - d, c + d * 0.55], Stroke::new(r * 0.12, INKY));
            let tip = c - d;
            p.add(Shape::convex_polygon(
                vec![
                    tip,
                    tip + vec2(r * 0.22, -r * 0.08),
                    tip + vec2(r * 0.08, -r * 0.22),
                ],
                ink,
                Stroke::NONE,
            ));
            p.circle_filled(tip, r * 0.07, ink);
        }
        Tool::Marker => {
            p.line_segment([c - d, c + d], Stroke::new(r * 0.36, ink));
        }
        Tool::Highlighter => {
            p.rect_filled(
                Rect::from_center_size(c, vec2(s * 2.0, s * 0.9)),
                3.0,
                ink.gamma_multiply(0.75),
            );
        }
        Tool::Eraser => {
            let pts = [
                c + vec2(-s, s * 0.2),
                c + vec2(-s * 0.2, s),
                c + vec2(s, -s * 0.2),
                c + vec2(s * 0.2, -s),
            ];
            p.add(Shape::convex_polygon(
                pts.to_vec(),
                Color32::from_rgb(240, 150, 170),
                Stroke::new(1.5, INKY),
            ));
            p.line_segment(
                [
                    pts[0] + (pts[3] - pts[0]) * 0.45,
                    pts[1] + (pts[2] - pts[1]) * 0.45,
                ],
                Stroke::new(1.5, INKY),
            );
        }
        Tool::Picker => eyedropper(p, c, r, ink),
        Tool::Hand => {
            let st = Stroke::new(r * 0.09, INKY);
            for dir in [
                vec2(1.0, 0.0),
                vec2(-1.0, 0.0),
                vec2(0.0, 1.0),
                vec2(0.0, -1.0),
            ] {
                let tip = c + dir * s;
                p.line_segment([c, tip], st);
                let n = vec2(-dir.y, dir.x);
                p.line_segment([tip, tip - dir * s * 0.35 + n * s * 0.3], st);
                p.line_segment([tip, tip - dir * s * 0.35 - n * s * 0.3], st);
            }
        }
    }
}

/// A curved arrow: counter-clockwise for undo, mirrored for redo.
fn undo_icon(p: &egui::Painter, c: Pos2, r: f32, redo: bool, col: Color32) {
    let s = r * 0.45;
    let flip = if redo { -1.0 } else { 1.0 };
    let pts: Vec<Pos2> = (0..=12)
        .map(|i| {
            let a = -PI * 0.1 + PI * 1.1 * (i as f32 / 12.0);
            c + vec2(-a.cos() * s * flip, a.sin() * s * 0.9 + s * 0.2)
        })
        .collect();
    let head = pts[0];
    p.add(Shape::line(pts, Stroke::new(r * 0.12, col)));
    p.line_segment(
        [head, head + vec2(s * 0.55 * flip, -s * 0.05)],
        Stroke::new(r * 0.12, col),
    );
    p.line_segment(
        [head, head + vec2(s * 0.1 * flip, -s * 0.55)],
        Stroke::new(r * 0.12, col),
    );
}

/// The color dial: current color in the middle, two rings of presets, a
/// continuous hue ring outside, and saturation / brightness bars below.
fn color_dial(ctx: &egui::Context, st: &mut UiState, open: f32) {
    use egui::ecolor::HsvaGamma;
    let screen = ctx.content_rect();
    let bars_h = 118.0;
    let r_out = ((screen.width().min(screen.height() - bars_h - 60.0)) * 0.46).clamp(120.0, 230.0)
        * (0.85 + 0.15 * open);
    let center = pos2(screen.center().x, screen.center().y - bars_h * 0.5);
    let bar_w = (r_out * 2.0).min(screen.width() - 32.0);
    let bars_top = center.y + r_out + 18.0;
    let area = Rect::from_min_max(
        center - vec2(r_out, r_out),
        pos2(center.x + r_out, bars_top + bars_h),
    )
    .union(Rect::from_center_size(
        pos2(center.x, bars_top + bars_h * 0.5),
        vec2(bar_w, bars_h),
    ))
    .expand(8.0);
    let hl = st.tool == Tool::Highlighter;
    let mut dial_hue = st.dial_hue;
    let mut close = false;
    let mut pick = false;
    egui::Area::new(Id::new("color_dial"))
        .order(Order::Foreground)
        .fixed_pos(area.min)
        .show(ctx, |ui| {
            ui.set_clip_rect(screen);
            ui.allocate_exact_size(area.size(), Sense::hover());
            let p = ui.painter().clone();
            let Some(ink) = ink_of(st, st.tool) else {
                return;
            };
            let mut hsv = HsvaGamma::from(ink.color);
            if hsv.s > 0.02 {
                dial_hue = hsv.h;
            } else {
                hsv.h = dial_hue;
            }

            // Backdrop.
            p.circle_filled(
                center + vec2(0.0, 3.0),
                r_out + 6.0,
                Color32::from_black_alpha(40),
            );
            p.circle_filled(center, r_out + 4.0, FACE);

            // Outer hue ring: a continuous rainbow (tap or drag).
            let (h_in, h_out) = (r_out * 0.80, r_out);
            let seg = 96;
            let mut mesh = egui::Mesh::default();
            for i in 0..=seg {
                let t = i as f32 / seg as f32;
                let a = t * 2.0 * PI - FRAC_PI_2;
                let col: Color32 = HsvaGamma {
                    h: t,
                    s: 1.0,
                    v: 1.0,
                    a: 1.0,
                }
                .into();
                mesh.colored_vertex(center + Vec2::angled(a) * h_in, col);
                mesh.colored_vertex(center + Vec2::angled(a) * h_out, col);
                if i > 0 {
                    let k = 2 * i as u32;
                    mesh.add_triangle(k - 2, k - 1, k);
                    mesh.add_triangle(k - 1, k, k + 1);
                }
            }
            p.add(Shape::mesh(mesh));
            // Hue marker.
            let ha = hsv.h * 2.0 * PI - FRAC_PI_2;
            let hm = center + Vec2::angled(ha) * (h_in + h_out) * 0.5;
            p.circle_stroke(hm, (h_out - h_in) * 0.55, Stroke::new(3.0, Color32::WHITE));
            p.circle_stroke(hm, (h_out - h_in) * 0.55 + 1.5, Stroke::new(1.0, INKY));
            let ring = ui.interact(
                Rect::from_center_size(center, Vec2::splat(2.0 * h_out)),
                Id::new("hue_ring"),
                Sense::click_and_drag(),
            );
            if let Some(pos) = ring.interact_pointer_pos() {
                let d = pos - center;
                if (ring.clicked() || ring.dragged()) && d.length() >= h_in - 6.0 {
                    let mut h = (d.y.atan2(d.x) + FRAC_PI_2) / (2.0 * PI);
                    if h < 0.0 {
                        h += 1.0;
                    }
                    dial_hue = h;
                    let s = if hsv.s < 0.15 { 0.9 } else { hsv.s };
                    let v = if hsv.v < 0.25 { 0.9 } else { hsv.v };
                    ink.color = HsvaGamma { h, s, v, a: 1.0 }.into();
                }
            }

            // Two rings of presets.
            let presets: [&[Color32]; 2] = if hl {
                [&HIGHLIGHT, &[]]
            } else {
                [&BASIC, &EXTRA]
            };
            for (k, cols) in presets.iter().enumerate() {
                let radius = r_out * if k == 0 { 0.40 } else { 0.63 };
                let rr = r_out * if k == 0 { 0.105 } else { 0.085 };
                for (i, &col) in cols.iter().enumerate() {
                    let a = i as f32 / cols.len() as f32 * 2.0 * PI - FRAC_PI_2;
                    let pc = center + Vec2::angled(a) * radius;
                    let resp = ui.interact(
                        Rect::from_center_size(pc, Vec2::splat(2.0 * rr)),
                        Id::new(("preset", k, i)),
                        Sense::click(),
                    );
                    p.circle_filled(pc, rr, col);
                    p.circle_stroke(pc, rr, Stroke::new(1.0, EDGE));
                    if ink.color == col {
                        p.circle_stroke(pc, rr + 3.5, Stroke::new(2.5, ACCENT));
                    }
                    if resp.clicked() {
                        ink.color = col;
                        close = true;
                    }
                }
            }

            // Centre: current color; tap to close.
            let rc = r_out * 0.2;
            let centre = ui.interact(
                Rect::from_center_size(center, Vec2::splat(2.0 * rc)),
                Id::new("dial_centre"),
                Sense::click(),
            );
            p.circle_filled(center, rc, ink.color);
            p.circle_stroke(center, rc, Stroke::new(1.5, EDGE));
            if centre.clicked() {
                close = true;
            }

            // Eyedropper, just outside the dial's lower-left edge.
            let ep = center + vec2(-r_out * 0.86, r_out * 0.86);
            let er = (r_out * 0.12).max(20.0);
            let eresp = ui.interact(
                Rect::from_center_size(ep, Vec2::splat(2.0 * er)),
                Id::new("dial_picker"),
                Sense::click(),
            );
            disc(&p, ep, er, FACE, false);
            eyedropper(&p, ep, er, INKY);
            if eresp.clicked() {
                pick = true;
            }

            // Saturation and brightness bars.
            let hsv = {
                let mut h = HsvaGamma::from(ink.color);
                if h.s <= 0.02 {
                    h.h = dial_hue;
                }
                h
            };
            for (row, name) in ["Saturation", "Brightness"].iter().enumerate() {
                let y = bars_top + 22.0 + row as f32 * 60.0;
                let rect =
                    Rect::from_center_size(pos2(center.x, y + 8.0), vec2(bar_w - 24.0, 22.0));
                p.rect_filled(
                    Rect::from_min_max(
                        pos2(rect.left() - 8.0, y - 20.0),
                        pos2(rect.right() + 8.0, rect.bottom() + 8.0),
                    ),
                    10.0,
                    FACE,
                );
                p.text(
                    pos2(rect.left(), y - 14.0),
                    Align2::LEFT_BOTTOM,
                    *name,
                    egui::FontId::proportional(12.0),
                    INKY,
                );
                let mut mesh = egui::Mesh::default();
                let n = 24;
                for i in 0..=n {
                    let t = i as f32 / n as f32;
                    let c: Color32 = if row == 0 {
                        HsvaGamma { s: t, ..hsv }
                    } else {
                        HsvaGamma { v: t, ..hsv }
                    }
                    .into();
                    let x = rect.left() + t * rect.width();
                    mesh.colored_vertex(pos2(x, rect.top()), c);
                    mesh.colored_vertex(pos2(x, rect.bottom()), c);
                    if i > 0 {
                        let k = 2 * i as u32;
                        mesh.add_triangle(k - 2, k - 1, k);
                        mesh.add_triangle(k - 1, k, k + 1);
                    }
                }
                p.add(Shape::mesh(mesh));
                p.rect_stroke(rect, 4.0, Stroke::new(1.0, EDGE), egui::StrokeKind::Outside);
                let val = if row == 0 { hsv.s } else { hsv.v };
                let mx = rect.left() + val * rect.width();
                p.rect_stroke(
                    Rect::from_center_size(pos2(mx, rect.center().y), vec2(8.0, 30.0)),
                    3.0,
                    Stroke::new(2.5, INKY),
                    egui::StrokeKind::Outside,
                );
                let resp = ui.interact(
                    rect.expand2(vec2(10.0, 10.0)),
                    Id::new(("bar", row)),
                    Sense::click_and_drag(),
                );
                if let Some(pos) = resp.interact_pointer_pos() {
                    if resp.clicked() || resp.dragged() {
                        let t = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                        let next = if row == 0 {
                            HsvaGamma { s: t, ..hsv }
                        } else {
                            HsvaGamma { v: t, ..hsv }
                        };
                        ink.color = next.into();
                    }
                }
            }
        });
    st.dial_hue = dial_hue;
    if close {
        st.menu = Menu::None;
    }
    if pick {
        if st.tool.brush().is_some() {
            st.last_ink = st.tool;
        }
        st.tool = Tool::Picker;
        st.menu = Menu::None;
    }
}

/// A pipette: glass tube tilted up-right with a rubber bulb.
fn eyedropper(p: &egui::Painter, c: Pos2, r: f32, tip: Color32) {
    let s = r * 0.5;
    let a = c + vec2(-s * 0.95, s * 0.95);
    let b = c + vec2(s * 0.35, -s * 0.35);
    p.line_segment([a, b], Stroke::new(r * 0.16, INKY));
    p.line_segment([a, a + (b - a) * 0.25], Stroke::new(r * 0.12, tip));
    p.circle_filled(c + vec2(s * 0.6, -s * 0.6), r * 0.2, INKY);
    let n = vec2(1.0, 1.0).normalized() * r * 0.22;
    p.line_segment([b - n, b + n], Stroke::new(r * 0.1, INKY));
}
