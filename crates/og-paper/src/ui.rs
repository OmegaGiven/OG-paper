// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Screen-size-independent controls: a tool button in the bottom-right
//! corner that fans out into a radial menu, small undo/redo buttons, a tool
//! panel bottom-left (width, pressure, color dial) and a settings button
//! top-right whose fan holds the canvas commands. Icons are drawn, not taken from a font.

use std::f32::consts::{FRAC_PI_2, PI};

use egui::{pos2, vec2, Align2, Color32, Id, Order, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use ogpaper_core::{Brush, Dash};

use crate::font::{self, Align, ALIGNS};
use crate::hotbar::{self, Preset};
use crate::objects::TextStyle;
use crate::shapes::{
    self, ArrowType, FillStyle, Head, ShapeKind, ShapeStyle, Sloppiness, ARROW_TYPES, FILLS, HEADS,
    SHAPES, SLOPPINESS,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Pen,
    Marker,
    Highlighter,
    Eraser,
    Hand,
    /// Eyedropper: take the color of the ink under the finger.
    Picker,
    /// Rectangles, ellipses, ... lines and arrows (the kind is picked in the panel).
    Shapes,
    Text,
    /// Select, move, resize, rotate, restyle and reorder what is drawn.
    Select,
}

/// The tool fan, inner ring first.
const TOOLS: [Tool; 9] = [
    Tool::Pen,
    Tool::Marker,
    Tool::Highlighter,
    Tool::Eraser,
    Tool::Select,
    Tool::Shapes,
    Tool::Text,
    Tool::Picker,
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
            Tool::Shapes => "Shapes",
            Tool::Text => "Text",
            Tool::Select => "Select",
        }
    }

    /// Tools with settings in the tool panel.
    fn has_panel(self) -> bool {
        self.brush().is_some() || matches!(self, Tool::Shapes | Tool::Text | Tool::Select)
    }
}

/// Per-brush settings; width is in screen pixels at the zoom you draw at.
#[derive(Clone, Copy)]
pub struct InkSettings {
    pub color: Color32,
    pub width: f32,
    /// Pen only: width follows pressure.
    pub pressure: bool,
    pub dash: Dash,
    /// 0..=255.
    pub opacity: u8,
}

impl InkSettings {
    /// The color with the opacity applied.
    pub fn rgba(&self) -> [u8; 4] {
        let [r, g, b, a] = self.color.to_array();
        [r, g, b, ((a as u32 * self.opacity as u32) / 255) as u8]
    }
}

/// What is selected, for the panel (filled in by the app).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SelKind {
    #[default]
    None,
    Ink,
    Shapes,
    /// Texts and tables.
    Text,
    Images,
    Mixed,
}

/// Style edits made in the panel for the current selection. The app fills
/// these from the selection; when the panel changes them, the app restyles.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct SelStyle {
    pub kind: SelKind,
    pub count: usize,
    pub ink: Option<InkSettings>,
    pub shape: ShapeStyle,
    pub text: TextStyle,
    /// Pictures' opacity.
    pub opacity: u8,
}

/// Which color the shape panel's dial edits.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ColorTarget {
    #[default]
    Stroke,
    Fill,
    /// Stroke and fill share one color.
    Both,
}

/// Screen-space drawing the app asks the UI to show over the canvas.
#[derive(Default, Clone)]
pub struct Overlay {
    /// Polylines: points (points units), width, color, closed-and-filled.
    pub lines: Vec<(Vec<Pos2>, f32, Color32, bool)>,
    /// Selection box corners (clockwise from top-left) and whether to show
    /// handles (resize corners + rotate knob above the top edge).
    pub sel_box: Option<([Pos2; 4], bool)>,
    /// Marquee rectangle.
    pub marquee: Option<Rect>,
}

impl PartialEq for InkSettings {
    fn eq(&self, o: &Self) -> bool {
        self.color == o.color
            && self.width == o.width
            && self.pressure == o.pressure
            && self.dash == o.dash
            && self.opacity == o.opacity
    }
}

impl std::fmt::Debug for InkSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ink({:?}, {})", self.color, self.width)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Menu {
    None,
    Tools,
    /// The settings button's fan (canvas commands).
    App,
}

/// Commands in the settings fan. The app decides which ones exist here
/// (the web page adds bookmarks, the timeline, full screen and the tour).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub enum AppItem {
    New,
    Open,
    Save,
    Home,
    /// Put a picture from a file on the canvas.
    Picture,
    Bookmarks,
    Timeline,
    FullScreen,
    Tour,
}

impl AppItem {
    fn name(self) -> &'static str {
        match self {
            AppItem::New => "New canvas",
            AppItem::Open => "Open",
            AppItem::Save => "Save copy",
            AppItem::Home => "Home",
            AppItem::Picture => "Insert picture",
            AppItem::Bookmarks => "Bookmarks",
            AppItem::Timeline => "Timeline",
            AppItem::FullScreen => "Full screen",
            AppItem::Tour => "Tour",
        }
    }

    fn action(self) -> Action {
        match self {
            AppItem::New => Action::New,
            AppItem::Open => Action::Open,
            AppItem::Save => Action::SaveAs,
            AppItem::Home => Action::Home,
            AppItem::Picture => Action::Picture,
            AppItem::Bookmarks => Action::Bookmarks,
            AppItem::Timeline => Action::Timeline,
            AppItem::FullScreen => Action::FullScreen,
            AppItem::Tour => Action::Tour,
        }
    }
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
    /// What the settings fan offers, in order.
    pub app_items: Vec<AppItem>,
    /// Timeline view open (its fan item shows as active).
    pub timeline_on: bool,
    /// The tool panel (bottom left) is expanded; collapsed it is one small button.
    /// `None` until the first frame, which picks by screen width.
    pub panel_open: Option<bool>,
    pub shape: ShapeStyle,
    /// Shape outline width, screen pixels at the zoom drawn at.
    pub shape_width: f32,
    pub color_target: ColorTarget,
    pub text: TextStyle,
    /// Text cap height, screen pixels at the zoom typed at.
    pub text_size: f32,
    /// The selection, for the panel (app fills it; panel edits it).
    pub sel: SelStyle,
    /// Font names egui can draw (the picker shows each name in its font).
    pub egui_fonts: std::collections::HashSet<String>,
    /// Text being typed (native editor): the screen point and the text.
    pub text_edit: Option<(Pos2, String)>,
    pub overlay: Overlay,
    /// Saved tools: the quick bar along the bottom and the inventory.
    pub hotbar: Vec<Option<Preset>>,
    pub inventory: Vec<Option<Preset>>,
    pub bag_open: bool,
    /// A saved tool picked up in the inventory, to put in another slot.
    pub held: Option<Preset>,
    /// The slots changed: the app saves them.
    pub presets_dirty: bool,
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
                dash: Dash::Solid,
                opacity: 255,
            },
            marker: InkSettings {
                color: Color32::from_rgb(30, 90, 200),
                width: 8.0,
                pressure: false,
                dash: Dash::Solid,
                opacity: 255,
            },
            highlighter: InkSettings {
                color: Color32::from_rgb(255, 214, 0),
                width: 22.0,
                pressure: false,
                dash: Dash::Solid,
                opacity: 255,
            },
            menu: Menu::None,
            file_name: "Untitled".into(),
            zoom_log10: 0.0,
            can_undo: false,
            can_redo: false,
            strokes: 0,
            message: None,
            touch_ui: false,
            app_items: vec![
                AppItem::New,
                AppItem::Open,
                AppItem::Save,
                AppItem::Picture,
                AppItem::Home,
            ],
            timeline_on: false,
            panel_open: None,
            shape: ShapeStyle::default(),
            shape_width: 3.0,
            color_target: ColorTarget::Stroke,
            text: TextStyle::default(),
            text_size: 24.0,
            sel: SelStyle::default(),
            egui_fonts: Default::default(),
            text_edit: None,
            overlay: Overlay::default(),
            hotbar: vec![None; hotbar::BAR],
            inventory: vec![None; hotbar::INVENTORY],
            bag_open: false,
            held: None,
            presets_dirty: false,
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

    /// The current tool and its settings.
    pub fn preset(&self) -> Preset {
        let mut p = Preset::tool(self.tool);
        match self.tool {
            Tool::Pen => p.ink = Some(self.pen),
            Tool::Marker => p.ink = Some(self.marker),
            Tool::Highlighter => p.ink = Some(self.highlighter),
            Tool::Shapes => p.shape = Some((self.shape, self.shape_width)),
            Tool::Text => p.text = Some((self.text, self.text_size)),
            _ => {}
        }
        p
    }

    /// Switch to a saved tool.
    pub fn apply(&mut self, p: &Preset) {
        self.tool = p.tool;
        if let Some(ink) = p.ink {
            if let Some(dst) = ink_of(self, p.tool) {
                *dst = ink;
            }
        }
        if p.tool.brush().is_some() {
            self.last_ink = p.tool;
        }
        if let Some((sh, w)) = p.shape {
            self.shape = sh;
            self.shape_width = w;
        }
        if let Some((tx, size)) = p.text {
            self.text = tx;
            self.text_size = size;
        }
        self.menu = Menu::None;
    }

    /// Use quick-bar slot `i` (0-based), if it holds a tool.
    pub fn use_slot(&mut self, i: usize) -> bool {
        match self.hotbar.get(i).copied().flatten() {
            Some(p) => {
                self.apply(&p);
                true
            }
            None => false,
        }
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
    // Selection.
    Duplicate,
    Delete,
    ToFront,
    ToBack,
    FlipH,
    FlipV,
    EditText,
    /// Load a font file of your own.
    AddFont,
    /// Insert a picture from a file.
    Picture,
    TextDone,
    TextCancel,
    // Web page panels.
    Bookmarks,
    Timeline,
    FullScreen,
    Tour,
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
    undo: Pos2,
    redo: Pos2,
    /// The settings button, top right.
    app: Pos2,
}

fn geo(ctx: &egui::Context, touch: bool) -> Geo {
    let screen = ctx.content_rect();
    let r = if touch { 30.0 } else { 24.0 };
    let m = if touch { 18.0 } else { 16.0 };
    let tool = pos2(screen.right() - m - r, screen.bottom() - m - r);
    let small = r * 0.72;
    // Redo sits next to the tool button, undo to its left.
    let redo = tool - vec2(r + 14.0 + small, r - small);
    let undo = redo - vec2(2.0 * small + 10.0, 0.0);
    let app = pos2(screen.right() - m - r, screen.top() + m + r);
    Geo {
        app,
        r,
        tool,
        undo,
        redo,
    }
}

/// Draw the UI; returns actions for the app to perform.
pub fn draw(ctx: &egui::Context, st: &mut UiState) -> Vec<Action> {
    let mut actions = Vec::new();
    let g = geo(ctx, st.touch_ui);
    let t = ctx.animate_bool_with_time(Id::new("tools_open"), st.menu == Menu::Tools, 0.12);

    // The controls layer covers exactly the buttons plus any open fan, so
    // "pointer over UI" is right and the rest of the screen stays canvas.
    let sq = |c: Pos2, r: f32| Rect::from_center_size(c, Vec2::splat(2.0 * r));
    let small = g.r * 0.72;
    let mut bbox = sq(g.tool, g.r)
        .union(sq(g.undo, small))
        .union(sq(g.redo, small));
    let tool_slots = ring_slots(TOOLS.len(), g.r);
    if t > 0.0 {
        let reach = fan_reach(&tool_slots, g.r) * t + g.r * 1.2;
        bbox = bbox.union(Rect::from_min_max(
            g.tool - vec2(reach + 30.0, reach + 16.0),
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

            // ---- tool fan: quarter-circle rings up and left of the tool button
            if t > 0.0 {
                for (i, &tool) in TOOLS.iter().enumerate() {
                    let (radius, frac) = tool_slots[i];
                    let a = PI + FRAC_PI_2 * frac;
                    let pc = g.tool + Vec2::angled(a) * radius * t;
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
                    let ic = tool_color(st, tool);
                    tool_icon(&p, pc, rr, tool, ic);
                    if t > 0.9 {
                        // Label on the outer side of the circle, away from its neighbours.
                        let out = Vec2::angled(a);
                        label(&p, pc + out * (rr + 16.0) + vec2(0.0, 2.0), tool.name());
                    }
                    if resp.clicked() {
                        if selected && tool.has_panel() {
                            // Picking the current brush again brings its panel back.
                            st.panel_open = Some(true);
                            st.menu = Menu::None;
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
            let cur_color = tool_color(st, st.tool);
            tool_icon(&p, g.tool, g.r, st.tool, cur_color);
            if tool_resp.clicked() {
                st.menu = if st.menu == Menu::Tools {
                    Menu::None
                } else {
                    Menu::Tools
                };
            }
            if (tool_resp.long_touched() || tool_resp.secondary_clicked()) && st.tool.has_panel() {
                st.panel_open = Some(true);
                st.menu = Menu::None;
            }

            // ---- undo / redo
            let undo_row = if st.menu == Menu::None {
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

    paint_overlay(ctx, &st.overlay, st.touch_ui);
    quick_bar(ctx, st, &g);
    tool_panel(ctx, st, &mut actions);
    text_editor(ctx, st, &mut actions);

    app_menu(ctx, st, &g, &mut actions);

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

/// Which set of slots.
#[derive(Clone, Copy, PartialEq)]
enum Slots {
    Bar,
    Inventory,
}

/// The quick bar (bottom middle) and, when its bag button is on, the
/// inventory above it. Tap a slot to use its tool; tap an empty one to save
/// the current tool there; long-press or right-click for more. In the
/// inventory, tap a slot to pick its tool up and another to put it down.
fn quick_bar(ctx: &egui::Context, st: &mut UiState, g: &Geo) {
    let touch = st.touch_ui;
    let screen = ctx.content_rect();
    let m = if touch { 18.0 } else { 16.0 };
    let gap = 4.0;
    let mut s = if touch { 44.0 } else { 38.0 };
    // Bottom row, between the panel button and undo / redo, if it fits;
    // else a row above them.
    let small = g.r * 0.72;
    let right = g.undo.x - small - 12.0;
    let left = screen.left() + m + if touch { 64.0 } else { 52.0 };
    let full = (hotbar::BAR + 1) as f32 * (s + gap);
    let (n, y, cx) = if full <= right - left {
        let cx = screen
            .center()
            .x
            .clamp(left + full * 0.5, right - full * 0.5);
        (hotbar::BAR, g.tool.y, cx)
    } else {
        let avail = screen.width() - 2.0 * m;
        if avail < full {
            // Fewer slots rather than slots too small to tap.
            s = (avail / (hotbar::BAR + 1) as f32 - gap).max(if touch { 40.0 } else { 30.0 });
        }
        let fit = ((avail / (s + gap)) as usize).saturating_sub(1);
        let n = fit.clamp(3, hotbar::BAR);
        (n, g.tool.y - g.r - 14.0 - s * 0.5, screen.center().x)
    };
    let row = (n + 1) as f32 * (s + gap) - gap;
    let x0 = cx - row * 0.5;
    let current = st.preset();
    let mut fx = SlotFx {
        current,
        changed: false,
        say: None,
    };

    egui::Area::new(Id::new("quick_bar"))
        .order(Order::Middle)
        .fixed_pos(pos2(x0, y - s * 0.5))
        .show(ctx, |ui| {
            ui.set_clip_rect(screen);
            let (all, _) = ui.allocate_exact_size(vec2(row, s), Sense::hover());
            for i in 0..n {
                let r =
                    Rect::from_min_size(all.min + vec2(i as f32 * (s + gap), 0.0), Vec2::splat(s));
                slot(ui, st, r, Slots::Bar, i, &mut fx);
            }
            // The bag: opens the inventory.
            let r = Rect::from_min_size(all.min + vec2(n as f32 * (s + gap), 0.0), Vec2::splat(s));
            let resp = ui.interact(r, Id::new("bag"), Sense::click());
            let p = ui.painter();
            p.rect_filled(
                r,
                8.0,
                if resp.hovered() || st.bag_open {
                    Color32::WHITE
                } else {
                    FACE
                },
            );
            p.rect_stroke(
                r,
                8.0,
                Stroke::new(
                    if st.bag_open { 2.5 } else { 1.0 },
                    if st.bag_open { ACCENT } else { EDGE },
                ),
                egui::StrokeKind::Inside,
            );
            bag_icon(p, r.center(), s * 0.5);
            if resp.clicked() {
                st.bag_open = !st.bag_open;
            }
            resp.on_hover_text(if st.bag_open {
                "Close the inventory"
            } else {
                "Saved tools: open the inventory"
            });
        });

    if st.bag_open {
        let cols = n.max(6);
        let rows = hotbar::INVENTORY.div_ceil(cols);
        let w = cols as f32 * (s + gap) - gap;
        egui::Area::new(Id::new("inventory"))
            .order(Order::Foreground)
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(pos2(cx.clamp(screen.left() + w * 0.5 + m, screen.right() - w * 0.5 - m), y - s * 0.5 - 10.0))
            .show(ctx, |ui| {
                ui.style_mut().visuals = egui::Visuals::light();
                egui::Frame::new()
                    .fill(FACE)
                    .stroke(Stroke::new(1.0, EDGE))
                    .corner_radius(12.0)
                    .inner_margin(10.0)
                    .shadow(egui::Shadow {
                        offset: [0, 2],
                        blur: 10,
                        spread: 0,
                        color: Color32::from_black_alpha(40),
                    })
                    .show(ui, |ui| {
                        ui.set_width(w);
                        ui.horizontal(|ui| {
                            ui.strong("Saved tools");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Close").clicked() {
                                    st.bag_open = false;
                                }
                                if ui
                                    .button("+ Save current")
                                    .on_hover_text("Keep the current tool and its settings")
                                    .clicked()
                                {
                                    match st.inventory.iter().position(|x| x.is_none()) {
                                        Some(k) => {
                                            st.inventory[k] = Some(current);
                                            fx.changed = true;
                                        }
                                        None => fx.say = Some("The inventory is full".into()),
                                    }
                                }
                            });
                        });
                        ui.label(
                            egui::RichText::new(
                                "Tap a tool to pick it up, then a slot (here or in the bar) to put it down.",
                            )
                            .small()
                            .weak(),
                        );
                        ui.add_space(4.0);
                        let (all, _) = ui.allocate_exact_size(
                            vec2(w, rows as f32 * (s + gap) - gap),
                            Sense::hover(),
                        );
                        for i in 0..hotbar::INVENTORY {
                            let (cx, cy) = (i % cols, i / cols);
                            let r = Rect::from_min_size(
                                all.min + vec2(cx as f32 * (s + gap), cy as f32 * (s + gap)),
                                Vec2::splat(s),
                            );
                            slot(ui, st, r, Slots::Inventory, i, &mut fx);
                        }
                        // Bin: drop the held tool.
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let bin = ui.add_enabled(st.held.is_some(), egui::Button::new("🗑 Throw away"));
                            if bin.clicked() {
                                st.held = None;
                                fx.changed = true;
                            }
                        });
                    });
            });
    } else if let Some(h) = st.held.take() {
        // Closed while holding a tool: put it back in the first free slot.
        if let Some(k) = st.inventory.iter().position(|x| x.is_none()) {
            st.inventory[k] = Some(h);
        } else if let Some(k) = st.hotbar.iter().position(|x| x.is_none()) {
            st.hotbar[k] = Some(h);
        }
        fx.changed = true;
    }

    // The held tool follows the pointer.
    if let (Some(h), Some(at)) = (st.held, ctx.pointer_hover_pos()) {
        let p = ctx.layer_painter(egui::LayerId::new(Order::Tooltip, Id::new("held")));
        let c = at + vec2(s * 0.3, s * 0.3);
        p.circle_filled(c, s * 0.45, Color32::from_white_alpha(230));
        p.circle_stroke(c, s * 0.45, Stroke::new(1.5, ACCENT));
        preset_icon(&p, c, s * 0.45, &h, &st.egui_fonts);
    }
    if fx.changed {
        st.presets_dirty = true;
    }
    if let Some(m) = fx.say {
        st.message = Some(m);
    }
}

/// What tapping slots did this frame.
struct SlotFx {
    current: Preset,
    changed: bool,
    say: Option<String>,
}

fn slot_mut(st: &mut UiState, which: Slots, i: usize) -> &mut Option<Preset> {
    match which {
        Slots::Bar => &mut st.hotbar[i],
        Slots::Inventory => &mut st.inventory[i],
    }
}

/// One slot: draws it and handles taps.
fn slot(ui: &mut egui::Ui, st: &mut UiState, rect: Rect, which: Slots, i: usize, fx: &mut SlotFx) {
    let current = fx.current;
    let resp = ui.interact(rect, Id::new(("slot", which as u8, i)), Sense::click());
    let item = *slot_mut(st, which, i);
    let p = ui.painter();
    let on = !st.bag_open && item.is_some_and(|it| it == current);
    p.rect_filled(
        rect,
        8.0,
        if resp.hovered() { Color32::WHITE } else { FACE },
    );
    p.rect_stroke(
        rect,
        8.0,
        Stroke::new(if on { 2.5 } else { 1.0 }, if on { ACCENT } else { EDGE }),
        egui::StrokeKind::Inside,
    );
    if which == Slots::Bar {
        p.text(
            rect.left_top() + vec2(5.0, 3.0),
            Align2::LEFT_TOP,
            format!("{}", i + 1),
            egui::FontId::proportional(9.0),
            Color32::from_gray(150),
        );
    }
    if let Some(it) = &item {
        preset_icon(p, rect.center(), rect.width() * 0.5, it, &st.egui_fonts);
    }
    if resp.clicked() {
        if st.bag_open {
            // Pick up / put down / swap.
            let held = st.held.take();
            st.held = std::mem::replace(slot_mut(st, which, i), held);
            fx.changed = true;
        } else if let Some(it) = item {
            st.apply(&it);
        } else {
            *slot_mut(st, which, i) = Some(current);
            fx.changed = true;
            fx.say = Some(format!("Saved {} to slot {}", describe(&current), i + 1));
        }
    }
    let tip = match &item {
        Some(it) => describe(it),
        None if st.bag_open => "Empty".into(),
        None => "Empty: tap to save the current tool here".into(),
    };
    let resp = resp.on_hover_text(tip);
    resp.context_menu(|ui| {
        if ui.button("Save the current tool here").clicked() {
            *slot_mut(st, which, i) = Some(current);
            fx.changed = true;
            ui.close();
        }
        if item.is_some() && ui.button("Empty this slot").clicked() {
            *slot_mut(st, which, i) = None;
            fx.changed = true;
            ui.close();
        }
    });
}

/// "Pen, 3 px", "Shapes: Arrow", "Text: Lora 24 px"...
fn describe(p: &Preset) -> String {
    if let Some(i) = p.ink {
        let [r, g, b, _] = i.color.to_array();
        let dash = match i.dash {
            Dash::Solid => "",
            Dash::Dashed => ", dashed",
            Dash::Dotted => ", dotted",
        };
        return format!(
            "{}, {:.0} px, #{r:02x}{g:02x}{b:02x}{dash}",
            p.tool.name(),
            i.width
        );
    }
    if let Some((sh, w)) = p.shape {
        return format!("{}: {}, {:.0} px", p.tool.name(), sh.kind.name(), w);
    }
    if let Some((tx, size)) = p.text {
        return format!("Text: {}, {:.0} px", font::name_of(tx.font), size);
    }
    p.tool.name().to_string()
}

/// A saved tool's picture: the tool in its color, with a width bar for pens.
fn preset_icon(
    p: &egui::Painter,
    c: Pos2,
    r: f32,
    it: &Preset,
    fonts: &std::collections::HashSet<String>,
) {
    if let Some(i) = it.ink {
        let col = i.color;
        tool_icon(p, c - vec2(0.0, r * 0.12), r * 0.85, it.tool, col);
        let w = (i.width * 0.35).clamp(1.5, r * 0.3);
        let [cr, cg, cb, _] = col.to_array();
        let a = if it.tool == Tool::Highlighter {
            150
        } else {
            i.opacity
        };
        let y = c.y + r * 0.62;
        let st = Stroke::new(w, Color32::from_rgba_unmultiplied(cr, cg, cb, a));
        match i.dash {
            Dash::Solid => {
                p.line_segment([pos2(c.x - r * 0.55, y), pos2(c.x + r * 0.55, y)], st);
            }
            _ => {
                for k in 0..3 {
                    let x = c.x - r * 0.55 + k as f32 * r * 0.42;
                    p.line_segment([pos2(x, y), pos2(x + r * 0.22, y)], st);
                }
            }
        }
        return;
    }
    if let Some((sh, _)) = it.shape {
        if sh.fill_style != FillStyle::None && !sh.kind.is_linear() {
            p.circle_filled(c + vec2(r * 0.45, r * 0.45), r * 0.16, c32(sh.fill));
        }
        shape_icon(p, c, r * 0.6, sh.kind, c32(sh.stroke));
        return;
    }
    if let Some((tx, _)) = it.text {
        let name = font::name_of(tx.font);
        let fam = if fonts.contains(&name) {
            egui::FontFamily::Name(name.as_str().into())
        } else {
            egui::FontFamily::Proportional
        };
        p.text(
            c,
            Align2::CENTER_CENTER,
            "Aa",
            egui::FontId::new(r * 0.8, fam),
            c32(tx.color),
        );
        return;
    }
    tool_icon(p, c, r * 0.9, it.tool, INKY);
}

/// A satchel: the inventory button.
fn bag_icon(p: &egui::Painter, c: Pos2, r: f32) {
    let s = r * 0.42;
    let st = Stroke::new((r * 0.08).max(1.2), INKY);
    let pts = |v: &[Vec2]| v.iter().map(|q| c + *q * s).collect::<Vec<_>>();
    p.add(Shape::closed_line(
        pts(&[
            vec2(-0.95, -0.35),
            vec2(0.95, -0.35),
            vec2(0.8, 0.95),
            vec2(-0.8, 0.95),
        ]),
        st,
    ));
    p.add(Shape::line(
        pts(&[
            vec2(-0.45, -0.35),
            vec2(-0.4, -0.85),
            vec2(0.4, -0.85),
            vec2(0.45, -0.35),
        ]),
        st,
    ));
    p.add(Shape::line(pts(&[vec2(-0.95, 0.15), vec2(0.95, 0.15)]), st));
    p.rect_filled(
        Rect::from_center_size(c + vec2(0.0, 0.15) * s, Vec2::splat(s * 0.35)),
        1.0,
        INKY,
    );
}

/// The tool panel, bottom left: the current tool's settings (or the
/// selection's), open until it is collapsed to a small button (which pops it
/// back out). New per-tool options (textures, presets, ...) go here as
/// sections.
fn tool_panel(ctx: &egui::Context, st: &mut UiState, actions: &mut Vec<Action>) {
    let tool = st.tool;
    if !tool.has_panel() || (tool == Tool::Select && st.sel.count == 0) {
        return;
    }
    let touch = st.touch_ui;
    let m = if touch { 18.0 } else { 16.0 };
    let screen = ctx.content_rect();
    // Bottom left: in thumb reach on phones, out of the way on desktops.
    let corner = vec2(m, -m);
    // Open by default where there is room; tucked away on phones.
    let open = *st.panel_open.get_or_insert(screen.width() >= 700.0);

    if !open {
        // Collapsed: a small round button showing the tool and its color.
        let r = if touch { 24.0 } else { 19.0 };
        let col = tool_color(st, tool);
        egui::Area::new(Id::new("tool_panel_btn"))
            .order(Order::Foreground)
            .anchor(Align2::LEFT_BOTTOM, corner)
            .show(ctx, |ui| {
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::splat(2.0 * r + 4.0), Sense::click());
                let c = rect.center();
                let p = ui.painter();
                disc(
                    p,
                    c,
                    r,
                    if resp.hovered() { Color32::WHITE } else { FACE },
                    false,
                );
                sliders_icon(p, c, r, col);
                if resp.clicked() {
                    st.panel_open = Some(true);
                }
                resp.on_hover_text(format!("{} settings", tool.name()));
            });
        return;
    }

    let mut dial_hue = st.dial_hue;
    let mut pick = false;
    egui::Area::new(Id::new("tool_panel"))
        .order(Order::Foreground)
        .anchor(Align2::LEFT_BOTTOM, corner)
        .show(ctx, |ui| {
            // Light like the rest of the controls, whatever the system theme.
            ui.style_mut().visuals = egui::Visuals::light();
            let frame = egui::Frame::new()
                .fill(FACE)
                .stroke(Stroke::new(1.0, EDGE))
                .corner_radius(12.0)
                .inner_margin(12.0)
                .shadow(egui::Shadow {
                    offset: [0, 2],
                    blur: 10,
                    spread: 0,
                    color: Color32::from_black_alpha(40),
                });
            frame.show(ui, |ui| {
                let w = if touch { 248.0 } else { 220.0 };
                ui.set_width(w);
                if touch {
                    ui.style_mut().spacing.interact_size.y = 34.0;
                    ui.style_mut().spacing.slider_width = w - 70.0;
                } else {
                    ui.style_mut().spacing.slider_width = w - 64.0;
                }
                let title = if tool == Tool::Select {
                    format!("Selection ({})", st.sel.count)
                } else {
                    tool.name().to_string()
                };
                ui.horizontal(|ui| {
                    ui.strong(title);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (rect, resp) = ui.allocate_exact_size(
                            Vec2::splat(if touch { 30.0 } else { 22.0 }),
                            Sense::click(),
                        );
                        let p = ui.painter();
                        if resp.hovered() {
                            p.circle_filled(
                                rect.center(),
                                rect.width() * 0.5,
                                Color32::from_gray(232),
                            );
                        }
                        collapse_icon(p, rect.center(), rect.width() * 0.5);
                        if resp.clicked() {
                            st.panel_open = Some(false);
                        }
                        resp.on_hover_text("Hide (tap the button to bring it back)");
                    });
                });
                let max_h = (screen.height() - 2.0 * m - 60.0).max(160.0);
                egui::ScrollArea::vertical()
                    .max_height(max_h)
                    .min_scrolled_height(max_h)
                    .show(ui, |ui| {
                        pick = match tool {
                            Tool::Pen | Tool::Marker | Tool::Highlighter => {
                                let ink = ink_of(st, tool).expect("brush");
                                ink_section(ui, ink, tool, touch, &mut dial_hue, true)
                            }
                            Tool::Shapes => shape_section(
                                ui,
                                &mut st.shape,
                                Some(&mut st.shape_width),
                                &mut st.color_target,
                                touch,
                                &mut dial_hue,
                            ),
                            Tool::Text => text_section(
                                ui,
                                &mut st.text,
                                Some(&mut st.text_size),
                                touch,
                                &mut dial_hue,
                                &st.egui_fonts,
                                actions,
                            ),
                            Tool::Select => {
                                select_actions(ui, st.sel.kind, actions);
                                match st.sel.kind {
                                    SelKind::Ink => match st.sel.ink.as_mut() {
                                        Some(ink) => ink_section(
                                            ui,
                                            ink,
                                            Tool::Marker,
                                            touch,
                                            &mut dial_hue,
                                            false,
                                        ),
                                        None => false,
                                    },
                                    SelKind::Shapes => shape_section(
                                        ui,
                                        &mut st.sel.shape,
                                        None,
                                        &mut st.color_target,
                                        touch,
                                        &mut dial_hue,
                                    ),
                                    SelKind::Text => text_section(
                                        ui,
                                        &mut st.sel.text,
                                        None,
                                        touch,
                                        &mut dial_hue,
                                        &st.egui_fonts,
                                        actions,
                                    ),
                                    SelKind::Images => {
                                        opacity_slider(ui, &mut st.sel.opacity);
                                        false
                                    }
                                    _ => false,
                                }
                            }
                            _ => false,
                        };
                    });
            });
        });
    st.dial_hue = dial_hue;
    if pick {
        // The dial's eyedropper: pick a color from the canvas, then come back.
        st.last_ink = tool;
        st.tool = Tool::Picker;
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).small().weak());
}

/// A row of option chips; returns true if the choice changed.
fn chips<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    items: &[T],
    cur: &mut T,
    size: f32,
    tip: impl Fn(T) -> String,
    draw: impl Fn(&egui::Painter, Pos2, f32, T),
) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for &it in items {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
            let on = *cur == it;
            let p = ui.painter();
            let bg = if on {
                Color32::from_rgb(252, 228, 236)
            } else if resp.hovered() {
                Color32::from_gray(232)
            } else {
                Color32::from_gray(242)
            };
            p.rect_filled(rect, 6.0, bg);
            if on {
                p.rect_stroke(
                    rect,
                    6.0,
                    Stroke::new(1.5, ACCENT),
                    egui::StrokeKind::Inside,
                );
            }
            draw(p, rect.center(), size * 0.36, it);
            if resp.clicked() && !on {
                *cur = it;
                changed = true;
            }
            resp.on_hover_text(tip(it));
        }
    });
    changed
}

fn text_chip(p: &egui::Painter, c: Pos2, text: &str) {
    p.text(
        c,
        Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(12.0),
        INKY,
    );
}

fn opacity_slider(ui: &mut egui::Ui, o: &mut u8) {
    heading(ui, "Opacity");
    let mut v = *o as f32 / 255.0 * 100.0;
    if ui
        .add(
            egui::Slider::new(&mut v, 5.0..=100.0)
                .suffix(" %")
                .fixed_decimals(0),
        )
        .changed()
    {
        *o = (v / 100.0 * 255.0).round() as u8;
    }
}

fn dash_chips(ui: &mut egui::Ui, dash: &mut Dash, sz: f32) -> bool {
    heading(ui, "Stroke style");
    chips(
        ui,
        &[Dash::Solid, Dash::Dashed, Dash::Dotted],
        dash,
        sz,
        |d| format!("{d:?}"),
        |p, c, r, d| line_icon(p, c, r, d, Sloppiness::Architect, ArrowType::Straight),
    )
}

/// Pen, marker and highlighter settings (also used for selected ink).
fn ink_section(
    ui: &mut egui::Ui,
    ink: &mut InkSettings,
    tool: Tool,
    touch: bool,
    dial_hue: &mut f32,
    preview: bool,
) -> bool {
    let sz = if touch { 34.0 } else { 28.0 };
    if preview {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
        let pw = ink.width.min(40.0);
        let [r, g, b, a] = ink.rgba();
        let a = if tool == Tool::Highlighter { 140 } else { a };
        let col = Color32::from_rgba_unmultiplied(r, g, b, a);
        let pts: Vec<Pos2> = (0..24)
            .map(|i| {
                let x = i as f32 / 23.0;
                pos2(
                    rect.left() + 14.0 + x * (rect.width() - 28.0),
                    rect.center().y + (x * 6.0).sin() * 8.0,
                )
            })
            .collect();
        ui.painter().add(Shape::line(pts, Stroke::new(pw, col)));
    }
    heading(ui, "Stroke width");
    let max = if tool == Tool::Highlighter {
        80.0
    } else {
        48.0
    };
    ui.add(
        egui::Slider::new(&mut ink.width, 0.5..=max)
            .logarithmic(true)
            .suffix(" px"),
    );
    if tool == Tool::Pen {
        ui.checkbox(&mut ink.pressure, "Width follows pressure");
    }
    if tool != Tool::Highlighter {
        dash_chips(ui, &mut ink.dash, sz);
        opacity_slider(ui, &mut ink.opacity);
    }
    heading(ui, "Color");
    color_dial(
        ui,
        &mut ink.color,
        tool == Tool::Highlighter,
        dial_hue,
        touch,
    )
}

/// Shape settings (also used for selected shapes, without the width).
fn shape_section(
    ui: &mut egui::Ui,
    sh: &mut ShapeStyle,
    width: Option<&mut f32>,
    target: &mut ColorTarget,
    touch: bool,
    dial_hue: &mut f32,
) -> bool {
    let sz = if touch { 34.0 } else { 28.0 };
    heading(ui, "Shape");
    chips(
        ui,
        &SHAPES,
        &mut sh.kind,
        sz,
        |k| k.name().into(),
        |p, c, r, k| shape_icon(p, c, r, k, INKY),
    );
    if matches!(sh.kind, ShapeKind::Star | ShapeKind::Polygon) {
        heading(
            ui,
            if sh.kind == ShapeKind::Star {
                "Points"
            } else {
                "Sides"
            },
        );
        let mut n = sh.sides as u32;
        if ui.add(egui::Slider::new(&mut n, 3..=12)).changed() {
            sh.sides = n as u8;
        }
    }
    if let Some(w) = width {
        heading(ui, "Stroke width");
        ui.add(
            egui::Slider::new(w, 0.5..=24.0)
                .logarithmic(true)
                .suffix(" px"),
        );
    }
    dash_chips(ui, &mut sh.dash, sz);
    heading(ui, "Sloppiness");
    chips(
        ui,
        &SLOPPINESS,
        &mut sh.sloppiness,
        sz,
        |s| s.name().into(),
        |p, c, r, s| line_icon(p, c, r, Dash::Solid, s, ArrowType::Curved),
    );
    if !sh.kind.is_linear() {
        if sh.kind != ShapeKind::Ellipse {
            heading(ui, "Edges");
            chips(
                ui,
                &[false, true],
                &mut sh.round,
                sz,
                |r| if r { "Round" } else { "Sharp" }.into(),
                |p, c, r, round| {
                    let st = Stroke::new(1.8, INKY);
                    let (bl, tr) = (c + vec2(-r, r), c + vec2(r, -r));
                    if round {
                        let rad = r * 1.1;
                        let mut pts = vec![bl];
                        for i in (0..=8).rev() {
                            let t = i as f32 / 8.0 * FRAC_PI_2;
                            pts.push(pos2(
                                c.x - r + rad * (1.0 - t.sin()),
                                c.y - r + rad * (1.0 - t.cos()),
                            ));
                        }
                        pts.push(tr);
                        p.add(Shape::line(pts, st));
                    } else {
                        p.add(Shape::line(vec![bl, pos2(bl.x, tr.y), tr], st));
                    }
                },
            );
        }
        heading(ui, "Fill");
        chips(
            ui,
            &FILLS,
            &mut sh.fill_style,
            sz,
            |f| f.name().into(),
            fill_icon,
        );
    }
    if sh.kind.is_linear() {
        heading(ui, "Line type");
        chips(
            ui,
            &ARROW_TYPES,
            &mut sh.arrow,
            sz,
            |a| a.name().into(),
            |p, c, r, a| line_icon(p, c, r, Dash::Solid, Sloppiness::Architect, a),
        );
    }
    if sh.kind == ShapeKind::Arrow {
        heading(ui, "Start");
        chips(
            ui,
            &HEADS,
            &mut sh.start,
            sz,
            |h| h.name().into(),
            |p, c, r, h| head_icon(p, c, r, h, true),
        );
        heading(ui, "End");
        chips(
            ui,
            &HEADS,
            &mut sh.end,
            sz,
            |h| h.name().into(),
            |p, c, r, h| head_icon(p, c, r, h, false),
        );
    }
    opacity_slider(ui, &mut sh.opacity);
    let fillable = !sh.kind.is_linear() && sh.fill_style != FillStyle::None;
    if fillable {
        heading(ui, "Color");
        ui.horizontal(|ui| {
            ui.selectable_value(target, ColorTarget::Stroke, "Stroke");
            ui.selectable_value(target, ColorTarget::Fill, "Fill");
            ui.selectable_value(target, ColorTarget::Both, "Both")
                .on_hover_text("Stroke and fill in one color");
        });
    } else {
        heading(ui, "Stroke color");
    }
    let edit_fill = fillable && *target == ColorTarget::Fill;
    let slot = if edit_fill {
        &mut sh.fill
    } else {
        &mut sh.stroke
    };
    let mut c = c32(*slot);
    let pick = color_dial(ui, &mut c, false, dial_hue, touch);
    *slot = u32c(c);
    if fillable && *target == ColorTarget::Both {
        sh.fill = sh.stroke;
    }
    pick
}

/// Text settings (also used for selected text, without the size).
fn text_section(
    ui: &mut egui::Ui,
    tx: &mut TextStyle,
    size: Option<&mut f32>,
    touch: bool,
    dial_hue: &mut f32,
    egui_fonts: &std::collections::HashSet<String>,
    actions: &mut Vec<Action>,
) -> bool {
    let sz = if touch { 34.0 } else { 28.0 };
    heading(ui, "Font");
    font_picker(ui, tx, touch, egui_fonts, actions);
    if let Some(size) = size {
        heading(ui, "Size");
        let mut pick = [16.0f32, 24.0, 36.0, 56.0]
            .iter()
            .position(|&v| (v - *size).abs() < 0.5)
            .unwrap_or(9);
        let before = pick;
        chips(
            ui,
            &[0usize, 1, 2, 3],
            &mut pick,
            sz,
            |i| ["Small", "Medium", "Large", "Very large"][i].into(),
            |p, c, _, i| text_chip(p, c, ["S", "M", "L", "XL"][i]),
        );
        if pick != before && pick < 4 {
            *size = [16.0, 24.0, 36.0, 56.0][pick];
        }
        ui.add(
            egui::Slider::new(size, 6.0..=160.0)
                .logarithmic(true)
                .suffix(" px"),
        );
    }
    heading(ui, "Align");
    chips(
        ui,
        &ALIGNS,
        &mut tx.align,
        sz,
        |a| format!("{a:?}"),
        |p, c, r, a| {
            for (i, w) in [1.0f32, 0.6, 0.85].iter().enumerate() {
                let y = c.y - r * 0.6 + i as f32 * r * 0.6;
                let len = 2.0 * r * w;
                let x0 = match a {
                    Align::Left => c.x - r,
                    Align::Center => c.x - len * 0.5,
                    Align::Right => c.x + r - len,
                };
                p.line_segment([pos2(x0, y), pos2(x0 + len, y)], Stroke::new(1.6, INKY));
            }
        },
    );
    opacity_slider(ui, &mut tx.opacity);
    heading(ui, "Color");
    let mut c = c32(tx.color);
    let pick = color_dial(ui, &mut c, false, dial_hue, touch);
    tx.color = u32c(c);
    pick
}

/// Fonts by style, each name shown in its own font, plus "Add your own".
fn font_picker(
    ui: &mut egui::Ui,
    tx: &mut TextStyle,
    touch: bool,
    egui_fonts: &std::collections::HashSet<String>,
    actions: &mut Vec<Action>,
) {
    const ORDER: [&str; 8] = [
        "Hand-drawn",
        "Marker",
        "Sans",
        "Serif",
        "Mono",
        "Display",
        "Yours",
        "Single-line",
    ];
    let rank = |c: &str| ORDER.iter().position(|o| *o == c).unwrap_or(ORDER.len());
    let mut fonts = font::list();
    fonts.sort_by_key(|f| (rank(&f.category), f.id));
    let row = if touch { 26.0 } else { 20.0 };
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, EDGE))
        .corner_radius(8.0)
        .inner_margin(4.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("fonts")
                .max_height(row * 7.5)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let mut cat = String::new();
                    for f in fonts {
                        if f.category != cat {
                            cat = f.category.clone();
                            ui.label(egui::RichText::new(&cat).small().weak());
                        }
                        let mut text =
                            egui::RichText::new(&f.name).size(if touch { 18.0 } else { 15.0 });
                        if egui_fonts.contains(&f.name) {
                            text = text.family(egui::FontFamily::Name(f.name.as_str().into()));
                        }
                        if !f.available {
                            text = text.weak();
                        }
                        let resp = ui
                            .add_sized(
                                vec2(ui.available_width(), row),
                                egui::Button::selectable(tx.font == f.id, text),
                            )
                            .on_hover_text(if f.available {
                                f.name.clone()
                            } else if f.category == "Not on this device" {
                                format!(
                                    "{}: not on this device (add it with \"Add your own font\")",
                                    f.name
                                )
                            } else {
                                format!("{}: loading…", f.name)
                            });
                        if resp.clicked() {
                            tx.font = f.id;
                        }
                    }
                });
        });
    if ui
        .button("+ Add your own font…")
        .on_hover_text("A TrueType (.ttf) or OpenType (.otf) file")
        .clicked()
    {
        actions.push(Action::AddFont);
    }
}

/// Buttons for what can be done with a selection.
fn select_actions(ui: &mut egui::Ui, kind: SelKind, actions: &mut Vec<Action>) {
    ui.horizontal_wrapped(|ui| {
        let items = [
            (Action::Duplicate, "Duplicate", "Ctrl+D"),
            (Action::Delete, "Delete", "Delete"),
            (Action::ToFront, "To front", "Bring to front"),
            (Action::ToBack, "To back", "Send to back"),
            (Action::FlipH, "Flip ↔", "Flip horizontally"),
            (Action::FlipV, "Flip ↕", "Flip vertically"),
        ];
        for (a, label, tip) in items {
            if ui.button(label).on_hover_text(tip).clicked() {
                actions.push(a);
            }
        }
        if kind == SelKind::Text && ui.button("Edit text").clicked() {
            actions.push(Action::EditText);
        }
    });
}

/// Selection box, marquee and shape previews, drawn over the canvas.
fn paint_overlay(ctx: &egui::Context, ov: &Overlay, touch: bool) {
    let p = ctx.layer_painter(egui::LayerId::new(Order::Background, Id::new("overlay")));
    for (pts, w, col, filled) in &ov.lines {
        if *filled {
            p.add(Shape::convex_polygon(pts.clone(), *col, Stroke::NONE));
        } else {
            p.add(Shape::line(pts.clone(), Stroke::new(*w, *col)));
        }
    }
    let blue = Color32::from_rgb(70, 110, 230);
    if let Some(r) = ov.marquee {
        p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(70, 110, 230, 24));
        p.rect_stroke(r, 0.0, Stroke::new(1.0, blue), egui::StrokeKind::Middle);
    }
    if let Some((c, handles)) = ov.sel_box {
        p.add(Shape::closed_line(c.to_vec(), Stroke::new(1.2, blue)));
        if handles {
            let hs = if touch { 7.0 } else { 5.0 };
            for q in c {
                p.rect_filled(
                    Rect::from_center_size(q, Vec2::splat(hs * 2.0)),
                    2.0,
                    Color32::WHITE,
                );
                p.rect_stroke(
                    Rect::from_center_size(q, Vec2::splat(hs * 2.0)),
                    2.0,
                    Stroke::new(1.2, blue),
                    egui::StrokeKind::Middle,
                );
            }
            let (top, knob) = rotate_knob(&c, touch);
            p.line_segment([top, knob], Stroke::new(1.0, blue));
            p.circle_filled(knob, hs + 1.0, Color32::WHITE);
            p.circle_stroke(knob, hs + 1.0, Stroke::new(1.2, blue));
        }
    }
}

/// Middle of a selection box's top edge and its rotate knob above it.
pub fn rotate_knob(c: &[Pos2; 4], touch: bool) -> (Pos2, Pos2) {
    let top = c[0] + (c[1] - c[0]) * 0.5;
    let up = (c[0] - c[3]).normalized();
    let up = if up.is_finite() { up } else { vec2(0.0, -1.0) };
    (top, top + up * if touch { 34.0 } else { 26.0 })
}

/// The native text editor (the web page shows its own, with the phone's
/// keyboard).
fn text_editor(ctx: &egui::Context, st: &mut UiState, actions: &mut Vec<Action>) {
    let Some((at, text)) = st.text_edit.as_mut() else {
        return;
    };
    egui::Area::new(Id::new("text_editor"))
        .order(Order::Foreground)
        .fixed_pos(*at)
        .show(ctx, |ui| {
            ui.style_mut().visuals = egui::Visuals::light();
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                let resp = ui.add(
                    egui::TextEdit::multiline(text)
                        .desired_rows(2)
                        .desired_width(240.0)
                        // Tab types a tab: tables are edited as tab-separated cells.
                        .lock_focus(true)
                        .hint_text("Type, then Done (Ctrl+Enter)"),
                );
                resp.request_focus();
                let done_key = ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
                let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                ui.horizontal(|ui| {
                    if ui.button("Done").clicked() || done_key {
                        actions.push(Action::TextDone);
                    }
                    if ui.button("Cancel").clicked() || esc {
                        actions.push(Action::TextCancel);
                    }
                });
            });
        });
}

/// Three slider lines with knobs; the middle knob shows the ink color.
fn sliders_icon(p: &egui::Painter, c: Pos2, r: f32, ink: Color32) {
    let s = r * 0.5;
    let line = Stroke::new((r * 0.09).max(1.2), INKY);
    for (i, k) in [(-1.0, 0.35), (0.0, -0.4), (1.0, 0.1)] {
        let y = c.y + i * s * 0.75;
        p.line_segment([pos2(c.x - s, y), pos2(c.x + s, y)], line);
        let kc = pos2(c.x + k * s, y);
        p.circle_filled(kc, r * 0.15, if i == 0.0 { ink } else { FACE });
        p.circle_stroke(kc, r * 0.15, line);
    }
}

/// A chevron pointing left: tuck the panel away into its corner.
fn collapse_icon(p: &egui::Painter, c: Pos2, r: f32) {
    let s = r * 0.45;
    let line = Stroke::new((r * 0.13).max(1.4), INKY);
    p.add(Shape::line(
        vec![
            c + vec2(s * 0.6, -s * 0.9),
            c + vec2(-s * 0.4, 0.0),
            c + vec2(s * 0.6, s * 0.9),
        ],
        line,
    ));
}

/// Where each of `n` fan items goes around a button of radius `r`: (ring
/// radius, position along the quarter circle 0..1). Rings fill from the
/// inside out, each holding as many items as fit along its arc, so a long
/// menu grows outward in layers instead of one huge curve.
fn ring_slots(n: usize, r: f32) -> Vec<(f32, f32)> {
    let rr = r * 0.9;
    let along = 2.0 * rr + 8.0;
    // Room between rings for the labels.
    let step = 2.0 * rr + 56.0;
    let mut radius = r * 4.4;
    let mut out = Vec::with_capacity(n);
    let mut left = n;
    while left > 0 {
        let cap = ((FRAC_PI_2 * radius / along).floor() as usize + 1).max(1);
        let m = cap.min(left);
        for i in 0..m {
            let frac = if m == 1 {
                0.5
            } else {
                i as f32 / (m - 1) as f32
            };
            out.push((radius, frac));
        }
        left -= m;
        radius += step;
    }
    out
}

fn fan_reach(slots: &[(f32, f32)], r: f32) -> f32 {
    slots.iter().map(|s| s.0).fold(0.0, f32::max) + r
}

/// The color a tool's icon shows.
fn tool_color(st: &mut UiState, tool: Tool) -> Color32 {
    match tool {
        Tool::Shapes => c32(st.shape.stroke),
        Tool::Text => c32(st.text.color),
        _ => ink_of(st, tool).map(|i| i.color).unwrap_or(INKY),
    }
}

fn c32(c: u32) -> Color32 {
    let [r, g, b, _] = c.to_le_bytes();
    Color32::from_rgb(r, g, b)
}

fn u32c(c: Color32) -> u32 {
    let [r, g, b, _] = c.to_array();
    u32::from_le_bytes([r, g, b, 255])
}

/// The settings button (top right) and its fan: a quarter circle down and
/// left of it, like the tool fan, with the zoom depth shown under it.
fn app_menu(ctx: &egui::Context, st: &mut UiState, g: &Geo, actions: &mut Vec<Action>) {
    let open = ctx.animate_bool_with_time(Id::new("app_open"), st.menu == Menu::App, 0.12);
    let items = st.app_items.clone();
    let slots = ring_slots(items.len(), g.r);
    let mut bbox = Rect::from_center_size(g.app, Vec2::splat(2.0 * g.r)).union(
        Rect::from_center_size(g.app + vec2(0.0, g.r + 12.0), vec2(2.0 * g.r + 24.0, 18.0)),
    );
    if open > 0.0 {
        let reach = fan_reach(&slots, g.r) * open + g.r * 1.2;
        bbox = bbox.union(Rect::from_min_max(
            g.app - vec2(reach + 50.0, g.r),
            g.app + vec2(g.r, reach + 24.0),
        ));
    }
    let bbox = bbox.expand(4.0);
    let screen = ctx.content_rect();
    egui::Area::new(Id::new("app_menu"))
        .order(Order::Foreground)
        .fixed_pos(bbox.min)
        .show(ctx, |ui| {
            ui.set_clip_rect(screen);
            ui.allocate_exact_size(bbox.size(), Sense::hover());
            let p = ui.painter().clone();
            if open > 0.0 {
                for (i, &item) in items.iter().enumerate() {
                    // From straight down (pi/2) round to straight left (pi).
                    let (radius, frac) = slots[i];
                    let a = FRAC_PI_2 + FRAC_PI_2 * frac;
                    let pc = g.app + Vec2::angled(a) * radius * open;
                    let rr = g.r * 0.9 * open.max(0.3);
                    let resp = ui.interact(
                        Rect::from_center_size(pc, Vec2::splat(2.0 * rr)),
                        Id::new(("app_item", i)),
                        Sense::click(),
                    );
                    let active = item == AppItem::Timeline && st.timeline_on;
                    disc(
                        &p,
                        pc,
                        rr,
                        if resp.hovered() { Color32::WHITE } else { FACE },
                        active,
                    );
                    app_icon(&p, pc, rr, item);
                    if open > 0.9 {
                        let out = Vec2::angled(a);
                        label(&p, pc + out * (rr + 18.0) + vec2(0.0, 2.0), item.name());
                    }
                    if resp.clicked() {
                        st.menu = Menu::None;
                        actions.push(item.action());
                    }
                }
            }
            let resp = ui.interact(
                Rect::from_center_size(g.app, Vec2::splat(2.0 * g.r)),
                Id::new("app_btn"),
                Sense::click(),
            );
            disc(&p, g.app, g.r, FACE, st.menu == Menu::App);
            gear_icon(&p, g.app, g.r);
            if resp.clicked() {
                st.menu = if st.menu == Menu::App {
                    Menu::None
                } else {
                    Menu::App
                };
            }
            // Zoom depth under the button (hidden while the fan is out).
            if open < 0.1 {
                let info = format!("10^{:.1}", st.zoom_log10 + 0.0);
                label_right(&p, pos2(g.app.x + g.r, g.app.y + g.r + 12.0), &info);
            }
        });
}

fn gear_icon(p: &egui::Painter, c: Pos2, r: f32) {
    let st = Stroke::new(r * 0.1, INKY);
    for i in 0..8 {
        let d = Vec2::angled(i as f32 * PI / 4.0);
        p.line_segment(
            [c + d * r * 0.36, c + d * r * 0.56],
            Stroke::new(r * 0.16, INKY),
        );
    }
    p.circle_stroke(c, r * 0.36, st);
    p.circle_filled(c, r * 0.31, FACE);
    p.circle_stroke(c, r * 0.34, st);
    p.circle_stroke(c, r * 0.13, st);
}

fn app_icon(p: &egui::Painter, c: Pos2, r: f32, item: AppItem) {
    let s = r * 0.42;
    let st = Stroke::new(r * 0.09, INKY);
    let line = |pts: &[Vec2]| {
        p.add(Shape::line(pts.iter().map(|v| c + *v * s).collect(), st));
    };
    match item {
        AppItem::New => {
            // A page with a folded corner and a plus.
            line(&[
                vec2(0.3, -1.0),
                vec2(-0.75, -1.0),
                vec2(-0.75, 1.0),
                vec2(0.75, 1.0),
                vec2(0.75, -0.55),
                vec2(0.3, -1.0),
                vec2(0.3, -0.55),
                vec2(0.75, -0.55),
            ]);
            line(&[vec2(0.0, -0.05), vec2(0.0, 0.65)]);
            line(&[vec2(-0.35, 0.3), vec2(0.35, 0.3)]);
        }
        AppItem::Open => {
            // A folder.
            line(&[
                vec2(-1.0, 0.8),
                vec2(-1.0, -0.75),
                vec2(-0.35, -0.75),
                vec2(-0.15, -0.5),
                vec2(0.85, -0.5),
                vec2(0.85, -0.2),
            ]);
            line(&[
                vec2(-1.0, 0.8),
                vec2(-0.65, -0.2),
                vec2(1.05, -0.2),
                vec2(0.7, 0.8),
                vec2(-1.0, 0.8),
            ]);
        }
        AppItem::Save => {
            line(&[vec2(0.0, -1.0), vec2(0.0, 0.45)]);
            line(&[vec2(-0.5, -0.05), vec2(0.0, 0.45), vec2(0.5, -0.05)]);
            line(&[vec2(-0.85, 0.95), vec2(0.85, 0.95)]);
        }
        AppItem::Home => {
            line(&[
                vec2(-0.75, -0.1),
                vec2(-0.75, 0.9),
                vec2(0.75, 0.9),
                vec2(0.75, -0.1),
            ]);
            line(&[vec2(-1.0, 0.05), vec2(0.0, -0.9), vec2(1.0, 0.05)]);
        }
        AppItem::Bookmarks => {
            line(&[
                vec2(-0.6, -1.0),
                vec2(0.6, -1.0),
                vec2(0.6, 1.0),
                vec2(0.0, 0.5),
                vec2(-0.6, 1.0),
                vec2(-0.6, -1.0),
            ]);
        }
        AppItem::Timeline => {
            p.circle_stroke(c, s, st);
            line(&[vec2(0.0, -0.6), vec2(0.0, 0.0), vec2(0.45, 0.3)]);
        }
        AppItem::FullScreen => {
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                line(&[
                    vec2(sx * 0.9, sy * 0.35),
                    vec2(sx * 0.9, sy * 0.9),
                    vec2(sx * 0.35, sy * 0.9),
                ]);
            }
        }
        AppItem::Tour => {
            line(&[vec2(-0.8, 0.0), vec2(-0.25, 0.6), vec2(0.85, -0.6)]);
        }
        AppItem::Picture => {
            // A framed landscape: a hill and a sun.
            line(&[
                vec2(-1.0, -0.75),
                vec2(1.0, -0.75),
                vec2(1.0, 0.75),
                vec2(-1.0, 0.75),
                vec2(-1.0, -0.75),
            ]);
            line(&[
                vec2(-1.0, 0.55),
                vec2(-0.35, -0.1),
                vec2(0.15, 0.4),
                vec2(0.45, 0.1),
                vec2(1.0, 0.6),
            ]);
            p.circle_stroke(c + vec2(0.45, -0.35) * s, s * 0.18, st);
        }
    }
}

/// A label whose right edge is at `at.x`, centred on `at.y`.
fn label_right(p: &egui::Painter, at: Pos2, text: &str) {
    let font = egui::FontId::proportional(11.0);
    let galley = p.layout_no_wrap(text.to_string(), font, INKY);
    let size = galley.size() + vec2(8.0, 2.0);
    let rect = Rect::from_min_size(pos2(at.x - size.x, at.y - size.y * 0.5), size);
    p.rect_filled(rect, 4.0, Color32::from_white_alpha(220));
    p.galley(rect.min + vec2(4.0, 1.0), galley, INKY);
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

/// Maps icon coordinates along a tool's barrel to the screen: `u` runs from
/// the tip (negative) up to the back end, slanting up-right like a pen in a
/// right hand; `v` is across the barrel. Units are `s` points.
struct Barrel {
    c: Pos2,
    s: f32,
}

impl Barrel {
    fn at(&self, u: f32, v: f32) -> Pos2 {
        let d = vec2(1.0, -1.0) * std::f32::consts::FRAC_1_SQRT_2;
        let n = vec2(1.0, 1.0) * std::f32::consts::FRAC_1_SQRT_2;
        self.c + d * (u * self.s) + n * (v * self.s)
    }

    /// A filled, outlined quad from `(u0, w0)` to `(u1, w1)`: it spans
    /// `u0..u1` with half-width `w0` at u0 and `w1` at u1.
    fn quad(
        &self,
        p: &egui::Painter,
        (u0, w0): (f32, f32),
        (u1, w1): (f32, f32),
        fill: Color32,
        line: Stroke,
    ) {
        p.add(Shape::convex_polygon(
            vec![
                self.at(u0, -w0),
                self.at(u1, -w1),
                self.at(u1, w1),
                self.at(u0, w0),
            ],
            fill,
            line,
        ));
    }
}

/// Drawn icons, sized relative to the button radius: outlined barrels with
/// the current ink color where the ink is (tip, cap or swipe).
fn tool_icon(p: &egui::Painter, c: Pos2, r: f32, tool: Tool, ink: Color32) {
    let line = Stroke::new((r * 0.075).max(1.2), INKY);
    match tool {
        Tool::Pen => {
            // A ballpoint: slim barrel, pointed tip in the ink color, a clip.
            let b = Barrel { c, s: r * 0.62 };
            b.quad(p, (-0.25, 0.17), (0.95, 0.17), FACE, line);
            p.add(Shape::convex_polygon(
                vec![b.at(-0.25, -0.17), b.at(-0.8, 0.0), b.at(-0.25, 0.17)],
                ink,
                line,
            ));
            p.line_segment([b.at(0.4, -0.3), b.at(0.85, -0.3)], line);
            p.line_segment([b.at(0.85, -0.3), b.at(0.85, -0.17)], line);
        }
        Tool::Marker => {
            // A fat marker: cap band and rounded nib in the ink color.
            let b = Barrel { c, s: r * 0.6 };
            b.quad(p, (-0.2, 0.27), (0.95, 0.27), FACE, line);
            b.quad(p, (0.62, 0.27), (0.95, 0.27), ink, line);
            b.quad(p, (-0.45, 0.14), (-0.2, 0.22), FACE, line);
            p.add(Shape::convex_polygon(
                vec![
                    b.at(-0.45, -0.12),
                    b.at(-0.72, -0.08),
                    b.at(-0.78, 0.0),
                    b.at(-0.72, 0.08),
                    b.at(-0.45, 0.12),
                ],
                ink,
                line,
            ));
        }
        Tool::Highlighter => {
            // A highlighter over the wide translucent swipe it leaves.
            let sw = r * 0.62;
            p.line_segment(
                [c + vec2(-sw, sw * 0.82), c + vec2(sw * 0.75, sw * 0.82)],
                Stroke::new(r * 0.26, ink.gamma_multiply(0.6)),
            );
            let b = Barrel {
                c: c + vec2(r * 0.08, -r * 0.08),
                s: r * 0.58,
            };
            b.quad(p, (-0.25, 0.32), (0.95, 0.32), FACE, line);
            b.quad(p, (0.55, 0.32), (0.95, 0.32), ink, line);
            // Chisel nib.
            p.add(Shape::convex_polygon(
                vec![
                    b.at(-0.25, -0.22),
                    b.at(-0.62, -0.22),
                    b.at(-0.48, 0.22),
                    b.at(-0.25, 0.22),
                ],
                ink,
                line,
            ));
        }
        Tool::Eraser => {
            // A block eraser: pink rubber end and a paper sleeve, over the
            // line it has just wiped.
            let b = Barrel {
                c: c + vec2(r * 0.05, -r * 0.08),
                s: r * 0.6,
            };
            b.quad(p, (-0.75, 0.36), (0.8, 0.36), FACE, line);
            b.quad(
                p,
                (-0.75, 0.36),
                (-0.15, 0.36),
                Color32::from_rgb(240, 140, 160),
                line,
            );
            let y = c.y + r * 0.6;
            p.line_segment(
                [pos2(c.x - r * 0.62, y), pos2(c.x + r * 0.62, y)],
                Stroke::new(line.width, Color32::from_gray(150)),
            );
        }
        Tool::Picker => eyedropper(p, c, r, ink),
        Tool::Shapes => {
            // A square overlapped by a circle and a triangle.
            let s = r * 0.32;
            p.rect_stroke(
                Rect::from_center_size(c + vec2(-s * 0.55, -s * 0.5), Vec2::splat(s * 1.5)),
                2.0,
                line,
                egui::StrokeKind::Middle,
            );
            p.circle_stroke(
                c + vec2(s * 0.55, s * 0.45),
                s * 0.8,
                Stroke::new(line.width, ink),
            );
            p.add(Shape::closed_line(
                vec![
                    c + vec2(-s * 0.35, s * 1.25),
                    c + vec2(-s * 1.25, s * 1.25),
                    c + vec2(-s * 0.8, s * 0.35),
                ],
                line,
            ));
        }
        Tool::Text => {
            // A serif "T".
            let s = r * 0.5;
            let st = Stroke::new((r * 0.11).max(1.5), ink);
            p.line_segment(
                [c + vec2(-s * 0.8, -s * 0.8), c + vec2(s * 0.8, -s * 0.8)],
                st,
            );
            p.line_segment(
                [c + vec2(-s * 0.8, -s * 0.8), c + vec2(-s * 0.8, -s * 0.55)],
                st,
            );
            p.line_segment(
                [c + vec2(s * 0.8, -s * 0.8), c + vec2(s * 0.8, -s * 0.55)],
                st,
            );
            p.line_segment([c + vec2(0.0, -s * 0.8), c + vec2(0.0, s * 0.85)], st);
            p.line_segment(
                [c + vec2(-s * 0.35, s * 0.85), c + vec2(s * 0.35, s * 0.85)],
                st,
            );
        }
        Tool::Select => {
            // A pointer arrow.
            let s = r * 0.62;
            let o = c + vec2(-s * 0.35, -s * 0.75);
            let pts = [
                (0.0, 0.0),
                (0.0, 1.25),
                (0.32, 0.95),
                (0.55, 1.42),
                (0.75, 1.33),
                (0.52, 0.86),
                (0.95, 0.86),
            ];
            p.add(Shape::convex_polygon(
                pts.iter().map(|q| o + vec2(q.0 * s, q.1 * s)).collect(),
                FACE,
                Stroke::NONE,
            ));
            p.add(Shape::closed_line(
                pts.iter().map(|q| o + vec2(q.0 * s, q.1 * s)).collect(),
                line,
            ));
        }
        Tool::Hand => {
            // Move: a cross with a solid arrowhead on each arm.
            let s = r * 0.62;
            for dir in [
                vec2(1.0, 0.0),
                vec2(-1.0, 0.0),
                vec2(0.0, 1.0),
                vec2(0.0, -1.0),
            ] {
                let n = vec2(-dir.y, dir.x);
                p.line_segment([c, c + dir * s * 0.62], line);
                p.add(Shape::convex_polygon(
                    vec![
                        c + dir * s,
                        c + dir * s * 0.58 + n * s * 0.3,
                        c + dir * s * 0.58 - n * s * 0.3,
                    ],
                    INKY,
                    Stroke::NONE,
                ));
            }
        }
    }
}

/// Outline of a shape kind, fitted in a circle of radius `r` around `c`.
fn shape_icon(p: &egui::Painter, c: Pos2, r: f32, kind: ShapeKind, col: Color32) {
    let st = ShapeStyle {
        kind,
        round: false,
        sides: 6,
        sloppiness: Sloppiness::Architect,
        start: Head::None,
        end: Head::Arrow,
        stroke: u32c(col),
        ..Default::default()
    };
    let geom = shapes::Geom {
        center: [c.x as f64, c.y as f64],
        half: [
            r as f64 * 0.8,
            r as f64 * if kind == ShapeKind::Rect { 0.6 } else { 0.8 },
        ],
        rot: 0.0,
        pts: vec![
            [(c.x - r * 0.75) as f64, (c.y + r * 0.6) as f64],
            [(c.x + r * 0.75) as f64, (c.y - r * 0.6) as f64],
        ],
    };
    pieces_icon(
        p,
        &shapes::pieces(&st, &geom, (r * 0.12).max(1.4) as f64, 1),
    );
}

/// Draw shape pieces given in points.
fn pieces_icon(p: &egui::Painter, pieces: &[shapes::Piece]) {
    for pc in pieces {
        let pts: Vec<Pos2> = pc
            .pts
            .iter()
            .map(|q| pos2(q[0] as f32, q[1] as f32))
            .collect();
        let col = {
            let [r, g, b, a] = pc.color.to_le_bytes();
            Color32::from_rgba_unmultiplied(r, g, b, a)
        };
        if pc.brush == Brush::Fill {
            p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
        } else {
            p.add(Shape::line(pts, Stroke::new(pc.width as f32, col)));
        }
    }
}

/// A small square showing a fill style.
fn fill_icon(p: &egui::Painter, c: Pos2, r: f32, f: FillStyle) {
    let st = ShapeStyle {
        kind: ShapeKind::Rect,
        fill_style: f,
        round: false,
        sloppiness: Sloppiness::Architect,
        stroke: u32c(INKY),
        fill: u32c(INKY),
        ..Default::default()
    };
    let geom = shapes::Geom {
        center: [c.x as f64, c.y as f64],
        half: [r as f64 * 0.7, r as f64 * 0.7],
        ..Default::default()
    };
    pieces_icon(p, &shapes::pieces(&st, &geom, 1.4, 1));
}

/// A short line in a dash style, a sloppiness or an arrow type.
fn line_icon(p: &egui::Painter, c: Pos2, r: f32, dash: Dash, slop: Sloppiness, arrow: ArrowType) {
    let st = ShapeStyle {
        kind: ShapeKind::Line,
        dash,
        sloppiness: slop,
        arrow,
        stroke: u32c(INKY),
        ..Default::default()
    };
    let (a, b) = if arrow == ArrowType::Straight {
        ([c.x - r * 0.8, c.y], [c.x + r * 0.8, c.y])
    } else {
        (
            [c.x - r * 0.8, c.y + r * 0.5],
            [c.x + r * 0.8, c.y - r * 0.5],
        )
    };
    let geom = shapes::Geom {
        pts: vec![[a[0] as f64, a[1] as f64], [b[0] as f64, b[1] as f64]],
        ..Default::default()
    };
    let mut pcs = shapes::pieces(&st, &geom, 1.8, 3);
    if dash != Dash::Solid {
        // egui has no dashes: draw the pattern as pieces.
        let n = if dash == Dash::Dashed { 3 } else { 5 };
        pcs.clear();
        for i in 0..n {
            let t0 = i as f32 / n as f32;
            let t1 = t0 + if dash == Dash::Dashed { 0.6 } else { 0.08 } / n as f32;
            let x0 = c.x - r * 0.8 + t0 * r * 1.6;
            let x1 = c.x - r * 0.8 + t1 * r * 1.6;
            p.line_segment(
                [pos2(x0, c.y), pos2(x1.max(x0 + 1.5), c.y)],
                Stroke::new(1.8, INKY),
            );
        }
    }
    pieces_icon(p, &pcs);
}

/// An arrowhead on a short line (`start`: pointing left).
fn head_icon(p: &egui::Painter, c: Pos2, r: f32, h: Head, start: bool) {
    let st = ShapeStyle {
        kind: ShapeKind::Arrow,
        sloppiness: Sloppiness::Architect,
        start: Head::None,
        end: h,
        stroke: u32c(INKY),
        ..Default::default()
    };
    let (a, b) = if start {
        ([c.x + r * 0.8, c.y], [c.x - r * 0.8, c.y])
    } else {
        ([c.x - r * 0.8, c.y], [c.x + r * 0.8, c.y])
    };
    let geom = shapes::Geom {
        pts: vec![[a[0] as f64, a[1] as f64], [b[0] as f64, b[1] as f64]],
        ..Default::default()
    };
    pieces_icon(p, &shapes::pieces(&st, &geom, 1.6, 1));
}

/// Undo: an arrow pointing left whose tail hooks round to the right and
/// down (↩); redo is its mirror image (↪).
fn undo_icon(p: &egui::Painter, c: Pos2, r: f32, redo: bool, col: Color32) {
    let s = r * 0.55;
    let f = if redo { -1.0 } else { 1.0 };
    let at = |x: f32, y: f32| c + vec2(x * f * s, y * s);
    let mut pts = vec![at(-0.5, -0.32)];
    for i in 0..=16 {
        let a = -FRAC_PI_2 + PI * (i as f32 / 16.0);
        pts.push(at(0.22 + 0.48 * a.cos(), 0.16 + 0.48 * a.sin()));
    }
    pts.push(at(-0.25, 0.64));
    p.add(Shape::line(pts, Stroke::new((r * 0.13).max(1.5), col)));
    p.add(Shape::convex_polygon(
        vec![at(-0.95, -0.32), at(-0.45, -0.72), at(-0.45, 0.08)],
        col,
        Stroke::NONE,
    ));
}

/// The color dial, drawn inside the tool panel at its full width: current
/// color in the middle, two rings of presets, a continuous hue ring outside,
/// an eyedropper, and saturation / brightness bars below. Returns true when
/// the eyedropper was tapped.
fn color_dial(
    ui: &mut egui::Ui,
    color: &mut Color32,
    hl: bool,
    dial_hue: &mut f32,
    touch: bool,
) -> bool {
    use egui::ecolor::HsvaGamma;
    let w = ui.available_width();
    let r_out = w * 0.5 - 2.0;
    let (area, _) = ui.allocate_exact_size(vec2(w, w), Sense::hover());
    let center = area.center();
    let p = ui.painter().clone();
    let mut pick = false;
    let mut hsv = HsvaGamma::from(*color);
    if hsv.s > 0.02 {
        *dial_hue = hsv.h;
    } else {
        hsv.h = *dial_hue;
    }

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
            *dial_hue = h;
            let s = if hsv.s < 0.15 { 0.9 } else { hsv.s };
            let v = if hsv.v < 0.25 { 0.9 } else { hsv.v };
            *color = HsvaGamma { h, s, v, a: 1.0 }.into();
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
            if *color == col {
                p.circle_stroke(pc, rr + 3.0, Stroke::new(2.0, ACCENT));
            }
            if resp.clicked() {
                *color = col;
            }
        }
    }

    // Centre: the current color.
    let rc = r_out * 0.2;
    p.circle_filled(center, rc, *color);
    p.circle_stroke(center, rc, Stroke::new(1.5, EDGE));

    // Eyedropper, in the dial's lower-left corner.
    let er = (r_out * 0.13).max(if touch { 18.0 } else { 14.0 });
    let ep = area.left_bottom() + vec2(er + 1.0, -er - 1.0);
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
    eresp.on_hover_text("Pick a color from the canvas");

    // Saturation and brightness bars.
    let hsv = {
        let mut h = HsvaGamma::from(*color);
        if h.s <= 0.02 {
            h.h = *dial_hue;
        }
        h
    };
    let bar_h = if touch { 22.0 } else { 16.0 };
    for (row, name) in ["Saturation", "Brightness"].iter().enumerate() {
        ui.add_space(2.0);
        ui.label(egui::RichText::new(*name).small().weak());
        let (rect, resp) = ui.allocate_exact_size(vec2(w, bar_h), Sense::click_and_drag());
        let rect = rect.shrink2(vec2(5.0, 0.0));
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
            Rect::from_center_size(pos2(mx, rect.center().y), vec2(7.0, bar_h + 6.0)),
            3.0,
            Stroke::new(2.0, INKY),
            egui::StrokeKind::Outside,
        );
        if let Some(pos) = resp.interact_pointer_pos() {
            if resp.clicked() || resp.dragged() {
                let t = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                let next = if row == 0 {
                    HsvaGamma { s: t, ..hsv }
                } else {
                    HsvaGamma { v: t, ..hsv }
                };
                *color = next.into();
            }
        }
    }
    pick
}

/// A pipette: glass tube tilted up-right with a rubber bulb.
fn eyedropper(p: &egui::Painter, c: Pos2, r: f32, tip: Color32) {
    // Rubber bulb at the back, a collar, a glass tube holding a little of
    // the ink, and a drop falling from the tip.
    let line = Stroke::new((r * 0.075).max(1.2), INKY);
    let b = Barrel {
        c: c + vec2(r * 0.06, -r * 0.06),
        s: r * 0.6,
    };
    b.quad(p, (-0.5, 0.13), (0.35, 0.13), FACE, line);
    b.quad(p, (-0.5, 0.13), (-0.12, 0.13), tip, line);
    p.add(Shape::convex_polygon(
        vec![b.at(-0.5, -0.13), b.at(-0.82, 0.0), b.at(-0.5, 0.13)],
        FACE,
        line,
    ));
    b.quad(p, (0.35, 0.3), (0.48, 0.3), INKY, Stroke::NONE);
    p.circle_filled(b.at(0.72, 0.0), r * 0.17, INKY);
    p.circle_filled(b.at(-0.98, 0.2), r * 0.07, tip);
}
