// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Screen-size-independent controls: a tool button in the bottom-right
//! corner that fans out into a radial menu, small undo/redo buttons, a tool
//! panel bottom-left (width, pressure, color dial) and a settings button
//! top-right whose fan holds the canvas commands. Icons are drawn, not taken from a font.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

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
    /// The brush: a plain line (simple), or the brush engine (advanced).
    Pen,
    /// Texture: splotches, spatter, stamps and patterns.
    Texture,
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
    /// Select by drawing a loop around things; then like Select.
    Lasso,
    /// Fill a closed outline with color.
    Bucket,
}

/// The tool fan, inner ring first.
const TOOLS: [Tool; 11] = [
    Tool::Pen,
    Tool::Texture,
    Tool::Highlighter,
    Tool::Bucket,
    Tool::Eraser,
    Tool::Select,
    Tool::Lasso,
    Tool::Shapes,
    Tool::Text,
    Tool::Picker,
    Tool::Hand,
];

impl Tool {
    pub fn brush(self) -> Option<Brush> {
        match self {
            Tool::Pen => Some(Brush::Pen),
            Tool::Texture => Some(Brush::Dabs),
            Tool::Highlighter => Some(Brush::Highlighter),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Tool::Pen => "Brush",
            Tool::Texture => "Texture",
            Tool::Highlighter => "Highlighter",
            Tool::Eraser => "Eraser",
            Tool::Hand => "Pan",
            Tool::Picker => "Picker",
            Tool::Shapes => "Shapes",
            Tool::Text => "Text",
            Tool::Select => "Select",
            Tool::Lasso => "Lasso",
            Tool::Bucket => "Bucket",
        }
    }

    /// Select or Lasso: tools that pick things to edit.
    pub fn selects(self) -> bool {
        matches!(self, Tool::Select | Tool::Lasso)
    }

    /// Tools with settings in the tool panel.
    fn has_panel(self) -> bool {
        self.brush().is_some()
            || matches!(
                self,
                Tool::Shapes | Tool::Text | Tool::Select | Tool::Lasso | Tool::Bucket
            )
    }
}

/// Per-brush settings; width is in screen pixels at the zoom you draw at.
#[derive(Clone, Copy)]
pub struct InkSettings {
    pub color: Color32,
    pub width: f32,
    /// Brush only: width follows pressure.
    pub pressure: bool,
    pub dash: Dash,
    /// 0..=255.
    pub opacity: u8,
    /// Brush only: the brush engine (GIMP-style dynamics and looks) instead
    /// of the plain line.
    pub advanced: bool,
    pub params: ogpaper_core::BrushParams,
}

impl InkSettings {
    pub fn new(color: Color32, width: f32, pressure: bool, dash: Dash, opacity: u8) -> Self {
        InkSettings {
            color,
            width,
            pressure,
            dash,
            opacity,
            advanced: false,
            params: Default::default(),
        }
    }

    /// How a stroke drawn with these settings by `tool` is stored.
    pub fn brush_for(&self, tool: Tool) -> Option<Brush> {
        match tool {
            Tool::Pen if self.advanced => Some(Brush::Dabs),
            Tool::Pen if self.pressure => Some(Brush::Pen),
            Tool::Pen => Some(Brush::Marker),
            Tool::Texture => Some(Brush::Dabs),
            t => t.brush(),
        }
    }

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
    /// Lasso loop being drawn (points).
    pub lasso: Option<Vec<Pos2>>,
    /// A selected line or arrow: its points, and the + between them.
    pub joints: Option<(Vec<Pos2>, Vec<Pos2>)>,
    /// People on a live canvas: where, their name, their color, and the
    /// direction they are in when they are off screen.
    pub labels: Vec<(Pos2, String, Color32, Option<Vec2>)>,
}

/// A page on this device, as Pages lists it.
#[derive(Clone, Debug, Default)]
pub struct LocalPage {
    /// The web page's id for it (its canvas); empty on desktop.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub key: String,
    pub name: String,
    /// When it last changed (ms since the Unix epoch).
    pub changed: u64,
    pub current: bool,
}

/// A server in Pages: its name, how it is, and its pages (name, changed,
/// open here).
#[derive(Clone, Debug, Default)]
pub struct ServerView {
    pub name: String,
    pub state: String,
    pub can_edit: bool,
    pub pages: Vec<(String, u64, bool)>,
}

/// What the Share live panel shows (from `net`).
#[derive(Clone, Debug, Default)]
pub struct LiveInfo {
    pub hosting: bool,
    pub state: String,
    /// Others: peer id and name.
    pub people: Vec<(u64, String)>,
    /// (what, link) to copy.
    pub links: Vec<(String, String)>,
    /// 0 newest, 1 host wins, 2 guests win.
    pub policy: u8,
    pub view_only: bool,
}

impl PartialEq for InkSettings {
    fn eq(&self, o: &Self) -> bool {
        self.color == o.color
            && self.width == o.width
            && self.pressure == o.pressure
            && self.dash == o.dash
            && self.opacity == o.opacity
            && self.advanced == o.advanced
            && (!self.advanced || self.params == o.params)
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
    /// The radial toolbar's fan.
    Bar,
    /// The settings button's fan (canvas commands).
    App,
}

/// Commands in the settings fan. The app decides which ones exist here
/// (the web page adds bookmarks, the timeline, full screen and the tour).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub enum AppItem {
    /// Find text anywhere on the canvas.
    Search,
    /// Save the view or the selection as PNG, JPEG, SVG or PDF.
    Export,
    /// Paste from the clipboard (for touch screens, with no Ctrl+V).
    Paste,
    /// Saved drawings to place copies of.
    Library,
    /// Diagram mode: lines and arrows stick to objects.
    Diagram,
    /// Move the controls around (now inside the Layout window).
    #[allow(dead_code)]
    Layout,
    /// Set your own keys.
    Hotkeys,
    /// The quick toolbar as a fan from the bottom-right corner.
    RadialBar,
    /// Show or hide the tool button, the tool panel, the quick toolbar.
    ShowTools,
    ShowPanel,
    ShowBar,
    /// Background grid: off, lines, dots.
    Grid,
    Dark,
    Import,
    Merge,
    Changes,
    Folder,
    Live,
    Pages,
    LayoutMenu,
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
            AppItem::Search => "Search text",
            AppItem::Export => "Export",
            AppItem::Paste => "Paste",
            AppItem::Library => "Library",
            AppItem::Diagram => "Diagram",
            AppItem::Layout => "Edit layout",
            AppItem::Hotkeys => "Hotkeys",
            AppItem::RadialBar => "Radial toolbar",
            AppItem::ShowTools => "Tool button",
            AppItem::ShowPanel => "Tool panel",
            AppItem::ShowBar => "Quick toolbar",
            AppItem::Grid => "Grid",
            AppItem::Dark => "Dark mode",
            AppItem::Import => "Import canvas",
            AppItem::Merge => "Merge copy",
            AppItem::Changes => "Save changes",
            AppItem::Folder => "Sync folder",
            AppItem::Live => "Share live",
            AppItem::Pages => "Pages",
            AppItem::LayoutMenu => "Layout",
            AppItem::New => "New canvas",
            AppItem::Open => "Open",
            AppItem::Save => "Save copy",
            AppItem::Home => "Home",
            AppItem::Picture => "Insert picture / PDF",
            AppItem::Bookmarks => "Bookmarks",
            AppItem::Timeline => "Timeline",
            AppItem::FullScreen => "Full screen",
            AppItem::Tour => "Tour",
        }
    }

    fn action(self) -> Action {
        match self {
            AppItem::Search => Action::Search,
            AppItem::Export => Action::Export,
            AppItem::Paste => Action::Paste,
            AppItem::Library => Action::Library,
            AppItem::Diagram => Action::Diagram,
            AppItem::Layout => Action::EditLayout,
            AppItem::Hotkeys => Action::Hotkeys,
            AppItem::RadialBar => Action::RadialBar,
            AppItem::ShowTools => Action::ShowTools,
            AppItem::ShowPanel => Action::ShowPanel,
            AppItem::ShowBar => Action::ShowBar,
            AppItem::Grid => Action::Grid,
            AppItem::Dark => Action::Dark,
            AppItem::Import => Action::Import,
            AppItem::Merge => Action::MergeCopy,
            AppItem::Changes => Action::SaveChanges,
            AppItem::Folder => Action::SyncFolder,
            AppItem::Live => Action::LivePanel,
            AppItem::Pages => Action::PagesPanel,
            AppItem::LayoutMenu => Action::LayoutMenu,
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

/// A sticker in the desktop library panel.
pub struct LibEntry {
    pub name: String,
    /// Thumbnail (RGBA, square), until it is a texture.
    pub thumb: Option<(usize, Vec<u8>)>,
    pub tex: Option<egui::TextureHandle>,
}

/// The background grid.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GridMode {
    #[default]
    Off,
    Lines,
    Dots,
}

impl GridMode {
    pub fn next(self) -> Self {
        match self {
            GridMode::Off => GridMode::Lines,
            GridMode::Lines => GridMode::Dots,
            GridMode::Dots => GridMode::Off,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            GridMode::Off => "off",
            GridMode::Lines => "lines",
            GridMode::Dots => "dots",
        }
    }

    pub fn from_key(k: &str) -> Self {
        match k {
            "lines" => GridMode::Lines,
            "dots" => GridMode::Dots,
            _ => GridMode::Off,
        }
    }
}

pub struct UiState {
    pub tool: Tool,
    pub grid: GridMode,
    /// Diagram mode: lines and arrows stick to what they touch.
    pub diagram: bool,
    /// The quick toolbar as a fan from the bottom-right corner (tools move
    /// bottom left, their settings top right).
    pub radial_bar: bool,
    /// Dark mode: the whole screen drawn with its lightness flipped.
    pub dark: bool,
    /// Another canvas is being placed (shows Place / Cancel).
    pub importing: bool,
    /// Syncing through a shared folder.
    pub folder_on: bool,
    /// The live connection, if any (set by the app each frame).
    pub live: Option<LiveInfo>,
    pub live_open: bool,
    pub join_text: String,
    pub relay_text: Option<String>,
    pub pages_open: bool,
    pub layout_open: bool,
    pub local_pages: Vec<LocalPage>,
    pub servers: Vec<ServerView>,
    pub add_server_text: String,
    pub name_text: Option<String>,
    /// Hidden by the person (Settings): the tool button, the tool panel,
    /// the quick toolbar.
    pub hide_tools: bool,
    pub hide_panel: bool,
    pub hide_bar: bool,
    /// Saved views in toolbar slots, the current view (filled in by the app
    /// each frame) and a view to fly to (for the app).
    pub views: std::collections::BTreeMap<u32, hotbar::View>,
    pub view_now: Option<hotbar::View>,
    pub fly_to: Option<String>,
    /// Actions a slot asked for (undo, redo), handed out with this frame's.
    pub queued: Vec<Action>,
    /// Hotkeys: the keymap, its menu, and the binding waiting for a key.
    pub keys: crate::hotkeys::Keymap,
    pub keys_open: bool,
    pub key_capture: Option<String>,
    /// Where the controls sit, and whether they are being moved.
    pub layout: crate::layout::Layout,
    pub layout_edit: bool,
    /// Desktop text search: open, focus it next frame, the query, the query
    /// the results are for, the results, and a result picked to fly to.
    pub search_open: bool,
    pub search_focus: bool,
    pub search_query: String,
    pub search_ran: String,
    pub search_hits: Vec<crate::search::Hit>,
    pub search_pick: Option<u32>,
    /// The ink tool the picker hands its color to (the last one used).
    pub last_ink: Tool,
    /// Picker loupe while dragging: position (points) and the color under it.
    pub pick_preview: Option<(Pos2, Option<Color32>)>,
    /// Hue kept while the color is grey, so the dial remembers it.
    pub dial_hue: f32,
    pub pen: InkSettings,
    /// The texture tool's settings (always the brush engine).
    pub texture: InkSettings,
    pub highlighter: InkSettings,
    /// The bucket's color and opacity (width unused).
    pub fill: InkSettings,
    /// Bucket gap closing (points): 0 off.
    pub fill_gap: u8,
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
    /// A picture is being cropped.
    pub cropping: bool,
    /// Desktop library: open, and its stickers (name, thumbnail pixels,
    /// texture once made).
    pub lib_open: bool,
    pub lib: Vec<LibEntry>,
    /// Font names egui can draw (the picker shows each name in its font).
    pub egui_fonts: std::collections::HashSet<String>,
    /// Text being typed (native editor): the screen point and the text.
    pub text_edit: Option<(Pos2, String)>,
    pub overlay: Overlay,
    /// Saved tools: the quick bar along the bottom (the active toolbar's
    /// slots) and the inventory.
    pub hotbar: Vec<Option<Preset>>,
    pub inventory: Vec<Option<Preset>>,
    /// Every toolbar; the active one's slots live in `hotbar` while it shows.
    pub toolbars: Vec<hotbar::Toolbar>,
    pub active_bar: usize,
    /// The toolbar switcher's list is open.
    pub bar_menu: bool,
    /// Renaming the active toolbar: the text being edited.
    pub bar_rename: Option<String>,
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
                advanced: false,
                params: Default::default(),
            },
            texture: {
                let look = ogpaper_core::brush::looks()
                    .into_iter()
                    .find(|l| l.texture)
                    .expect("a texture look");
                InkSettings {
                    color: Color32::from_rgb(40, 120, 70),
                    width: 28.0,
                    pressure: false,
                    dash: Dash::Solid,
                    opacity: 255,
                    advanced: true,
                    params: look.params,
                }
            },
            highlighter: InkSettings {
                color: Color32::from_rgb(255, 214, 0),
                width: 22.0,
                pressure: false,
                dash: Dash::Solid,
                opacity: 255,
                advanced: false,
                params: Default::default(),
            },
            fill: InkSettings {
                color: Color32::from_rgb(250, 210, 30),
                width: 1.0,
                pressure: false,
                dash: Dash::Solid,
                opacity: 255,
                advanced: false,
                params: Default::default(),
            },
            fill_gap: 0,
            menu: Menu::None,
            file_name: "Untitled".into(),
            zoom_log10: 0.0,
            can_undo: false,
            can_redo: false,
            strokes: 0,
            message: None,
            touch_ui: false,
            app_items: vec![
                AppItem::Pages,
                AppItem::New,
                AppItem::Open,
                AppItem::Import,
                AppItem::Merge,
                AppItem::Changes,
                AppItem::Folder,
                AppItem::Live,
                AppItem::Export,
                AppItem::Paste,
                AppItem::Library,
                AppItem::Picture,
                AppItem::Home,
                AppItem::Search,
                AppItem::LayoutMenu,
                AppItem::Dark,
                AppItem::Diagram,
                AppItem::Hotkeys,
            ],
            hide_tools: false,
            hide_panel: false,
            hide_bar: false,
            radial_bar: false,
            dark: false,
            importing: false,
            folder_on: false,
            live: None,
            live_open: false,
            join_text: String::new(),
            relay_text: None,
            pages_open: false,
            layout_open: false,
            local_pages: Vec::new(),
            servers: Vec::new(),
            add_server_text: String::new(),
            name_text: None,
            queued: Vec::new(),
            views: Default::default(),
            view_now: None,
            fly_to: None,
            keys: Default::default(),
            keys_open: false,
            key_capture: None,
            diagram: false,
            layout: Default::default(),
            layout_edit: false,
            cropping: false,
            lib_open: false,
            lib: Vec::new(),
            search_open: false,
            search_focus: false,
            search_query: String::new(),
            search_ran: String::new(),
            search_hits: Vec::new(),
            search_pick: None,
            grid: GridMode::Off,
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
            toolbars: vec![hotbar::empty_bar("Toolbar 1")],
            active_bar: 0,
            bar_menu: false,
            bar_rename: None,
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
        self.menu != Menu::None || self.bar_menu || self.bag_open
    }

    /// Close every menu (a tap on the canvas does this).
    pub fn close_menus(&mut self) {
        self.menu = Menu::None;
        self.bar_menu = false;
        self.bar_rename = None;
        // A tap off the inventory closes it too (a held tool goes back).
        self.bag_open = false;
    }

    /// Esc: close every menu and panel that is open (not the tool panel).
    /// False when nothing was.
    pub fn close_all(&mut self) -> bool {
        let any = self.menu != Menu::None
            || self.bar_menu
            || self.bag_open
            || self.search_open
            || self.lib_open
            || self.keys_open;
        self.close_menus();
        self.bag_open = false;
        self.search_open = false;
        self.lib_open = false;
        self.keys_open = false;
        any
    }

    /// The current tool and its settings.
    pub fn preset(&self) -> Preset {
        let mut p = Preset::tool(self.tool);
        match self.tool {
            Tool::Pen => p.ink = Some(self.pen),
            Tool::Texture => p.ink = Some(self.texture),
            Tool::Highlighter => p.ink = Some(self.highlighter),
            Tool::Shapes => p.shape = Some((self.shape, self.shape_width)),
            Tool::Text => p.text = Some((self.text, self.text_size)),
            _ => {}
        }
        p
    }

    /// Switch to a saved tool (or fly to a saved view).
    pub fn apply(&mut self, p: &Preset) {
        if let Some(c) = p.cmd {
            self.queued.push(match c {
                hotbar::SlotCmd::Undo => Action::Undo,
                hotbar::SlotCmd::Redo => Action::Redo,
            });
            return;
        }
        if let Some(v) = p.view {
            self.fly_to = self.views.get(&v).map(|v| v.cam.clone());
            self.menu = Menu::None;
            return;
        }
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

    /// Everything to save: the toolbars (with the showing one's current slots)
    /// and the inventory.
    pub fn saved(&self) -> hotbar::Saved {
        let mut bars = self.toolbars.clone();
        if let Some(b) = bars.get_mut(self.active_bar) {
            b.slots = self.hotbar.clone();
        }
        // Only the views some slot still points at.
        let used: std::collections::HashSet<u32> = bars
            .iter()
            .flat_map(|b| b.slots.iter())
            .chain(self.inventory.iter())
            .flatten()
            .filter_map(|p| p.view)
            .collect();
        hotbar::Saved {
            bars,
            active: self.active_bar,
            inv: self.inventory.clone(),
            views: self
                .views
                .iter()
                .filter(|(k, _)| used.contains(k))
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
        }
    }

    /// Keep `v` as a saved view; its id, for a slot.
    pub fn add_view(&mut self, v: hotbar::View) -> u32 {
        let id = self.views.keys().next_back().map_or(1, |k| k + 1);
        self.views.insert(id, v);
        id
    }

    /// Put a saved view in the first empty slot of the active toolbar (else
    /// the inventory). Where it went, for a message.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn view_to_bar(&mut self, v: hotbar::View) -> String {
        let name = v.name.clone();
        let p = Preset::view(self.add_view(v));
        self.presets_dirty = true;
        if let Some(k) = self.hotbar.iter().position(|x| x.is_none()) {
            self.hotbar[k] = Some(p);
            format!(
                "{name} is on toolbar {}, slot {}",
                self.active_bar + 1,
                k + 1
            )
        } else {
            // Full: in hand, with the inventory open, to drop on any slot.
            self.held = Some(p);
            self.bag_open = true;
            self.bar_menu = false;
            format!("Tap a slot to put {name} there")
        }
    }

    pub fn load_saved(&mut self, s: hotbar::Saved) {
        self.active_bar = s.active.min(s.bars.len().saturating_sub(1));
        self.hotbar = s
            .bars
            .get(self.active_bar)
            .map_or_else(|| vec![None; hotbar::BAR], |b| b.slots.clone());
        self.toolbars = s.bars;
        self.inventory = s.inv;
        self.views = s.views;
    }

    /// Show toolbar `k` in the quick bar.
    pub fn switch_bar(&mut self, k: usize) {
        if k >= self.toolbars.len() || k == self.active_bar {
            return;
        }
        self.toolbars[self.active_bar].slots = std::mem::take(&mut self.hotbar);
        self.active_bar = k;
        self.hotbar = self.toolbars[k].slots.clone();
        self.bar_rename = None;
        self.presets_dirty = true;
        self.message = Some(format!("Toolbar {}", k + 1));
    }

    /// Next (`+1`) or previous (`-1`) toolbar, wrapping around.
    pub fn cycle_bar(&mut self, step: isize) {
        let n = self.toolbars.len() as isize;
        if n > 1 {
            self.switch_bar((self.active_bar as isize + step).rem_euclid(n) as usize);
        }
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
        Tool::Texture => Some(&mut st.texture),
        Tool::Highlighter => Some(&mut st.highlighter),
        Tool::Bucket => Some(&mut st.fill),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Undo,
    Redo,
    New,
    Open,
    /// Bring another canvas in, to move and place.
    Import,
    /// Merge another copy of this canvas into it.
    MergeCopy,
    /// Save only what changed since the last merge.
    SaveChanges,
    /// Start or stop syncing through a shared folder.
    SyncFolder,
    /// Open or close the Share live panel.
    LivePanel,
    /// Host this canvas (desktop).
    HostStart,
    /// The browser's no-server hosting card (WebRTC invites).
    RtcPanel,
    /// Share through the relay typed in Share live.
    RelayShare,
    /// New keys: links handed out so far stop working.
    NewLinks,
    /// Open or close Pages (this device's pages and the servers').
    PagesPanel,
    /// Open or close the Layout window (what shows, grid, edit layout).
    LayoutMenu,
    /// The background: 0 plain paper, 1 grid lines, 2 dots.
    SetGrid(u8),
    OpenLocal(usize),
    NewLocal,
    DeleteLocal(usize),
    /// Add the server whose link is typed in Pages.
    AddServer,
    RemoveServer(usize),
    RefreshServer(usize),
    NewServerPage(usize),
    /// Server, page.
    OpenServerPage(usize, usize),
    /// Stop hosting, or leave a shared canvas.
    NetStop,
    /// Join by the link typed in Share live.
    Join,
    /// The host's rule for rival edits.
    SetPolicy(u8),
    /// Fly to someone's view (true: and follow it).
    GoToPeer(u64, bool),
    /// The name typed in Share live becomes the one others see.
    SetName,
    ImportPlace,
    ImportCancel,
    SaveAs,
    Home,
    // Selection.
    Duplicate,
    Delete,
    ToFront,
    ToBack,
    FlipH,
    FlipV,
    /// Toggle diagram mode.
    Diagram,
    /// Start moving the controls around.
    EditLayout,
    /// Open the hotkeys menu.
    Hotkeys,
    /// Switch the quick toolbar between a bar and a fan.
    RadialBar,
    /// Show / hide the tool button, the tool panel, the quick toolbar.
    ShowTools,
    ShowPanel,
    ShowBar,
    /// The hotkeys changed in the menu: save them.
    SaveKeys,
    /// Save the selection to the library.
    SaveSticker,
    /// Open the library.
    Library,
    /// Library (desktop): place sticker i, delete it.
    LibPlace(usize),
    LibDelete(usize),
    /// Crop the selected picture; finish; give up.
    Crop,
    CropDone,
    CropCancel,
    EditText,
    /// Load a font file of your own.
    AddFont,
    /// Insert a picture from a file.
    Picture,
    TextDone,
    TextCancel,
    /// Cycle the background grid: off, lines, dots.
    Grid,
    Dark,
    /// Open the text search.
    Search,
    /// Export the view or selection as a picture or PDF.
    Export,
    /// Paste from the clipboard.
    Paste,
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
    /// The settings button, top right by default.
    app: Pos2,
    /// The arcs the tool and settings fans open along (start, sweep).
    tool_arc: (f32, f32),
    app_arc: (f32, f32),
    /// Radial toolbar: its button and arc.
    bar: Pos2,
    bar_arc: (f32, f32),
}

/// Size of the round buttons and fans by screen: about 60% on a phone
/// (smaller side ~380 points), full size from ~700 points (tablets, desktops).
fn ui_scale(screen: Rect) -> f32 {
    let side = screen.width().min(screen.height());
    (0.6 + 0.4 * (side - 380.0) / (700.0 - 380.0)).clamp(0.6, 1.0)
}

/// A point at screen fractions `f`, kept `pad` inside the screen.
fn at_frac(screen: Rect, f: [f32; 2], pad: f32) -> Pos2 {
    pos2(
        (screen.left() + f[0] * screen.width()).clamp(
            screen.left() + pad,
            (screen.right() - pad).max(screen.left() + pad),
        ),
        (screen.top() + f[1] * screen.height()).clamp(
            screen.top() + pad,
            (screen.bottom() - pad).max(screen.top() + pad),
        ),
    )
}

/// The screen fractions of a point.
fn frac_of(screen: Rect, p: Pos2) -> [f32; 2] {
    [
        ((p.x - screen.left()) / screen.width().max(1.0)).clamp(0.0, 1.0),
        ((p.y - screen.top()) / screen.height().max(1.0)).clamp(0.0, 1.0),
    ]
}

fn geo(ctx: &egui::Context, st: &UiState) -> Geo {
    let touch = st.touch_ui;
    let lay = &st.layout;
    let screen = ctx.content_rect();
    let k = ui_scale(screen);
    let r = if touch { 30.0 } else { 24.0 } * k;
    let m = if touch { 18.0 } else { 16.0 } * k.max(0.75);
    let corner_br = pos2(screen.right() - m - r, screen.bottom() - m - r);
    // Radial toolbar: tools bottom left, the toolbar's fan bottom right.
    let tool = match lay.tool {
        _ if st.radial_bar => pos2(screen.left() + m + r, screen.bottom() - m - r),
        Some(f) => at_frac(screen, f, m + r),
        None => corner_br,
    };
    let bar = corner_br;
    let app = match lay.app {
        Some(f) => at_frac(screen, f, m + r),
        None => pos2(screen.right() - m - r, screen.top() + m + r),
    };
    Geo {
        app,
        r,
        tool,
        tool_arc: crate::layout::fan_arc(frac_of(screen, tool)),
        app_arc: crate::layout::fan_arc(frac_of(screen, app)),
        bar,
        bar_arc: crate::layout::fan_arc(frac_of(screen, bar)),
    }
}

/// Draw the UI; returns actions for the app to perform.
pub fn draw(ctx: &egui::Context, st: &mut UiState) -> Vec<Action> {
    let mut actions = Vec::new();
    // A press anywhere but on an open menu closes it (on the canvas, the
    // app also skips drawing for that tap). What is where comes from the
    // last frame: the fans' own buttons and items, the panels' areas.
    let press = ctx.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    if let Some(pos) = press {
        let on_widget = |id: Id| {
            ctx.read_response(id)
                .is_some_and(|r| r.rect.expand(4.0).contains(pos))
        };
        let on_fan = on_widget(Id::new("tool_btn"))
            || on_widget(Id::new("app_btn"))
            || on_widget(Id::new("rbar_btn"))
            || [false, true]
                .into_iter()
                .any(|r| on_widget(Id::new(("ur", r))))
            || (0..TOOLS.len()).any(|i| on_widget(Id::new(("tool", i))))
            || (0..st.app_items.len() + 4).any(|i| on_widget(Id::new(("app_item", i))))
            || (0..hotbar::BAR + 4).any(|i| on_widget(Id::new(("rbar", i))))
            || (0..st.toolbars.len())
                .any(|k| (0..hotbar::BAR).any(|i| on_widget(Id::new(("rbar_o", k, i)))));
        let hit = ctx.layer_id_at(pos).map(|l| l.id);
        let on = |names: &[&str]| hit.is_some_and(|id| names.iter().any(|n| id == Id::new(*n)));
        if st.menu != Menu::None && !on_fan {
            st.menu = Menu::None;
        }
        if st.bar_menu && !on(&["toolbar_menu", "inventory", "quick_bar", "radial_bar"]) && !on_fan
        {
            st.bar_menu = false;
        }
        if st.bag_open && !on(&["inventory", "quick_bar", "radial_bar"]) && !on_fan {
            st.bag_open = false;
        }
        if st.live_open && !on(&["live"]) && !on_fan {
            st.live_open = false;
        }
        if st.pages_open && !on(&["pages"]) && !on_fan {
            st.pages_open = false;
        }
        if st.layout_open && !on(&["layout_menu"]) && !on_fan {
            st.layout_open = false;
        }
    }
    if st.layout_edit {
        layout_editor(ctx, st);
        return actions;
    }
    if st.importing {
        import_bar(ctx, &mut actions);
    }
    let g = geo(ctx, st);
    let t = ctx.animate_bool_with_time(Id::new("tools_open"), st.menu == Menu::Tools, 0.12);

    // The controls layer covers exactly the buttons plus any open fan, so
    // "pointer over UI" is right and the rest of the screen stays canvas.
    let sq = |c: Pos2, r: f32| Rect::from_center_size(c, Vec2::splat(2.0 * r));
    let mut bbox = sq(g.tool, g.r);
    // Undo and redo lead the fan, then the tools.
    let tool_slots = ring_slots(TOOLS.len() + 2, g.r, g.tool_arc.1);
    if t > 0.0 {
        let reach = fan_reach(&tool_slots, g.r) * t + g.r * 1.2;
        bbox = bbox.union(Rect::from_center_size(
            g.tool,
            Vec2::splat(2.0 * (reach + 30.0)),
        ));
    }
    let bbox = bbox.expand(4.0);
    let screen = ctx.content_rect();
    // The tool button and its fan (Settings > Tool button can hide them).
    if !st.hide_tools {
        egui::Area::new(Id::new("controls"))
            .order(Order::Foreground)
            .fixed_pos(bbox.min)
            .show(ctx, |ui| {
                ui.set_clip_rect(screen);
                ui.allocate_exact_size(bbox.size(), Sense::hover());
                let p = ui.painter().clone();

                // ---- tool fan: quarter-circle rings up and left of the tool button
                if t > 0.0 {
                    // Undo and redo: the fan stays open, to step back several times.
                    for (i, redo) in [(0usize, false), (1, true)] {
                        let (radius, frac) = tool_slots[i];
                        let a = g.tool_arc.0 + g.tool_arc.1 * frac;
                        let pc = g.tool + Vec2::angled(a) * radius * t;
                        let rr = g.r * 0.9 * t.max(0.3);
                        let resp = ui.interact(
                            Rect::from_center_size(pc, Vec2::splat(2.0 * rr)),
                            Id::new(("ur", redo)),
                            Sense::click(),
                        );
                        let enabled = if redo { st.can_redo } else { st.can_undo };
                        disc(
                            &p,
                            pc,
                            rr,
                            if resp.hovered() && enabled {
                                Color32::WHITE
                            } else {
                                FACE
                            },
                            false,
                        );
                        undo_icon(
                            &p,
                            pc,
                            rr * 0.8,
                            redo,
                            if enabled {
                                INKY
                            } else {
                                Color32::from_gray(190)
                            },
                        );
                        if t > 0.9 {
                            let out = Vec2::angled(a);
                            label(
                                &p,
                                pc + out * (rr + 16.0) + vec2(0.0, 2.0),
                                if redo { "Redo" } else { "Undo" },
                            );
                        }
                        if enabled && resp.clicked() {
                            actions.push(if redo { Action::Redo } else { Action::Undo });
                        }
                    }
                    for (i, &tool) in TOOLS.iter().enumerate() {
                        let (radius, frac) = tool_slots[i + 2];
                        let a = g.tool_arc.0 + g.tool_arc.1 * frac;
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
                if (tool_resp.long_touched() || tool_resp.secondary_clicked())
                    && st.tool.has_panel()
                {
                    st.panel_open = Some(true);
                    st.menu = Menu::None;
                }
            });
    }
    paint_overlay(ctx, &st.overlay, st.touch_ui);
    quick_bar(ctx, st, &g);
    if st.radial_bar && !st.hide_bar {
        radial_bar(ctx, st, &g);
    }
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

    if st.search_open {
        search_panel(ctx, st);
    }
    if st.lib_open {
        library_panel(ctx, st, &mut actions);
    }
    if st.keys_open {
        hotkeys_panel(ctx, st, &mut actions);
    }
    if st.live_open {
        live_panel(ctx, st, &mut actions);
    }
    if st.pages_open {
        pages_panel(ctx, st, &mut actions);
    }
    if st.layout_open {
        layout_panel(ctx, st, &mut actions);
    }
    actions.append(&mut st.queued);

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

/// Where the toolbar list and inventory open, relative to the bar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Above,
    Below,
    Right,
    Left,
}

/// Which set of slots.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Slots {
    /// Toolbar k (the active one is `hotbar`).
    Row(usize),
    Inventory,
}

/// The quick bar (bottom middle) and, when its bag button is on, the
/// inventory above it. Tap a slot to use its tool; tap an empty one to save
/// the current tool there; long-press or right-click for more. In the
/// inventory, tap a slot to pick its tool up and another to put it down.
fn quick_bar(ctx: &egui::Context, st: &mut UiState, g: &Geo) {
    let touch = st.touch_ui;
    let screen = ctx.content_rect();
    let m = if touch { 18.0 } else { 16.0 } * ui_scale(screen).max(0.75);
    let gap = 4.0;
    let mut s = (if touch { 44.0 } else { 38.0 } * ui_scale(screen)).max(26.0);
    // Bottom row, between the panel button and undo / redo, if it fits;
    // else a row above them.
    let right = g.tool.x - g.r - 12.0;
    let left = screen.left() + m + if touch { 64.0 } else { 52.0 };
    // The toolbar switcher and the slots.
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
    // Moved (Edit layout): centred where it was put, as many slots as fit.
    let (n, y, cx) = match st.layout.bar {
        Some(f) => {
            let avail = screen.width() - 2.0 * m;
            let fit = ((avail / (s + gap)) as usize)
                .saturating_sub(1)
                .clamp(3, hotbar::BAR);
            let row = (fit + 1) as f32 * (s + gap) - gap;
            let c = at_frac(screen, f, 0.0);
            (
                fit,
                c.y.clamp(screen.top() + m + s * 0.5, screen.bottom() - m - s * 0.5),
                c.x.clamp(
                    screen.left() + m + row * 0.5,
                    (screen.right() - m - row * 0.5).max(screen.left() + m + row * 0.5),
                ),
            )
        }
        // The tool button moved: the bar stays along the bottom.
        None if st.layout.tool.is_some() => {
            let avail = screen.width() - 2.0 * m;
            let fit = ((avail / (s + gap)) as usize)
                .saturating_sub(1)
                .clamp(3, hotbar::BAR);
            (fit, screen.bottom() - m - s * 0.5, screen.center().x)
        }
        None => (n, y, cx),
    };
    let row = (n + 1) as f32 * (s + gap) - gap;
    let x0 = cx - row * 0.5;
    let current = st.preset();
    let mut fx = SlotFx {
        current,
        changed: false,
        say: None,
    };
    // The other toolbars ticked "shown" stack on the active one, away from
    // the screen edge the bar sits on.
    let up = y > screen.center().y;
    let others: Vec<usize> = (0..st.toolbars.len())
        .filter(|&k| k != st.active_bar && st.toolbars[k].shown)
        .collect();
    let rows = others.len() + 1;
    let step = if up { -(s + gap) } else { s + gap };
    let top = if up {
        y - s * 0.5 - (rows - 1) as f32 * (s + gap)
    } else {
        y - s * 0.5
    };
    let stack = Rect::from_min_size(pos2(x0, top), vec2(row, rows as f32 * (s + gap) - gap));
    // A portrait screen (and no place set in Edit layout): a column down
    // the left side instead, so it has the long side to itself.
    let vertical = !st.radial_bar && st.layout.bar_is_vertical(screen.height() > screen.width());
    // A column on the right half grows (and opens its menus) leftward.
    let mut leftward = false;
    let (n, col_y0, col_x0) = if vertical {
        // Slots as wide as the tool panel's round button below them, on
        // the same centre line (see tool_panel).
        let pr = if touch { 24.0 } else { 19.0 } * ui_scale(screen).max(0.7);
        let pm = if touch { 18.0 } else { 16.0 };
        s = 2.0 * pr;
        // Clear of the round buttons at the top and bottom.
        let avail = screen.height() - 2.0 * (pm + 2.0 * pr + 4.0 + 12.0);
        let fit = (((avail + gap) / (s + gap)) as usize)
            .saturating_sub(1)
            .clamp(3, hotbar::BAR);
        let len = (fit + 1) as f32 * (s + gap) - gap;
        match st.layout.bar {
            // Moved (Edit layout): centred where it was put.
            Some(f) => {
                let c = at_frac(screen, f, 0.0);
                leftward = c.x > screen.center().x;
                (
                    fit,
                    (c.y - len * 0.5).clamp(
                        screen.top() + m,
                        (screen.bottom() - m - len).max(screen.top() + m),
                    ),
                    (c.x - s * 0.5).clamp(
                        screen.left() + m,
                        (screen.right() - m - s).max(screen.left() + m),
                    ),
                )
            }
            None => (
                fit,
                screen.center().y - len * 0.5,
                screen.left() + pm + 2.0 + pr - s * 0.5,
            ),
        }
    } else {
        (n, 0.0, 0.0)
    };
    let portrait = screen.height() > screen.width();
    let (side, stack) = if st.radial_bar && portrait {
        // Radial on a portrait screen: the toolbar list still opens as
        // columns from the left edge, the inventory beside them.
        let len = (hotbar::BAR + 1) as f32 * (s + gap);
        (
            Side::Right,
            Rect::from_min_size(
                pos2(screen.left() + 2.0, screen.center().y - len * 0.5),
                vec2(1.0, len),
            ),
        )
    } else if st.radial_bar {
        (
            Side::Above,
            Rect::from_center_size(g.bar, Vec2::splat(2.0 * g.r)),
        )
    } else if vertical {
        let len = (n + 1) as f32 * (s + gap) - gap;
        let wide = rows as f32 * (s + gap) - gap;
        let left = if leftward { col_x0 + s - wide } else { col_x0 };
        (
            if leftward { Side::Left } else { Side::Right },
            Rect::from_min_size(pos2(left, col_y0), vec2(wide, len)),
        )
    } else {
        (if up { Side::Above } else { Side::Below }, stack)
    };
    // Where cell j (0 = number, 1..n = slots) of row ri goes.
    let cell = |ri: usize, j: usize| -> Pos2 {
        if vertical {
            let dx = ri as f32 * (s + gap);
            pos2(
                if leftward { col_x0 - dx } else { col_x0 + dx },
                col_y0 + j as f32 * (s + gap),
            )
        } else {
            pos2(x0 + j as f32 * (s + gap), y - s * 0.5 + ri as f32 * step)
        }
    };

    // Radial toolbar: the bar is a fan from its own button (see radial_bar).
    if !st.radial_bar && !st.hide_bar {
        egui::Area::new(Id::new("quick_bar"))
            .order(Order::Middle)
            .fixed_pos(stack.min)
            .show(ctx, |ui| {
                ui.set_clip_rect(screen);
                ui.allocate_exact_size(stack.size(), Sense::hover());
                // Row 0 is the active toolbar; the others follow it.
                for (ri, k) in std::iter::once(st.active_bar)
                    .chain(others.iter().copied())
                    .enumerate()
                {
                    let r = Rect::from_min_size(cell(ri, 0), Vec2::splat(s));
                    let active = k == st.active_bar;
                    if number_button(ui, st, r, k, active && st.bar_menu, active) {
                        if active {
                            // Toolbars and the inventory open together.
                            st.bar_menu = !st.bar_menu;
                            st.bag_open = st.bar_menu;
                        } else {
                            st.switch_bar(k);
                        }
                    }
                    for i in 0..n {
                        let r = Rect::from_min_size(cell(ri, i + 1), Vec2::splat(s));
                        slot(ui, st, r, Slots::Row(k), i, &mut fx);
                    }
                }
            });
    }
    // The toolbar list, with the inventory reaching out from it.
    let menu = if st.bar_menu {
        toolbar_menu(ctx, st, stack, side, s, gap, &mut fx)
    } else {
        None
    };
    st.bag_open = st.bar_menu;
    if let (true, Some(menu)) = (st.bag_open, menu) {
        inventory(ctx, st, menu, side, s, gap, &mut fx);
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
        hotbar::grow_inventory(&mut st.inventory);
        st.presets_dirty = true;
    }
    if let Some(m) = fx.say {
        st.message = Some(m);
    }
}

/// A toolbar's number button. True when tapped.
fn number_button(
    ui: &mut egui::Ui,
    st: &UiState,
    r: Rect,
    k: usize,
    open: bool,
    active: bool,
) -> bool {
    let resp = ui.interact(r, Id::new(("bar_num", k)), Sense::click());
    let p = ui.painter();
    rect_shadow(p, r, 8.0);
    p.rect_filled(
        r,
        8.0,
        if resp.hovered() || open {
            Color32::WHITE
        } else {
            FACE
        },
    );
    p.rect_stroke(
        r,
        8.0,
        Stroke::new(
            if open || active { 2.0 } else { 1.0 },
            if open || active { ACCENT } else { EDGE },
        ),
        egui::StrokeKind::Inside,
    );
    if active {
        toolbars_icon(p, r.center(), r.width() * 0.5, k + 1);
    } else {
        p.text(
            r.center(),
            Align2::CENTER_CENTER,
            format!("{}", k + 1),
            egui::FontId::proportional(r.width() * 0.4),
            INKY,
        );
    }
    let tip = if active {
        format!(
            "Toolbar {} — tap to see all toolbars ([ and ] cycle, Alt+1–9 jump)",
            k + 1
        )
    } else {
        format!(
            "Toolbar {} — tap to make it the active one (keys 1–9)",
            k + 1
        )
    };
    let clicked = resp.clicked();
    let _ = st;
    resp.on_hover_text(tip);
    clicked
}

/// The inventory: a big grid over most of the screen (not over the bars,
/// so tools can be carried to them).
fn inventory(
    ctx: &egui::Context,
    st: &mut UiState,
    bars: Rect,
    side: Side,
    s: f32,
    gap: f32,
    fx: &mut SlotFx,
) {
    let screen = ctx.content_rect();
    let m = 12.0;
    // `bars` is the toolbar list: the inventory reaches out from it, away
    // from the quick bar. Beside a column (portrait) it takes the rest of
    // the screen's width, as tall as the list; over it if that is too narrow.
    let side = match side {
        Side::Right | Side::Left => {
            let (l, r) = if side == Side::Right {
                (bars.right() + 8.0, screen.right() - m)
            } else {
                (screen.left() + m, bars.left() - 8.0)
            };
            if r - l >= 3.0 * (s + gap) + 24.0 {
                let top = bars
                    .top()
                    .min(screen.bottom() - m - (4.0 * s + 90.0))
                    .max(screen.top() + m);
                let bottom = bars
                    .bottom()
                    .max(top + 4.0 * s + 90.0)
                    .min(screen.bottom() - m);
                return inventory_at(
                    ctx,
                    st,
                    Rect::from_min_max(pos2(l, top), pos2(r, bottom)),
                    s,
                    gap,
                    fx,
                    true,
                );
            }
            Side::Above
        }
        other => other,
    };
    let side = match side {
        Side::Right | Side::Left => Side::Above,
        other => other,
    };
    // As wide as the toolbar list (at least six slots), centred on it.
    let want = bars
        .width()
        .max(6.0 * (s + gap) + 24.0)
        .min(screen.width() - 2.0 * m);
    let left = (bars.center().x - want * 0.5).clamp(screen.left() + m, screen.right() - m - want);
    let (l, r) = (left, left + want);
    let area = match side {
        Side::Above => Rect::from_min_max(pos2(l, screen.top() + m), pos2(r, bars.top() - 10.0)),
        Side::Below => {
            Rect::from_min_max(pos2(l, bars.bottom() + 10.0), pos2(r, screen.bottom() - m))
        }
        Side::Right => Rect::from_min_max(
            pos2(bars.right() + 10.0, screen.top() + m),
            screen.max - vec2(m, m),
        ),
        Side::Left => Rect::from_min_max(
            screen.min + vec2(m, m),
            pos2(bars.left() - 10.0, screen.bottom() - m),
        ),
    };
    // At most ~60% of the screen tall, next to the bar, so there is canvas
    // around it to tap (which closes it).
    let max_h = (screen.height() * 0.6).max(4.0 * s + 90.0);
    let area = match side {
        Side::Above => Rect::from_min_max(
            pos2(area.left(), area.top().max(area.bottom() - max_h)),
            area.max,
        ),
        Side::Below => Rect::from_min_max(
            area.min,
            pos2(area.right(), area.bottom().min(area.top() + max_h)),
        ),
        Side::Right | Side::Left => {
            let h = area.height().min(max_h);
            let top =
                (bars.center().y - h * 0.5).clamp(area.top(), (area.bottom() - h).max(area.top()));
            Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), h))
        }
    };
    inventory_at(ctx, st, area, s, gap, fx, false);
}

/// The inventory panel filling `area`; with `full` all of it (beside the
/// toolbar list), else at most ~60% of the screen tall.
fn inventory_at(
    ctx: &egui::Context,
    st: &mut UiState,
    area: Rect,
    s: f32,
    gap: f32,
    fx: &mut SlotFx,
    full: bool,
) {
    let screen = ctx.content_rect();
    let max_h = if full {
        area.height()
    } else {
        (screen.height() * 0.6).max(4.0 * s + 90.0)
    };
    let area = Rect::from_min_size(area.min, vec2(area.width(), area.height().min(max_h)));
    if area.height() < 3.0 * s {
        return;
    }
    let inner = area.width() - 24.0;
    let cols = ((inner + gap) / (s + gap)).floor().max(2.0) as usize;
    // Fill the panel with empty slots, like a game's inventory grid.
    let fit = (((area.height() - 90.0) + gap) / (s + gap))
        .floor()
        .max(1.0) as usize;
    let want = (cols * fit).max(st.inventory.len().div_ceil(cols) * cols);
    if st.inventory.len() < want {
        st.inventory.resize(want, None);
    }
    let rows = st.inventory.len().div_ceil(cols);
    egui::Area::new(Id::new("inventory"))
        .order(Order::Foreground)
        .fixed_pos(area.min)
        .show(ctx, |ui| {
            ui.style_mut().visuals = egui::Visuals::light();
            egui::Frame::new()
                .fill(FACE)
                .stroke(Stroke::new(1.0, EDGE))
                .corner_radius(12.0)
                .inner_margin(12.0)
                .shadow(egui::Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_black_alpha(40) })
                .show(ui, |ui| {
                    ui.set_width(inner);
                    ui.set_height(area.height() - 24.0);
                    // Narrow (beside the toolbar list on a phone): icons only,
                    // so the panel keeps to its space.
                    let narrow = inner < 230.0;
                    ui.horizontal(|ui| {
                        if inner >= 150.0 {
                            ui.strong("Inventory");
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("×").on_hover_text("Close").clicked() {
                                st.bag_open = false;
                                st.bar_menu = false;
                            }
                            let bin = ui
                                .add_enabled(st.held.is_some(), egui::Button::new("🗑"))
                                .on_hover_text("Throw away the tool in hand");
                            if bin.clicked() {
                                st.held = None;
                                fx.changed = true;
                            }
                            if ui
                                .button(if narrow { "+" } else { "+ Save current" })
                                .on_hover_text("Keep the current tool and its settings")
                                .clicked()
                            {
                                match st.inventory.iter().position(|x| x.is_none()) {
                                    Some(k) => {
                                        st.inventory[k] = Some(fx.current);
                                        fx.changed = true;
                                    }
                                    None => fx.say = Some("The inventory is full".into()),
                                }
                            }
                        });
                    });
                    ui.label(
                        egui::RichText::new("Tap a tool to pick it up, then tap a slot — here or in a toolbar — to put it there.")
                            .small()
                            .weak(),
                    );
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        let (all, _) = ui.allocate_exact_size(vec2(inner, rows as f32 * (s + gap) - gap), Sense::hover());
                        for i in 0..st.inventory.len() {
                            let r = Rect::from_min_size(
                                all.min + vec2((i % cols) as f32 * (s + gap), (i / cols) as f32 * (s + gap)),
                                Vec2::splat(s),
                            );
                            slot(ui, st, r, Slots::Inventory, i, fx);
                        }
                    });
                });
        });
}

/// Every toolbar, Factorio style: a row of slots each, newest on top. Tick
/// one to keep it showing in the quick bar, tap its number to make it the
/// active one, 🗑 to delete it, + to add one.
fn toolbar_menu(
    ctx: &egui::Context,
    st: &mut UiState,
    bars: Rect,
    side: Side,
    s: f32,
    gap: f32,
    fx: &mut SlotFx,
) -> Option<Rect> {
    let screen = ctx.content_rect();
    let n = hotbar::BAR;
    let check = if st.touch_ui { 30.0 } else { 24.0 };
    // Beside a vertical quick bar (portrait) each toolbar is a column;
    // else a row. `w` is a toolbar's length along it.
    let vert = matches!(side, Side::Right | Side::Left);
    let w = check + 6.0 + (n + 1) as f32 * (s + gap) + s;
    let x = (bars.left() - check - 6.0).clamp(
        screen.left() + 8.0,
        (screen.right() - w - 32.0).max(screen.left() + 8.0),
    );
    let room = match side {
        Side::Above => bars.top() - screen.top() - 24.0,
        Side::Below => screen.bottom() - bars.bottom() - 24.0,
        Side::Right | Side::Left => screen.height() - 24.0,
    };
    let (pivot, at) = match side {
        Side::Above => (Align2::LEFT_BOTTOM, pos2(x, bars.top() - 10.0)),
        Side::Below => (Align2::LEFT_TOP, pos2(x, bars.bottom() + 10.0)),
        // Level with the bar, its slots in line with the bar's slots.
        Side::Right => (
            Align2::LEFT_TOP,
            pos2(
                bars.right() + 10.0,
                (bars.top() - 10.0 - (if st.touch_ui { 34.0 } else { 28.0 }) - check - 6.0)
                    .max(screen.top() + 8.0),
            ),
        ),
        Side::Left => (
            Align2::RIGHT_TOP,
            pos2(
                bars.left() - 10.0,
                (bars.top() - 10.0 - (if st.touch_ui { 34.0 } else { 28.0 }) - check - 6.0)
                    .max(screen.top() + 8.0),
            ),
        ),
    };
    let shown = egui::Area::new(Id::new("toolbar_menu"))
        .order(Order::Foreground)
        .pivot(pivot)
        .fixed_pos(at)
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
                    if vert {
                        // As wide as its columns (at least the header), but
                        // never wider than the room right of the bar.
                        // Up to four columns wide; more scroll sideways.
                        let cols = st.toolbars.len().min(4) as f32 * (s + gap) - gap;
                        let room_w = if side == Side::Left {
                            at.x - screen.left() - 12.0 - 20.0
                        } else {
                            screen.right() - at.x - 12.0 - 20.0
                        };
                        ui.set_max_width(cols.max(2.0 * (s + gap)).min(room_w));
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .button(if vert { "+" } else { "+ Add toolbar" })
                            .on_hover_text("Add a new, empty toolbar")
                            .clicked()
                        {
                            let k = st.toolbars.len();
                            st.toolbars
                                .push(hotbar::empty_bar(format!("Toolbar {}", k + 1)));
                            st.switch_bar(k);
                            fx.changed = true;
                        }
                        if !vert {
                            ui.label(
                                egui::RichText::new(
                                    "Tick to keep showing · tap a number to use it",
                                )
                                .small()
                                .weak(),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("×").on_hover_text("Close").clicked() {
                                st.bar_menu = false;
                            }
                        });
                    });
                    ui.add_space(4.0);
                    let mut gone = None;
                    let total = st.toolbars.len();
                    let across = total as f32 * (s + gap) - gap;
                    let area = if vert {
                        egui::ScrollArea::horizontal()
                    } else {
                        egui::ScrollArea::vertical().max_height((room - 60.0).max(s + gap))
                    };
                    area.show(ui, |ui| {
                        let (all, _) = ui.allocate_exact_size(
                            if vert {
                                vec2(across, w)
                            } else {
                                vec2(w, across)
                            },
                            Sense::hover(),
                        );
                        // Newest on top (in columns: nearest the bar).
                        for (row, k) in (0..total).rev().enumerate() {
                            // Where item `along` (px from the toolbar's
                            // start) of this toolbar goes.
                            let at = |along: f32| -> Pos2 {
                                let off = row as f32 * (s + gap);
                                if vert {
                                    pos2(all.left() + off, all.top() + along)
                                } else {
                                    pos2(all.left() + along, all.top() + off)
                                }
                            };
                            let active = k == st.active_bar;
                            // Shown tick (the active one always shows).
                            let cr = Rect::from_center_size(
                                at(check * 0.5)
                                    + if vert {
                                        vec2(s * 0.5, 0.0)
                                    } else {
                                        vec2(0.0, s * 0.5)
                                    },
                                Vec2::splat(check),
                            );
                            let resp = ui.interact(cr, Id::new(("bar_shown", k)), Sense::click());
                            let on = active || st.toolbars[k].shown;
                            let p = ui.painter();
                            p.rect_filled(
                                cr.shrink(3.0),
                                5.0,
                                if on { ACCENT } else { Color32::WHITE },
                            );
                            p.rect_stroke(
                                cr.shrink(3.0),
                                5.0,
                                Stroke::new(1.2, if on { ACCENT } else { EDGE }),
                                egui::StrokeKind::Inside,
                            );
                            if on {
                                let c = cr.center();
                                let d = check * 0.18;
                                p.add(Shape::line(
                                    vec![
                                        c + vec2(-d * 1.2, 0.0),
                                        c + vec2(-d * 0.2, d),
                                        c + vec2(d * 1.4, -d),
                                    ],
                                    Stroke::new(2.0, Color32::WHITE),
                                ));
                            }
                            if resp.clicked() && !active {
                                st.toolbars[k].shown = !st.toolbars[k].shown;
                                fx.changed = true;
                            }
                            resp.on_hover_text(if active {
                                "The active toolbar always shows"
                            } else {
                                "Show this toolbar in the quick bar"
                            });
                            let x0 = check + 6.0;
                            let nr = Rect::from_min_size(at(x0), Vec2::splat(s));
                            if number_button(ui, st, nr, k, false, active) && !active {
                                st.switch_bar(k);
                            }
                            for i in 0..n {
                                let r = Rect::from_min_size(
                                    at(x0 + (i + 1) as f32 * (s + gap)),
                                    Vec2::splat(s),
                                );
                                slot(ui, st, r, Slots::Row(k), i, fx);
                            }
                            // Delete.
                            let dr = Rect::from_min_size(
                                at(x0 + (n + 1) as f32 * (s + gap)),
                                Vec2::splat(s),
                            );
                            let resp = ui.interact(dr, Id::new(("bar_del", k)), Sense::click());
                            let can = total > 1;
                            let p = ui.painter();
                            p.rect_filled(
                                dr,
                                8.0,
                                if resp.hovered() && can {
                                    Color32::from_rgb(255, 235, 238)
                                } else {
                                    FACE
                                },
                            );
                            p.rect_stroke(
                                dr,
                                8.0,
                                Stroke::new(1.0, EDGE),
                                egui::StrokeKind::Inside,
                            );
                            bin_icon(
                                p,
                                dr.center(),
                                s * 0.5,
                                if can { INKY } else { Color32::from_gray(190) },
                            );
                            if resp.clicked() && can {
                                gone = Some(k);
                            }
                            resp.on_hover_text(if can {
                                "Delete this toolbar (its tools are not kept)"
                            } else {
                                "The last toolbar stays"
                            });
                        }
                    });
                    if let Some(k) = gone {
                        if k == st.active_bar {
                            st.switch_bar(if k + 1 < st.toolbars.len() {
                                k + 1
                            } else {
                                k - 1
                            });
                        }
                        st.toolbars.remove(k);
                        if st.active_bar > k {
                            st.active_bar -= 1;
                        }
                        fx.changed = true;
                    }
                });
        });
    let mut r = shown.response.rect;
    if vert {
        // Its width as laid out this frame (the area's own rect can lag a
        // frame behind when toolbars are added).
        let cols = st.toolbars.len().min(4) as f32 * (s + gap) - gap;
        let room_w = if side == Side::Left {
            at.x - screen.left() - 12.0 - 20.0
        } else {
            screen.right() - at.x - 12.0 - 20.0
        };
        let w = cols.max(2.0 * (s + gap)).min(room_w) + 20.0 + 2.0;
        if side == Side::Left {
            r.min.x = r.max.x - w;
        } else {
            r.max.x = r.min.x + w;
        }
    }
    Some(r)
}

/// A bin.
fn bin_icon(p: &egui::Painter, c: Pos2, r: f32, col: Color32) {
    let st = Stroke::new((r * 0.09).max(1.2), col);
    let s = r * 0.55;
    p.line_segment([c + vec2(-s, -s * 0.6), c + vec2(s, -s * 0.6)], st);
    p.add(Shape::line(
        vec![
            c + vec2(-s * 0.35, -s * 0.6),
            c + vec2(-s * 0.25, -s * 0.9),
            c + vec2(s * 0.25, -s * 0.9),
            c + vec2(s * 0.35, -s * 0.6),
        ],
        st,
    ));
    p.add(Shape::line(
        vec![
            c + vec2(-s * 0.75, -s * 0.6),
            c + vec2(-s * 0.6, s),
            c + vec2(s * 0.6, s),
            c + vec2(s * 0.75, -s * 0.6),
        ],
        st,
    ));
    p.line_segment(
        [c + vec2(-s * 0.2, -s * 0.2), c + vec2(-s * 0.15, s * 0.65)],
        st,
    );
    p.line_segment(
        [c + vec2(s * 0.2, -s * 0.2), c + vec2(s * 0.15, s * 0.65)],
        st,
    );
}

/// Stacked bars with the active toolbar's number: the toolbar switcher.
fn toolbars_icon(p: &egui::Painter, c: Pos2, r: f32, number: usize) {
    for k in 0..3 {
        let o = vec2(-r * 0.12, -r * 0.12) * (2 - k) as f32;
        let rect =
            Rect::from_center_size(c + o + vec2(r * 0.08, r * 0.08), vec2(r * 1.1, r * 0.75));
        p.rect_filled(
            rect,
            3.0,
            if k == 2 {
                Color32::WHITE
            } else {
                Color32::from_gray(225)
            },
        );
        p.rect_stroke(rect, 3.0, Stroke::new(1.2, INKY), egui::StrokeKind::Inside);
    }
    p.text(
        c + vec2(r * 0.08, r * 0.1),
        Align2::CENTER_CENTER,
        format!("{number}"),
        egui::FontId::proportional(r * 0.62),
        INKY,
    );
}

/// What tapping slots did this frame.
struct SlotFx {
    current: Preset,
    changed: bool,
    say: Option<String>,
}

fn slot_mut(st: &mut UiState, which: Slots, i: usize) -> &mut Option<Preset> {
    match which {
        Slots::Row(k) if k == st.active_bar => &mut st.hotbar[i],
        Slots::Row(k) => &mut st.toolbars[k].slots[i],
        Slots::Inventory => &mut st.inventory[i],
    }
}

/// One slot: draws it and handles taps.
fn slot(ui: &mut egui::Ui, st: &mut UiState, rect: Rect, which: Slots, i: usize, fx: &mut SlotFx) {
    let current = fx.current;
    let resp = ui.interact(rect, Id::new(("slot", which, i)), Sense::click());
    let item = *slot_mut(st, which, i);
    let p = ui.painter();
    let on = !st.bag_open && item.is_some_and(|it| it == current);
    rect_shadow(p, rect, 8.0);
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
    if matches!(which, Slots::Row(_)) {
        p.text(
            rect.left_top() + vec2(5.0, 3.0),
            Align2::LEFT_TOP,
            format!("{}", i + 1),
            egui::FontId::proportional(9.0),
            Color32::from_gray(150),
        );
    }
    let view_name = item.and_then(|it| it.view).map(|v| {
        st.views
            .get(&v)
            .map_or("A view".to_string(), |v| v.name.clone())
    });
    match (&item, &view_name) {
        (Some(it), _) if it.cmd.is_some() => {
            let redo = it.cmd == Some(hotbar::SlotCmd::Redo);
            let on = if redo { st.can_redo } else { st.can_undo };
            undo_icon(
                p,
                rect.center(),
                rect.width() * 0.4,
                redo,
                if on { INKY } else { Color32::from_gray(190) },
            );
        }
        (Some(_), Some(name)) => view_icon(p, rect.center(), rect.width() * 0.5, name),
        (Some(it), None) => preset_icon(p, rect.center(), rect.width() * 0.5, it, &st.egui_fonts),
        _ => {}
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
        Some(_) if view_name.is_some() => {
            format!("Fly to {}", view_name.clone().unwrap_or_default())
        }
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
        if let Some(v) = st.view_now.clone() {
            if ui
                .button("Save this view here")
                .on_hover_text("Tap the slot later to fly back")
                .clicked()
            {
                let n = st.views.len() + 1;
                let id = st.add_view(hotbar::View {
                    name: format!("View {n}"),
                    ..v
                });
                *slot_mut(st, which, i) = Some(Preset::view(id));
                fx.changed = true;
                ui.close();
            }
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
    match p.cmd {
        Some(hotbar::SlotCmd::Undo) => return "Undo".into(),
        Some(hotbar::SlotCmd::Redo) => return "Redo".into(),
        None => {}
    }
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
        let w = (i.width * 0.35).min(r * 0.3).max(1.0);
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
/// A circular arrow (rotate).
fn rotate_icon(p: &egui::Painter, c: Pos2, r: f32, col: Color32) {
    let stroke = Stroke::new((r * 0.22).max(1.4), col);
    let (a0, a1) = (-2.6_f32, 1.9_f32);
    let pts: Vec<Pos2> = (0..=16)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / 16.0;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    let end = *pts.last().unwrap();
    p.add(Shape::line(pts, stroke));
    // Arrowhead at the end, pointing along the turn.
    let t = vec2(-a1.sin(), a1.cos());
    let n = vec2(a1.cos(), a1.sin());
    let h = r * 0.55;
    p.add(Shape::line(
        vec![end - t * h + n * h * 0.6, end, end - t * h - n * h * 0.6],
        stroke,
    ));
}

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
    if st.hide_panel || !tool.has_panel() || (tool.selects() && st.sel.count == 0) {
        return;
    }
    let touch = st.touch_ui;
    let m = if touch { 18.0 } else { 16.0 };
    let screen = ctx.content_rect();
    // Bottom left by default (in thumb reach on phones, out of the way on
    // desktops); any corner via Edit layout.
    let pc = if st.radial_bar {
        crate::layout::Corner::TopLeft
    } else {
        st.layout.panel
    };
    let align = match pc {
        crate::layout::Corner::BottomLeft => Align2::LEFT_BOTTOM,
        crate::layout::Corner::BottomRight => Align2::RIGHT_BOTTOM,
        crate::layout::Corner::TopLeft => Align2::LEFT_TOP,
        crate::layout::Corner::TopRight => Align2::RIGHT_TOP,
    };
    let corner = vec2(
        if pc.is_right() { -m } else { m },
        // Top right sits under the settings button.
        match pc {
            crate::layout::Corner::TopRight => m + if touch { 70.0 } else { 60.0 },
            crate::layout::Corner::TopLeft => m,
            _ => -m,
        },
    );
    // Open by default where there is room; tucked away on phones.
    let open = *st.panel_open.get_or_insert(screen.width() >= 700.0);

    if !open {
        // Collapsed: a small round button showing the tool and its color.
        let r = if touch { 24.0 } else { 19.0 } * ui_scale(screen).max(0.7);
        let col = tool_color(st, tool);
        egui::Area::new(Id::new("tool_panel_btn"))
            .order(Order::Foreground)
            .anchor(align, corner)
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
        .anchor(align, corner)
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
                let title = if tool.selects() {
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
                            Tool::Pen => {
                                brush_section(ui, &mut st.pen, touch, &mut dial_hue, false)
                            }
                            Tool::Texture => {
                                brush_section(ui, &mut st.texture, touch, &mut dial_hue, true)
                            }
                            Tool::Highlighter => ink_section(
                                ui,
                                &mut st.highlighter,
                                tool,
                                touch,
                                &mut dial_hue,
                                true,
                            ),
                            Tool::Bucket => fill_section(
                                ui,
                                &mut st.fill,
                                &mut st.fill_gap,
                                touch,
                                &mut dial_hue,
                            ),
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
                            Tool::Select | Tool::Lasso => {
                                select_actions(ui, st.sel.kind, st.sel.count, st.cropping, actions);
                                match st.sel.kind {
                                    SelKind::Ink => match st.sel.ink.as_mut() {
                                        Some(ink) => ink_section(
                                            ui,
                                            ink,
                                            Tool::Texture,
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

/// The bucket: a swatch, gap closing, opacity and color.
fn fill_section(
    ui: &mut egui::Ui,
    ink: &mut InkSettings,
    gap: &mut u8,
    touch: bool,
    dial_hue: &mut f32,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let [r, g, b, a] = ink.rgba();
    ui.painter().rect_filled(
        rect.shrink2(vec2(14.0, 4.0)),
        6.0,
        Color32::from_rgba_unmultiplied(r, g, b, a),
    );
    ui.label(
        egui::RichText::new("Tap inside a closed outline to fill it.")
            .small()
            .weak(),
    );
    heading(ui, "Close gaps");
    ui.horizontal(|ui| {
        for (v, name, tip) in [
            (0u8, "Off", "Only fill fully closed outlines"),
            (2, "Small", "Bridge gaps of a few points"),
            (5, "Medium", "Bridge gaps up to about 10 points"),
            (9, "Large", "Bridge gaps up to about 18 points"),
        ] {
            if ui
                .selectable_label(*gap == v, name)
                .on_hover_text(tip)
                .clicked()
            {
                *gap = v;
            }
        }
    });
    opacity_slider(ui, &mut ink.opacity);
    heading(ui, "Color");
    color_dial(ui, &mut ink.color, false, dial_hue, touch)
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
fn select_actions(
    ui: &mut egui::Ui,
    kind: SelKind,
    count: usize,
    cropping: bool,
    actions: &mut Vec<Action>,
) {
    if cropping {
        ui.label("Drag the edges or corners; drag inside to slide.");
        ui.horizontal(|ui| {
            if ui.button("Done").on_hover_text("Enter").clicked() {
                actions.push(Action::CropDone);
            }
            if ui.button("Cancel").on_hover_text("Esc").clicked() {
                actions.push(Action::CropCancel);
            }
        });
        return;
    }
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
        if ui
            .button("Add to library")
            .on_hover_text("Save it to place copies anywhere (Settings > Library)")
            .clicked()
        {
            actions.push(Action::SaveSticker);
        }
        if kind == SelKind::Images
            && count == 1
            && ui
                .button("Crop")
                .on_hover_text("Crop the picture")
                .clicked()
        {
            actions.push(Action::Crop);
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
    for (at, name, col, dir) in &ov.labels {
        let font = egui::FontId::proportional(12.0);
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(name.clone(), font, Color32::WHITE));
        let size = galley.size() + vec2(12.0, 6.0);
        let r = match dir {
            // Off screen: the pill sits inside the edge, an arrow toward them.
            Some(d) => {
                let tip = *at + *d * 10.0;
                let back = *at - *d * 2.0;
                let side = vec2(-d.y, d.x) * 6.0;
                p.add(Shape::convex_polygon(
                    vec![tip, back + side, back - side],
                    *col,
                    Stroke::NONE,
                ));
                let c = *at - *d * (size.x.max(size.y) * 0.5 + 4.0);
                Rect::from_center_size(c, size)
            }
            None => {
                p.circle_filled(*at, 5.0, *col);
                p.circle_stroke(*at, 5.0, Stroke::new(1.5, Color32::WHITE));
                Rect::from_min_size(*at + vec2(8.0, -size.y - 2.0), size)
            }
        };
        p.rect_filled(r, 6.0, *col);
        p.galley(r.min + vec2(6.0, 3.0), galley, Color32::WHITE);
    }
    let blue = Color32::from_rgb(70, 110, 230);
    if let Some(r) = ov.marquee {
        p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(70, 110, 230, 24));
        p.rect_stroke(r, 0.0, Stroke::new(1.0, blue), egui::StrokeKind::Middle);
    }
    if let Some((pts, mids)) = &ov.joints {
        let hs = if touch { 8.0 } else { 6.0 };
        for m in mids {
            let r = hs * 1.1;
            p.circle_filled(*m, r, Color32::from_rgba_unmultiplied(255, 255, 255, 235));
            p.circle_stroke(*m, r, Stroke::new(1.2, blue));
            p.line_segment(
                [*m - vec2(r * 0.55, 0.0), *m + vec2(r * 0.55, 0.0)],
                Stroke::new(1.6, blue),
            );
            p.line_segment(
                [*m - vec2(0.0, r * 0.55), *m + vec2(0.0, r * 0.55)],
                Stroke::new(1.6, blue),
            );
        }
        for (i, q) in pts.iter().enumerate() {
            let end = i == 0 || i + 1 == pts.len();
            p.circle_filled(*q, hs, if end { blue } else { Color32::WHITE });
            p.circle_stroke(
                *q,
                hs,
                Stroke::new(1.5, if end { Color32::WHITE } else { blue }),
            );
        }
    }
    if let Some(l) = &ov.lasso {
        if l.len() > 1 {
            // Closing edge faint, the drawn loop dashed.
            p.line_segment(
                [l[l.len() - 1], l[0]],
                Stroke::new(1.0, blue.gamma_multiply(0.4)),
            );
            p.extend(Shape::dashed_line(l, Stroke::new(1.5, blue), 6.0, 4.0));
        }
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
/// Slots for `n` buttons on rings around a button, along an arc of `span`
/// radians: (radius, fraction along the arc).
fn ring_slots(n: usize, r: f32, span: f32) -> Vec<(f32, f32)> {
    let rr = r * 0.9;
    let along = 2.0 * rr + 8.0;
    // Room between rings for the labels.
    // Room for the labels; less on small screens (the buttons are smaller).
    let step = 2.0 * rr + 56.0 * (r / 24.0).clamp(0.55, 1.0);
    let full = span >= TAU - 1e-3;
    // A full ring starts closer in: it has room all round.
    let mut radius = if full { r * 3.2 } else { r * 4.4 };
    let mut out = Vec::with_capacity(n);
    let mut left = n;
    while left > 0 {
        let cap = if full {
            ((span * radius / along).floor() as usize).max(1)
        } else {
            ((span * radius / along).floor() as usize + 1).max(1)
        };
        let m = cap.min(left);
        for i in 0..m {
            let frac = if full {
                i as f32 / m as f32
            } else if m == 1 {
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
    let slots = ring_slots(items.len(), g.r, g.app_arc.1);
    let mut bbox = Rect::from_center_size(g.app, Vec2::splat(2.0 * g.r)).union(
        Rect::from_center_size(g.app + vec2(0.0, g.r + 12.0), vec2(2.0 * g.r + 24.0, 18.0)),
    );
    if open > 0.0 {
        let reach = fan_reach(&slots, g.r) * open + g.r * 1.2;
        bbox = bbox.union(Rect::from_center_size(
            g.app,
            Vec2::splat(2.0 * (reach + 50.0)),
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
                    // Along the arc toward the middle of the screen.
                    let (radius, frac) = slots[i];
                    let a = g.app_arc.0 + g.app_arc.1 * frac;
                    let pc = g.app + Vec2::angled(a) * radius * open;
                    let rr = g.r * 0.9 * open.max(0.3);
                    let resp = ui.interact(
                        Rect::from_center_size(pc, Vec2::splat(2.0 * rr)),
                        Id::new(("app_item", i)),
                        Sense::click(),
                    );
                    let active = (item == AppItem::Timeline && st.timeline_on)
                        || (item == AppItem::Grid && st.grid != GridMode::Off)
                        || (item == AppItem::Dark && st.dark)
                        || (item == AppItem::Folder && st.folder_on)
                        || (item == AppItem::Live && st.live.is_some())
                        || (item == AppItem::Diagram && st.diagram)
                        || (item == AppItem::RadialBar && st.radial_bar)
                        || (item == AppItem::ShowTools && !st.hide_tools)
                        || (item == AppItem::ShowPanel && !st.hide_panel)
                        || (item == AppItem::ShowBar && !st.hide_bar);
                    disc(
                        &p,
                        pc,
                        rr,
                        if resp.hovered() { Color32::WHITE } else { FACE },
                        active,
                    );
                    app_icon(
                        &p,
                        pc,
                        rr,
                        item,
                        st.grid,
                        (st.hide_tools, st.hide_panel, st.hide_bar),
                    );
                    if open > 0.9 {
                        let out = Vec2::angled(a);
                        let name = match item {
                            AppItem::ShowTools if st.hide_tools => "Tool button: hidden",
                            AppItem::ShowTools => "Tool button: shown",
                            AppItem::ShowPanel if st.hide_panel => "Tool panel: hidden",
                            AppItem::ShowPanel => "Tool panel: shown",
                            AppItem::ShowBar if st.hide_bar => "Quick toolbar: hidden",
                            AppItem::ShowBar => "Quick toolbar: shown",
                            AppItem::Folder if st.folder_on => "Sync folder: on",
                            AppItem::Folder => "Sync folder: off",
                            AppItem::Dark if st.dark => "Dark mode: on",
                            AppItem::Dark => "Dark mode: off",
                            AppItem::Diagram if st.diagram => "Diagram: on",
                            AppItem::Diagram => "Diagram: off",
                            AppItem::Grid => match st.grid {
                                GridMode::Off => "Grid: off",
                                GridMode::Lines => "Grid: lines",
                                GridMode::Dots => "Grid: dots",
                            },
                            _ => item.name(),
                        };
                        label(&p, pc + out * (rr + 18.0) + vec2(0.0, 2.0), name);
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

fn app_icon(
    p: &egui::Painter,
    c: Pos2,
    r: f32,
    item: AppItem,
    grid: GridMode,
    st_hidden: (bool, bool, bool),
) {
    let s = r * 0.42;
    let st = Stroke::new(r * 0.09, INKY);
    let line = |pts: &[Vec2]| {
        p.add(Shape::line(pts.iter().map(|v| c + *v * s).collect(), st));
    };
    match item {
        AppItem::ShowTools | AppItem::ShowPanel | AppItem::ShowBar => {
            // An eye (struck through when hidden), over what it is for.
            let hidden = match item {
                AppItem::ShowTools => st_hidden.0,
                AppItem::ShowPanel => st_hidden.1,
                _ => st_hidden.2,
            };
            let eye_c = c + vec2(0.0, -0.25) * s;
            line(&[
                vec2(-0.9, -0.25),
                vec2(-0.45, -0.6),
                vec2(0.0, -0.7),
                vec2(0.45, -0.6),
                vec2(0.9, -0.25),
            ]);
            line(&[
                vec2(-0.9, -0.25),
                vec2(-0.45, 0.1),
                vec2(0.0, 0.2),
                vec2(0.45, 0.1),
                vec2(0.9, -0.25),
            ]);
            p.circle_stroke(eye_c, s * 0.22, st);
            if hidden {
                line(&[vec2(-0.8, 0.3), vec2(0.8, -0.8)]);
            }
            match item {
                AppItem::ShowTools => {
                    p.circle_stroke(c + vec2(0.0, 0.7) * s, s * 0.28, st);
                }
                AppItem::ShowPanel => {
                    line(&[
                        vec2(-0.45, 0.45),
                        vec2(0.45, 0.45),
                        vec2(0.45, 1.0),
                        vec2(-0.45, 1.0),
                        vec2(-0.45, 0.45),
                    ]);
                }
                _ => {
                    for x in [-0.6, -0.2, 0.2, 0.6] {
                        let r = Rect::from_center_size(c + vec2(x, 0.75) * s, Vec2::splat(s * 0.3));
                        p.rect_stroke(r, 1.5, st, egui::StrokeKind::Middle);
                    }
                }
            }
        }
        AppItem::LayoutMenu => {
            // Four tiles: a layout.
            p.rect_stroke(
                Rect::from_min_max(c + vec2(-0.9, -0.9) * s, c + vec2(-0.1, -0.1) * s),
                2.0,
                st,
                egui::StrokeKind::Middle,
            );
            p.rect_stroke(
                Rect::from_min_max(c + vec2(0.1, -0.9) * s, c + vec2(0.9, -0.1) * s),
                2.0,
                st,
                egui::StrokeKind::Middle,
            );
            p.rect_stroke(
                Rect::from_min_max(c + vec2(-0.9, 0.1) * s, c + vec2(0.9, 0.9) * s),
                2.0,
                st,
                egui::StrokeKind::Middle,
            );
        }
        AppItem::Pages => {
            // A stack of pages.
            line(&[vec2(-0.55, -0.95), vec2(0.75, -0.95), vec2(0.75, 0.55)]);
            line(&[
                vec2(-0.8, -0.7),
                vec2(0.5, -0.7),
                vec2(0.5, 0.9),
                vec2(-0.8, 0.9),
                vec2(-0.8, -0.7),
            ]);
            line(&[vec2(-0.5, -0.25), vec2(0.2, -0.25)]);
            line(&[vec2(-0.5, 0.15), vec2(0.2, 0.15)]);
        }
        AppItem::Live => {
            // Two people.
            p.circle_stroke(c + vec2(-0.4, -0.35) * s, s * 0.3, st);
            p.circle_stroke(c + vec2(0.45, -0.25) * s, s * 0.26, st);
            line(&[
                vec2(-0.95, 0.85),
                vec2(-0.85, 0.25),
                vec2(-0.4, 0.1),
                vec2(0.05, 0.25),
                vec2(0.15, 0.85),
            ]);
            line(&[
                vec2(0.3, 0.75),
                vec2(0.4, 0.3),
                vec2(0.75, 0.2),
                vec2(0.95, 0.35),
                vec2(1.0, 0.75),
            ]);
        }
        AppItem::Folder => {
            // A folder with two arrows going round.
            line(&[
                vec2(-0.95, -0.6),
                vec2(-0.35, -0.6),
                vec2(-0.15, -0.35),
                vec2(0.95, -0.35),
                vec2(0.95, 0.8),
                vec2(-0.95, 0.8),
                vec2(-0.95, -0.6),
            ]);
            line(&[vec2(-0.4, 0.05), vec2(0.35, 0.05), vec2(0.15, -0.12)]);
            line(&[vec2(0.4, 0.45), vec2(-0.35, 0.45), vec2(-0.15, 0.62)]);
        }
        AppItem::Changes => {
            // A page with a plus: just the new bits.
            line(&[
                vec2(-0.7, -0.9),
                vec2(0.3, -0.9),
                vec2(0.7, -0.5),
                vec2(0.7, 0.9),
                vec2(-0.7, 0.9),
                vec2(-0.7, -0.9),
            ]);
            line(&[vec2(0.0, -0.3), vec2(0.0, 0.5)]);
            line(&[vec2(-0.4, 0.1), vec2(0.4, 0.1)]);
        }
        AppItem::Merge => {
            // Two lines joining into one.
            line(&[
                vec2(-0.8, -0.85),
                vec2(-0.8, -0.2),
                vec2(0.0, 0.35),
                vec2(0.0, 0.95),
            ]);
            line(&[vec2(0.8, -0.85), vec2(0.8, -0.2), vec2(0.0, 0.35)]);
            line(&[vec2(-0.3, 0.65), vec2(0.0, 0.95), vec2(0.3, 0.65)]);
        }
        AppItem::Import => {
            // An arrow down into a tray.
            line(&[vec2(0.0, -0.95), vec2(0.0, 0.35)]);
            line(&[vec2(-0.45, -0.1), vec2(0.0, 0.35), vec2(0.45, -0.1)]);
            line(&[
                vec2(-0.9, 0.3),
                vec2(-0.9, 0.9),
                vec2(0.9, 0.9),
                vec2(0.9, 0.3),
            ]);
        }
        AppItem::Dark => {
            // A crescent moon.
            p.circle_filled(c, s * 0.85, INKY);
            p.circle_filled(c + vec2(0.45, -0.35) * s, s * 0.72, FACE);
        }
        AppItem::RadialBar => {
            // A quarter fan of slots around a corner button.
            p.circle_stroke(c + vec2(0.7, 0.7) * s, s * 0.32, st);
            for k in 0..4 {
                let a = PI + FRAC_PI_2 * k as f32 / 3.0;
                let q = c + vec2(0.7, 0.7) * s + Vec2::angled(a) * s * 1.25;
                let rr = Rect::from_center_size(q, Vec2::splat(s * 0.42));
                p.rect_stroke(rr, 2.0, st, egui::StrokeKind::Middle);
            }
        }
        AppItem::Hotkeys => {
            // A keyboard.
            line(&[
                vec2(-1.0, -0.6),
                vec2(1.0, -0.6),
                vec2(1.0, 0.6),
                vec2(-1.0, 0.6),
                vec2(-1.0, -0.6),
            ]);
            for y in [-0.25, 0.1] {
                for x in [-0.65, -0.3, 0.05, 0.4, 0.7] {
                    p.circle_filled(c + vec2(x, y) * s, r * 0.045, INKY);
                }
            }
            line(&[vec2(-0.45, 0.38), vec2(0.45, 0.38)]);
        }
        AppItem::Layout => {
            // Four tiles, one lifted and moving.
            for (x, y) in [(-0.95, -0.95), (0.15, -0.95), (-0.95, 0.15)] {
                line(&[
                    vec2(x, y),
                    vec2(x + 0.8, y),
                    vec2(x + 0.8, y + 0.8),
                    vec2(x, y + 0.8),
                    vec2(x, y),
                ]);
            }
            line(&[
                vec2(0.3, 0.3),
                vec2(1.0, 0.3),
                vec2(1.0, 1.0),
                vec2(0.3, 1.0),
                vec2(0.3, 0.3),
            ]);
            line(&[vec2(0.1, 0.55), vec2(0.1, 0.1), vec2(0.55, 0.1)]);
        }
        AppItem::Diagram => {
            // Two boxes joined by an arrow.
            line(&[
                vec2(-1.0, -0.95),
                vec2(-0.3, -0.95),
                vec2(-0.3, -0.35),
                vec2(-1.0, -0.35),
                vec2(-1.0, -0.95),
            ]);
            line(&[
                vec2(0.3, 0.35),
                vec2(1.0, 0.35),
                vec2(1.0, 0.95),
                vec2(0.3, 0.95),
                vec2(0.3, 0.35),
            ]);
            line(&[vec2(-0.65, -0.35), vec2(-0.65, 0.65), vec2(0.3, 0.65)]);
            line(&[vec2(0.05, 0.42), vec2(0.3, 0.65), vec2(0.05, 0.88)]);
        }
        AppItem::Library => {
            // Two books on a shelf, one leaning.
            line(&[vec2(-0.9, 0.95), vec2(0.95, 0.95)]);
            line(&[
                vec2(-0.75, 0.95),
                vec2(-0.75, -0.75),
                vec2(-0.3, -0.75),
                vec2(-0.3, 0.95),
            ]);
            line(&[vec2(-0.75, -0.35), vec2(-0.3, -0.35)]);
            line(&[
                vec2(-0.1, 0.95),
                vec2(0.35, -0.8),
                vec2(0.8, -0.65),
                vec2(0.4, 0.95),
            ]);
        }
        AppItem::Paste => {
            // A clipboard.
            line(&[
                vec2(-0.35, -0.8),
                vec2(-0.8, -0.8),
                vec2(-0.8, 1.0),
                vec2(0.8, 1.0),
                vec2(0.8, -0.8),
                vec2(0.35, -0.8),
            ]);
            line(&[
                vec2(-0.35, -1.0),
                vec2(0.35, -1.0),
                vec2(0.35, -0.6),
                vec2(-0.35, -0.6),
                vec2(-0.35, -1.0),
            ]);
            line(&[vec2(-0.45, -0.1), vec2(0.45, -0.1)]);
            line(&[vec2(-0.45, 0.35), vec2(0.45, 0.35)]);
        }
        AppItem::Export => {
            // A tray with an arrow leaving it.
            line(&[vec2(0.0, 0.35), vec2(0.0, -1.0)]);
            line(&[vec2(-0.45, -0.55), vec2(0.0, -1.0), vec2(0.45, -0.55)]);
            line(&[
                vec2(-0.9, 0.1),
                vec2(-0.9, 0.9),
                vec2(0.9, 0.9),
                vec2(0.9, 0.1),
            ]);
        }
        AppItem::Search => {
            // A magnifying glass.
            p.circle_stroke(c + vec2(-0.2, -0.2) * s, s * 0.62, st);
            line(&[vec2(0.25, 0.25), vec2(0.95, 0.95)]);
        }
        AppItem::Grid => {
            // A 3x3 grid of lines or dots (whichever the next tap gives).
            if grid == GridMode::Lines {
                for k in [-1.0, 0.0, 1.0] {
                    p.circle_filled(c + vec2(-0.75, k * 0.75) * s, r * 0.06, INKY);
                    p.circle_filled(c + vec2(0.0, k * 0.75) * s, r * 0.06, INKY);
                    p.circle_filled(c + vec2(0.75, k * 0.75) * s, r * 0.06, INKY);
                }
            } else {
                for k in [-0.75, 0.0, 0.75] {
                    line(&[vec2(-1.0, k), vec2(1.0, k)]);
                    line(&[vec2(k, -1.0), vec2(k, 1.0)]);
                }
            }
        }
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

/// A soft drop shadow under a round button: a few faint layers, offset down.
fn disc_shadow(p: &egui::Painter, c: Pos2, r: f32) {
    for (dy, grow, a) in [(3.0, 3.5, 8u8), (2.2, 2.0, 14), (1.4, 0.8, 22)] {
        p.circle_filled(c + vec2(0.0, dy), r + grow, Color32::from_black_alpha(a));
    }
}

/// The same under a rounded square (quick-bar slots).
fn rect_shadow(p: &egui::Painter, r: Rect, rounding: f32) {
    for (dy, grow, a) in [(3.0, 2.5, 7u8), (2.0, 1.2, 12), (1.2, 0.4, 18)] {
        p.rect_filled(
            r.expand(grow).translate(vec2(0.0, dy)),
            rounding + grow,
            Color32::from_black_alpha(a),
        );
    }
}

fn disc(p: &egui::Painter, c: Pos2, r: f32, fill: Color32, active: bool) {
    disc_shadow(p, c, r);
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
            // A paintbrush: handle, ferrule, and a tip dipped in the ink color.
            let b = Barrel { c, s: r * 0.62 };
            b.quad(p, (0.15, 0.1), (0.98, 0.1), FACE, line);
            b.quad(p, (-0.2, 0.16), (0.15, 0.16), Color32::from_gray(205), line);
            p.add(Shape::convex_polygon(
                vec![
                    b.at(-0.2, -0.17),
                    b.at(-0.55, -0.14),
                    b.at(-0.85, 0.0),
                    b.at(-0.55, 0.14),
                    b.at(-0.2, 0.17),
                ],
                ink,
                line,
            ));
        }
        Tool::Texture => {
            // A sponge dabbing splotches in the ink color.
            let s = r * 0.5;
            for (x, y, k) in [
                (-0.75, 0.7, 0.28),
                (-0.2, 0.95, 0.18),
                (-0.95, 0.15, 0.16),
                (0.35, 0.85, 0.12),
            ] {
                p.circle_filled(c + vec2(x, y) * s, k * s * 1.4, ink);
            }
            let body = Rect::from_center_size(c + vec2(0.25, -0.25) * s, vec2(1.3, 0.95) * s);
            p.rect_filled(body, s * 0.25, Color32::from_rgb(250, 225, 120));
            p.rect_stroke(body, s * 0.25, line, egui::StrokeKind::Middle);
            for (x, y) in [(-0.05, -0.45), (0.45, -0.2), (0.15, 0.0), (0.6, -0.5)] {
                p.circle_stroke(c + vec2(x, y) * s, s * 0.08, line);
            }
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
        Tool::Bucket => {
            // A tipped paint bucket with a drip.
            let s = r * 0.5;
            let rot = |v: Vec2| {
                let (sn, cs) = (-0.5f32).sin_cos();
                c + vec2(v.x * cs - v.y * sn, v.x * sn + v.y * cs) * s
            };
            let body = [
                vec2(-0.6, -0.5),
                vec2(0.6, -0.5),
                vec2(0.45, 0.75),
                vec2(-0.45, 0.75),
            ];
            p.add(Shape::convex_polygon(
                body.iter().map(|&v| rot(v)).collect(),
                FACE,
                Stroke::NONE,
            ));
            p.add(Shape::closed_line(
                body.iter().map(|&v| rot(v)).collect(),
                line,
            ));
            p.add(Shape::line(
                vec![
                    rot(vec2(-0.55, -0.5)),
                    rot(vec2(0.0, -1.05)),
                    rot(vec2(0.55, -0.5)),
                ],
                line,
            ));
            p.circle_filled(c + vec2(0.85, 0.55) * s, s * 0.22, ink);
            p.line_segment([c + vec2(0.62, -0.05) * s, c + vec2(0.85, 0.4) * s], line);
        }
        Tool::Lasso => {
            // A dashed loop with a tail.
            let s = r * 0.5;
            let o = c + vec2(0.05 * s, -0.2 * s);
            let n = 14;
            for i in (0..n).step_by(2) {
                let a0 = i as f32 / n as f32 * TAU;
                let a1 = (i + 1) as f32 / n as f32 * TAU;
                let q = |a: f32| o + vec2(a.cos() * s * 0.85, a.sin() * s * 0.55);
                p.line_segment([q(a0), q(a1)], line);
            }
            let t0 = o + vec2(-0.55 * s, 0.42 * s);
            p.add(Shape::line(
                vec![
                    t0,
                    t0 + vec2(-0.1 * s, 0.4 * s),
                    t0 + vec2(-0.45 * s, 0.6 * s),
                ],
                line,
            ));
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

/// Desktop text search: a box at the top, results below; Enter flies to the
/// first result, clicking flies to that one, Esc closes.
fn search_panel(ctx: &egui::Context, st: &mut UiState) {
    let screen = ctx.content_rect();
    let w = 360.0f32.min(screen.width() - 24.0);
    egui::Area::new(Id::new("search"))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_TOP)
        .fixed_pos(pos2(screen.center().x, screen.top() + 14.0))
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
                    let mut close = false;
                    ui.horizontal(|ui| {
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut st.search_query)
                                .hint_text("Search text on the canvas")
                                .desired_width(w - 70.0),
                        );
                        if std::mem::take(&mut st.search_focus) {
                            r.request_focus();
                        }
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            st.search_pick = st.search_hits.first().map(|h| h.group);
                            r.request_focus();
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            close = true;
                        }
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    });
                    if !st.search_query.trim().is_empty() {
                        ui.add_space(4.0);
                        if st.search_hits.is_empty() {
                            ui.label(egui::RichText::new("No text matches").weak());
                        } else {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} found · click to go there",
                                    st.search_hits.len()
                                ))
                                .small()
                                .weak(),
                            );
                            egui::ScrollArea::vertical()
                                .max_height(240.0)
                                .show(ui, |ui| {
                                    for h in &st.search_hits {
                                        let label =
                                            format!("{}   (zoom 10^{:.1})", h.snippet, h.zoom);
                                        if ui
                                            .add_sized(
                                                [w, 0.0],
                                                egui::Button::selectable(false, label).wrap(),
                                            )
                                            .clicked()
                                        {
                                            st.search_pick = Some(h.group);
                                        }
                                    }
                                });
                        }
                    }
                    if close {
                        st.search_open = false;
                    }
                });
        });
}

/// Desktop library: thumbnails of saved stickers; click one to place a copy
/// in the middle of the screen.
fn library_panel(ctx: &egui::Context, st: &mut UiState, actions: &mut Vec<Action>) {
    let screen = ctx.content_rect();
    let w = 340.0f32.min(screen.width() - 24.0);
    egui::Area::new(Id::new("library"))
        .order(Order::Foreground)
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(pos2(screen.right() - 12.0, screen.top() + 96.0))
        .show(ctx, |ui| {
            ui.style_mut().visuals = egui::Visuals::light();
            egui::Frame::new()
                .fill(FACE)
                .stroke(Stroke::new(1.0, EDGE))
                .corner_radius(12.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.set_width(w);
                    ui.horizontal(|ui| {
                        ui.strong("Library");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Close").clicked() {
                                st.lib_open = false;
                            }
                        });
                    });
                    if st.lib.is_empty() {
                        ui.label(
                            egui::RichText::new(
                                "Empty. Select something and press Add to library in the selection panel.",
                            )
                            .weak(),
                        );
                        return;
                    }
                    ui.label(egui::RichText::new("Click to place a copy").small().weak());
                    let cell = 96.0;
                    egui::ScrollArea::vertical().max_height(screen.height() * 0.6).show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for (i, e) in st.lib.iter_mut().enumerate() {
                                if e.tex.is_none() {
                                    if let Some((side, px)) = e.thumb.take() {
                                        let img = egui::ColorImage::from_rgba_unmultiplied([side, side], &px);
                                        e.tex = Some(ctx.load_texture(format!("lib{i}"), img, Default::default()));
                                    }
                                }
                                ui.vertical(|ui| {
                                    ui.set_width(cell);
                                    let resp = match &e.tex {
                                        Some(t) => ui.add(
                                            egui::Button::image(egui::Image::new(t).fit_to_exact_size(vec2(cell - 8.0, cell - 8.0)))
                                                .fill(Color32::WHITE),
                                        ),
                                        None => ui.add_sized([cell, cell], egui::Button::new("…")),
                                    };
                                    if resp.on_hover_text(&e.name).clicked() {
                                        actions.push(Action::LibPlace(i));
                                    }
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(&e.name).small());
                                        if ui.small_button("×").on_hover_text("Remove from the library").clicked() {
                                            actions.push(Action::LibDelete(i));
                                        }
                                    });
                                });
                            }
                        });
                    });
                });
        });
}

/// Edit layout: the controls as handles to drag anywhere; fans then open
/// toward the middle of the screen from wherever their button is. The tool
/// panel goes to the corner it is dropped nearest.
fn layout_editor(ctx: &egui::Context, st: &mut UiState) {
    let screen = ctx.content_rect();
    let touch = st.touch_ui;
    let m = if touch { 18.0 } else { 16.0 };
    let g = geo(ctx, st);
    let accent = Color32::from_rgb(200, 40, 90);
    let tool = st.tool;
    let col = tool_color(st, tool);
    let mut lay = st.layout;
    // Everything here is UI: the canvas gets no input while editing.
    egui::Area::new(Id::new("layout_edit"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            ui.set_clip_rect(screen);
            let (all, _) = ui.allocate_exact_size(screen.size(), Sense::hover());
            let p = ui.painter().clone();
            p.rect_filled(
                all,
                0.0,
                Color32::from_rgba_unmultiplied(245, 242, 235, 140),
            );
            // Thirds: corners give quarter fans, edges half, the middle a ring.
            let faint = Stroke::new(1.0, Color32::from_rgba_unmultiplied(120, 120, 140, 70));
            for k in [1.0 / 3.0, 2.0 / 3.0] {
                let x = screen.left() + screen.width() * k;
                let y = screen.top() + screen.height() * k;
                p.line_segment([pos2(x, screen.top()), pos2(x, screen.bottom())], faint);
                p.line_segment([pos2(screen.left(), y), pos2(screen.right(), y)], faint);
            }
            // A draggable handle: its new centre (snapped to the edges)
            // while dragged.
            let handle =
                |id: &str, rect: Rect, paint: &dyn Fn(&egui::Painter, Rect)| -> Option<Pos2> {
                    let resp = ui.interact(rect, Id::new(("lay", id)), Sense::drag());
                    let mut r = rect;
                    if resp.dragged() {
                        r = r.translate(resp.drag_delta());
                    }
                    let lit = resp.hovered() || resp.dragged();
                    p.rect_filled(
                        r.expand(6.0),
                        10.0,
                        Color32::from_rgba_unmultiplied(255, 255, 255, 170),
                    );
                    p.rect_stroke(
                        r.expand(6.0),
                        10.0,
                        Stroke::new(if lit { 2.0 } else { 1.2 }, accent),
                        egui::StrokeKind::Middle,
                    );
                    paint(&p, r);
                    (resp.dragged() || resp.drag_stopped()).then(|| {
                        let mut c = r.center();
                        let h = r.size() * 0.5;
                        let snap = 28.0;
                        if c.x - h.x - screen.left() < snap {
                            c.x = screen.left() + m + h.x;
                        }
                        if screen.right() - c.x - h.x < snap {
                            c.x = screen.right() - m - h.x;
                        }
                        if c.y - h.y - screen.top() < snap {
                            c.y = screen.top() + m + h.y;
                        }
                        if screen.bottom() - c.y - h.y < snap {
                            c.y = screen.bottom() - m - h.y;
                        }
                        c
                    })
                };
            let tool_r = Rect::from_center_size(g.tool, Vec2::splat(2.0 * g.r));
            if let Some(c) = handle("tool", tool_r, &|p, r| {
                disc(p, r.center(), g.r, FACE, false);
                tool_icon(p, r.center(), g.r, tool, col);
            }) {
                lay.tool = Some(frac_of(screen, c));
            }
            let app_r = Rect::from_center_size(g.app, Vec2::splat(2.0 * g.r));
            if let Some(c) = handle("app", app_r, &|p, r| {
                disc(p, r.center(), g.r, FACE, false);
                gear_icon(p, r.center(), g.r);
            }) {
                lay.app = Some(frac_of(screen, c));
            }
            // The quick bar, as a strip of empty slots.
            let s = (if touch { 44.0 } else { 38.0 } * ui_scale(screen)).max(26.0);
            let w = ((hotbar::BAR + 2) as f32 * (s + 4.0) - 4.0).min(screen.width() - 2.0 * m);
            // A column or a row, as quick_bar draws it: by the screen, or
            // as set with the rotate button.
            let vertical = !st.radial_bar && lay.bar_is_vertical(screen.height() > screen.width());
            let bar_rect = if vertical {
                let pr = if touch { 24.0 } else { 19.0 } * ui_scale(screen).max(0.7);
                let s = 2.0 * pr;
                let room = screen.height() - 2.0 * (m + 2.0 * pr + 4.0 + 12.0);
                let n = (((room + 4.0) / (s + 4.0)) as usize).clamp(5, hotbar::BAR + 2);
                let len = n as f32 * (s + 4.0) - 4.0;
                let c = match lay.bar {
                    Some(f) => {
                        let c = at_frac(screen, f, 0.0);
                        pos2(
                            c.x.clamp(screen.left() + m + s * 0.5, screen.right() - m - s * 0.5),
                            c.y.clamp(
                                screen.top() + m + len * 0.5,
                                (screen.bottom() - m - len * 0.5).max(screen.top() + m + len * 0.5),
                            ),
                        )
                    }
                    None => pos2(screen.left() + m + 2.0 + pr, screen.center().y),
                };
                Rect::from_center_size(c, vec2(s, len))
            } else {
                let bar_c = match lay.bar {
                    Some(f) => at_frac(screen, f, 0.0),
                    None if lay.tool.is_some() => {
                        pos2(screen.center().x, screen.bottom() - m - s * 0.5)
                    }
                    None => pos2(screen.center().x, g.tool.y),
                };
                Rect::from_center_size(bar_c, vec2(w, s))
            };
            if let Some(c) = handle("bar", bar_rect, &|p, r| {
                let s = r.width().min(r.height());
                let along = if r.width() >= r.height() {
                    vec2(1.0, 0.0)
                } else {
                    vec2(0.0, 1.0)
                };
                let n = ((r.width().max(r.height()) + 4.0) / (s + 4.0)) as usize;
                for i in 0..n {
                    let b = Rect::from_min_size(
                        r.left_top() + along * (i as f32 * (s + 4.0)),
                        Vec2::splat(s),
                    );
                    p.rect_filled(b, 8.0, FACE);
                    p.rect_stroke(b, 8.0, Stroke::new(1.0, EDGE), egui::StrokeKind::Middle);
                }
            }) {
                lay.bar = Some(frac_of(screen, c));
            }
            // Rotate: turns the quick bar between a column and a row.
            if !st.radial_bar {
                let rr = if touch { 15.0 } else { 12.0 };
                let rc = if vertical {
                    // Above the column, or below it when at the top.
                    if bar_rect.top() - 10.0 - 2.0 * rr > screen.top() + 4.0 {
                        pos2(bar_rect.center().x, bar_rect.top() - 10.0 - rr)
                    } else {
                        pos2(bar_rect.center().x, bar_rect.bottom() + 10.0 + rr)
                    }
                } else if bar_rect.right() + 10.0 + 2.0 * rr < screen.right() - 4.0 {
                    pos2(bar_rect.right() + 10.0 + rr, bar_rect.center().y)
                } else if bar_rect.left() - 10.0 - 2.0 * rr > screen.left() + 4.0 {
                    pos2(bar_rect.left() - 10.0 - rr, bar_rect.center().y)
                } else if bar_rect.top() - 10.0 - 2.0 * rr > screen.top() + 4.0 {
                    // A row as wide as the screen: above its middle.
                    pos2(bar_rect.center().x, bar_rect.top() - 10.0 - rr)
                } else {
                    pos2(bar_rect.center().x, bar_rect.bottom() + 10.0 + rr)
                };
                let rect = Rect::from_center_size(rc, Vec2::splat(2.0 * rr));
                let resp = ui.interact(rect, Id::new("lay_rotate"), Sense::click());
                disc_shadow(&p, rc, rr);
                p.circle_filled(rc, rr, if resp.hovered() { Color32::WHITE } else { FACE });
                p.circle_stroke(rc, rr, Stroke::new(1.2, accent));
                rotate_icon(&p, rc, rr * 0.55, INKY);
                if resp.clicked() {
                    lay.bar_vertical = Some(!vertical);
                }
                resp.on_hover_text(if vertical {
                    "Turn the quick bar into a row"
                } else {
                    "Turn the quick bar into a column"
                });
            }
            // The tool panel, as a card in its corner.
            let k = ui_scale(screen);
            let (pw, ph) = (200.0 * k.max(0.7), 150.0 * k.max(0.7));
            let pc = lay.panel;
            let px = if pc.is_right() {
                screen.right() - m - pw
            } else {
                screen.left() + m
            };
            let py = if pc.is_top() {
                screen.top() + m + if touch { 70.0 } else { 60.0 }
            } else {
                screen.bottom() - m - ph
            };
            if let Some(c) = handle(
                "panel",
                Rect::from_min_size(pos2(px, py), vec2(pw, ph)),
                &|p, r| {
                    p.rect_filled(r, 12.0, FACE);
                    p.text(
                        r.left_top() + vec2(12.0, 12.0),
                        Align2::LEFT_TOP,
                        "Tool settings",
                        egui::FontId::proportional(14.0),
                        INKY,
                    );
                    p.text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        "goes to a corner",
                        egui::FontId::proportional(12.0),
                        Color32::from_gray(120),
                    );
                },
            ) {
                lay.panel = crate::layout::Corner::nearest(frac_of(screen, c));
            }
        });
    st.layout = lay;
    let mut done =
        ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter));
    egui::Area::new(Id::new("layout_bar"))
        .order(Order::Tooltip)
        .anchor(Align2::CENTER_TOP, vec2(0.0, 14.0))
        .show(ctx, |ui| {
            ui.style_mut().visuals = egui::Visuals::light();
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                // Narrow enough to leave the corner buttons uncovered; on a
                // phone the text wraps and the buttons go under it.
                let w = (screen.width() - 2.0 * 76.0).clamp(200.0, 560.0);
                ui.set_max_width(w);
                let text = "Drag the controls anywhere. Fans open toward the middle.";
                let one_line = ui.fonts_mut(|f| {
                    f.layout_no_wrap(text.into(), egui::FontId::proportional(14.0), INKY)
                        .size()
                        .x
                }) + 140.0
                    <= w;
                let buttons = |ui: &mut egui::Ui, st: &mut UiState, done: &mut bool| {
                    if ui.button("Reset").clicked() {
                        st.layout = Default::default();
                    }
                    if ui.button("Done").clicked() {
                        *done = true;
                    }
                };
                if one_line {
                    ui.horizontal(|ui| {
                        ui.label(text);
                        buttons(ui, st, &mut done);
                    });
                } else {
                    ui.add(egui::Label::new(text).wrap());
                    ui.horizontal(|ui| buttons(ui, st, &mut done));
                }
            });
        });
    if done {
        st.layout_edit = false;
        let mut p = crate::prefs::load();
        p.insert("layout".into(), st.layout.encode());
        crate::prefs::save(&p);
    }
}

/// While another canvas is being placed: what to do, Place and Cancel.
fn import_bar(ctx: &egui::Context, actions: &mut Vec<Action>) {
    let screen = ctx.content_rect();
    egui::Area::new(Id::new("import_bar"))
        .order(Order::Tooltip)
        .anchor(Align2::CENTER_TOP, vec2(0.0, 14.0))
        .show(ctx, |ui| {
            ui.style_mut().visuals = egui::Visuals::light();
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width((screen.width() - 2.0 * 76.0).clamp(200.0, 520.0));
                ui.add(
                    egui::Label::new(
                        "Importing a canvas: drag to move it (pinch or scroll to zoom).",
                    )
                    .wrap(),
                );
                ui.horizontal(|ui| {
                    if ui
                        .button(egui::RichText::new("Place").strong())
                        .on_hover_text("Put it here (Enter); undo removes it")
                        .clicked()
                    {
                        actions.push(Action::ImportPlace);
                    }
                    if ui
                        .button("Cancel")
                        .on_hover_text("Leave it out (Esc)")
                        .clicked()
                    {
                        actions.push(Action::ImportCancel);
                    }
                });
            });
        });
}

/// A preview of a brush: its dabs along a wave, drawn with egui shapes.
fn brush_preview(ui: &mut egui::Ui, ink: &InkSettings, h: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 8.0, Color32::from_rgb(250, 249, 246));
    let w = ink.width.clamp(2.0, h * 0.6);
    let len = rect.width() - 24.0 - w;
    let n = 40;
    let pts: Vec<ogpaper_core::Point> = (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let x = t * len;
            let y = (t * 6.0).sin() * (h * 0.22 - w * 0.25).max(0.0);
            // Pressure rises then falls, to show the dynamics.
            let pr = (t * PI).sin();
            [x, y, pr, 0.0]
        })
        .collect();
    let mut along = 0.0;
    let pts: Vec<ogpaper_core::Point> = pts
        .iter()
        .enumerate()
        .map(|(i, q)| {
            if i > 0 {
                along += (q[0] - pts[i - 1][0]).hypot(q[1] - pts[i - 1][1]);
            }
            [q[0], q[1], q[2], along]
        })
        .collect();
    let [r, g, b, a] = ink.rgba();
    let col = u32::from_le_bytes([r, g, b, a]);
    let o = pos2(rect.left() + 12.0 + w * 0.5, rect.center().y);
    let dabs = ogpaper_core::brush::dabs(&pts, w, col, &ink.params);
    for d in dabs.iter().take(4000) {
        let [r, g, b, a] = d.color.to_le_bytes();
        let c = Color32::from_rgba_unmultiplied(r, g, b, a);
        let at = o + vec2(d.x, d.y);
        let (sn, cs) = d.angle.sin_cos();
        let aspect = ink.params.aspect;
        let pts: Vec<Pos2> = ogpaper_core::brush::tip_outline(ink.params.tip, d.rand)
            .into_iter()
            .map(|v| {
                let v = vec2(v[0], v[1] * aspect) * d.size;
                at + vec2(v.x * cs - v.y * sn, v.x * sn + v.y * cs)
            })
            .collect();
        if matches!(ink.params.tip, ogpaper_core::Tip::Ring) {
            let mut pts = pts;
            pts.push(pts[0]);
            p.add(Shape::line(pts, Stroke::new((d.size * 0.12).max(1.0), c)));
        } else {
            p.add(Shape::convex_polygon(pts, c, Stroke::NONE));
        }
    }
}

/// The brush (and texture) panel: Simple is the plain line of old; Advanced
/// opens the brush engine with its looks.
fn brush_section(
    ui: &mut egui::Ui,
    ink: &mut InkSettings,
    touch: bool,
    dial_hue: &mut f32,
    texture: bool,
) -> bool {
    if !texture {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(!ink.advanced, "Simple")
                .on_hover_text("A plain line: width, pressure, dashes")
                .clicked()
            {
                ink.advanced = false;
            }
            if ui
                .selectable_label(ink.advanced, "Advanced")
                .on_hover_text("The brush engine: looks, tips, dynamics, scatter, color")
                .clicked()
            {
                ink.advanced = true;
            }
        });
        if !ink.advanced {
            return ink_section(ui, ink, Tool::Pen, touch, dial_hue, true);
        }
    }
    brush_preview(ui, ink, 56.0);
    heading(ui, if texture { "Texture" } else { "Look" });
    let looks = ogpaper_core::brush::looks();
    ui.horizontal_wrapped(|ui| {
        for l in looks.iter().filter(|l| l.texture == texture) {
            if ui
                .selectable_label(ink.params.look == l.params.look, l.name)
                .clicked()
            {
                let seed = ink.params.seed;
                ink.params = l.params;
                ink.params.seed = seed;
            }
        }
    });
    heading(ui, "Size");
    ui.add(
        egui::Slider::new(&mut ink.width, 0.5..=200.0)
            .logarithmic(true)
            .suffix(" px"),
    );
    opacity_slider(ui, &mut ink.opacity);
    let p = &mut ink.params;
    egui::CollapsingHeader::new("Tip").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for t in ogpaper_core::Tip::ALL {
                if ui.selectable_label(p.tip == t, t.name()).clicked() {
                    p.tip = t;
                }
            }
        });
        if p.tip == ogpaper_core::Tip::Pattern {
            ui.horizontal_wrapped(|ui| {
                for pt in ogpaper_core::Pattern::ALL {
                    if ui.selectable_label(p.pattern == pt, pt.name()).clicked() {
                        p.pattern = pt;
                    }
                }
            });
            ui.add(
                egui::Slider::new(&mut p.pattern_scale, 0.05..=4.0)
                    .logarithmic(true)
                    .text("pattern size"),
            );
        }
        ui.add(egui::Slider::new(&mut p.hardness, 0.0..=1.0).text("hardness"));
        let mut sp = p.spacing * 100.0;
        if ui
            .add(
                egui::Slider::new(&mut sp, 1.0..=300.0)
                    .logarithmic(true)
                    .suffix(" %")
                    .text("spacing"),
            )
            .changed()
        {
            p.spacing = sp / 100.0;
        }
        let mut deg = p.angle.to_degrees();
        if ui
            .add(
                egui::Slider::new(&mut deg, -180.0..=180.0)
                    .suffix("°")
                    .text("angle"),
            )
            .changed()
        {
            p.angle = deg.to_radians();
        }
        ui.add(egui::Slider::new(&mut p.aspect, 0.05..=1.0).text("roundness"));
        ui.checkbox(&mut p.follow, "Turn with the stroke");
    });
    egui::CollapsingHeader::new("Dynamics").show(ui, |ui| {
        ui.add(egui::Slider::new(&mut p.p_size, 0.0..=1.0).text("pressure → size"));
        ui.add(egui::Slider::new(&mut p.p_opacity, 0.0..=1.0).text("pressure → opacity"));
        ui.add(egui::Slider::new(&mut p.s_size, -1.0..=1.0).text("speed → thinner"));
        ui.add(egui::Slider::new(&mut p.s_opacity, -1.0..=1.0).text("speed → fainter"));
        ui.add(egui::Slider::new(&mut p.taper_in, 0.0..=20.0).text("taper start"));
        ui.add(egui::Slider::new(&mut p.taper_out, 0.0..=20.0).text("taper end"));
        ui.add(
            egui::Slider::new(&mut p.fade, 0.0..=200.0)
                .logarithmic(true)
                .text("fade out"),
        );
    });
    egui::CollapsingHeader::new("Scatter").show(ui, |ui| {
        ui.add(egui::Slider::new(&mut p.jitter, 0.0..=4.0).text("scatter"));
        let mut c = p.count as f32;
        if ui
            .add(
                egui::Slider::new(&mut c, 1.0..=32.0)
                    .step_by(1.0)
                    .text("per step"),
            )
            .changed()
        {
            p.count = c as u8;
        }
        ui.add(egui::Slider::new(&mut p.size_jitter, 0.0..=1.0).text("size jitter"));
        ui.add(egui::Slider::new(&mut p.angle_jitter, 0.0..=1.0).text("angle jitter"));
        ui.add(egui::Slider::new(&mut p.wobble, 0.0..=4.0).text("sketchy wobble"));
        let mut ps = p.passes as f32;
        if ui
            .add(
                egui::Slider::new(&mut ps, 1.0..=6.0)
                    .step_by(1.0)
                    .text("passes"),
            )
            .changed()
        {
            p.passes = ps as u8;
        }
    });
    egui::CollapsingHeader::new("Paint").show(ui, |ui| {
        ui.add(
            egui::Slider::new(&mut p.flow, 0.02..=1.0)
                .logarithmic(true)
                .text("flow (build-up)"),
        );
        ui.add(egui::Slider::new(&mut p.grain, 0.0..=1.0).text("paper grain"));
        ui.add(egui::Slider::new(&mut p.hue_jitter, 0.0..=0.5).text("color jitter"));
        let mut grad = p.color2 != 0;
        ui.horizontal(|ui| {
            if ui.checkbox(&mut grad, "Fade to").changed() {
                p.color2 = if grad { 0xffff_8020 } else { 0 };
            }
            if grad {
                let [r, g, b, a] = p.color2.to_le_bytes();
                let mut c = Color32::from_rgba_unmultiplied(r, g, b, a);
                if ui.color_edit_button_srgba(&mut c).changed() {
                    let [r, g, b, _] = c.to_array();
                    p.color2 = u32::from_le_bytes([r, g, b, 255]).max(1);
                }
            }
        });
        if ui
            .button("New random seed")
            .on_hover_text("Different scatter, same settings")
            .clicked()
        {
            p.seed = p.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        }
    });
    heading(ui, "Color");
    color_dial(ui, &mut ink.color, false, dial_hue, touch)
}

/// Settings > Hotkeys: every tool, look and command with its key; tap a key
/// to change it (then press the new one), × to clear it.
/// Layout: what shows on screen, the background, and Edit layout.
fn layout_panel(ctx: &egui::Context, st: &mut UiState, actions: &mut Vec<Action>) {
    let screen = ctx.content_rect();
    let w = 340.0f32.min(screen.width() - 24.0);
    egui::Area::new(Id::new("layout_menu"))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_CENTER)
        .fixed_pos(screen.center())
        .show(ctx, |ui| {
            ui.style_mut().visuals = egui::Visuals::light();
            egui::Frame::new()
                .fill(FACE)
                .stroke(Stroke::new(1.0, EDGE))
                .corner_radius(12.0)
                .inner_margin(12.0)
                .shadow(egui::Shadow {
                    offset: [0, 3],
                    blur: 14,
                    spread: 0,
                    color: Color32::from_black_alpha(45),
                })
                .show(ui, |ui| {
                    ui.set_width(w);
                    ui.horizontal(|ui| {
                        ui.strong("Layout");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("×").on_hover_text("Close").clicked() {
                                st.layout_open = false;
                            }
                        });
                    });
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Show").strong());
                    // Each tick changes it at once (and is remembered).
                    let mut tick =
                        |ui: &mut egui::Ui, on: bool, text: &str, hint: &str, a: Action| {
                            let mut v = on;
                            if ui.checkbox(&mut v, text).on_hover_text(hint).changed() {
                                actions.push(a);
                            }
                        };
                    tick(
                        ui,
                        !st.hide_tools,
                        "Tool button",
                        "The round button that opens the tools",
                        Action::ShowTools,
                    );
                    tick(
                        ui,
                        !st.hide_panel,
                        "Tool panel",
                        "The current tool's settings",
                        Action::ShowPanel,
                    );
                    tick(
                        ui,
                        !st.hide_bar,
                        "Quick toolbar",
                        "Your saved tools",
                        Action::ShowBar,
                    );
                    tick(
                        ui,
                        st.radial_bar,
                        "Quick toolbar as a radial menu",
                        "A round button in the corner that fans out, instead of a bar",
                        Action::RadialBar,
                    );
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Paper").strong());
                    ui.horizontal(|ui| {
                        for (code, label, mode) in [
                            (0u8, "Plain", GridMode::Off),
                            (1, "Grid lines", GridMode::Lines),
                            (2, "Dots", GridMode::Dots),
                        ] {
                            if ui.selectable_label(st.grid == mode, label).clicked()
                                && st.grid != mode
                            {
                                actions.push(Action::SetGrid(code));
                            }
                        }
                    });
                    ui.add_space(10.0);
                    if ui
                        .button("Edit layout: move the buttons…")
                        .on_hover_text("Drag the controls anywhere; fans open toward the middle")
                        .clicked()
                    {
                        st.layout_open = false;
                        actions.push(Action::EditLayout);
                    }
                });
        });
}

/// "5 min ago" and such.
fn ago(ms: u64) -> String {
    if ms == 0 {
        return String::new();
    }
    let now = crate::timeline::now_ms().max(0) as u64;
    let s = now.saturating_sub(ms) / 1000;
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86400 => format!("{} h ago", s / 3600),
        _ => format!("{} days ago", s / 86400),
    }
}

/// Pages: the pages on this device and on the servers added here.
fn pages_panel(ctx: &egui::Context, st: &mut UiState, actions: &mut Vec<Action>) {
    let screen = ctx.content_rect();
    let w = 420.0f32.min(screen.width() - 24.0);
    egui::Area::new(Id::new("pages"))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_CENTER)
        .fixed_pos(screen.center())
        .show(ctx, |ui| {
            ui.style_mut().visuals = egui::Visuals::light();
            egui::Frame::new()
                .fill(FACE)
                .stroke(Stroke::new(1.0, EDGE))
                .corner_radius(12.0)
                .inner_margin(12.0)
                .shadow(egui::Shadow { offset: [0, 3], blur: 14, spread: 0, color: Color32::from_black_alpha(45) })
                .show(ui, |ui| {
                    ui.set_width(w);
                    ui.horizontal(|ui| {
                        ui.strong("Pages");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("×").on_hover_text("Close").clicked() {
                                st.pages_open = false;
                            }
                        });
                    });
                    // Scroll only when the list is long (a scroll area sized
                    // on an earlier, shorter list cuts off its foot).
                    let rows = st.local_pages.len()
                        + st.servers.iter().map(|s| s.pages.len() + 2).sum::<usize>()
                        + 9;
                    let max_h = screen.height() * 0.72;
                    let tall = rows as f32 * 24.0 > max_h;
                    let mut body = |ui: &mut egui::Ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("On this device").strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("+ New page").clicked() {
                                    actions.push(Action::NewLocal);
                                }
                            });
                        });
                        if st.local_pages.is_empty() {
                            ui.label(egui::RichText::new("Nothing saved here yet").small().weak());
                        }
                        for (i, p) in st.local_pages.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let name = if p.current {
                                    egui::RichText::new(format!("{} (open)", p.name)).strong()
                                } else {
                                    egui::RichText::new(&p.name)
                                };
                                ui.label(name);
                                ui.label(egui::RichText::new(ago(p.changed)).small().weak());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if !p.current {
                                        if ui.small_button("Delete").clicked() {
                                            actions.push(Action::DeleteLocal(i));
                                        }
                                        if ui.small_button("Open").clicked() {
                                            actions.push(Action::OpenLocal(i));
                                        }
                                    }
                                });
                            });
                        }
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new("Servers").strong());
                        if st.servers.is_empty() {
                            ui.label(
                                egui::RichText::new(
                                    "Add a server (og-paper --serve-dir, e.g. on a NAS) by its server link to see and open its pages.",
                                )
                                .small()
                                .weak(),
                            );
                        }
                        for (i, s) in st.servers.iter().enumerate() {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(&s.name).strong());
                                ui.label(egui::RichText::new(&s.state).small().weak());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("Remove").on_hover_text("Forget this server here (its pages stay on it)").clicked() {
                                        actions.push(Action::RemoveServer(i));
                                    }
                                    if s.can_edit && ui.small_button("+ Page").clicked() {
                                        actions.push(Action::NewServerPage(i));
                                    }
                                    if ui.small_button("↻").on_hover_text("Check again").clicked() {
                                        actions.push(Action::RefreshServer(i));
                                    }
                                });
                            });
                            for (j, (name, changed, open)) in s.pages.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.add_space(12.0);
                                    if *open {
                                        ui.label(egui::RichText::new(format!("{name} (open)")).strong());
                                    } else {
                                        ui.label(name);
                                    }
                                    ui.label(egui::RichText::new(ago(*changed)).small().weak());
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button(if *open { "Connect" } else { "Open" }).clicked() {
                                            actions.push(Action::OpenServerPage(i, j));
                                        }
                                    });
                                });
                            }
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut st.add_server_text)
                                    .hint_text("Server link (ws://… ?k=…)")
                                    .desired_width(w - 80.0),
                            );
                            if ui.button("Add").clicked() && !st.add_server_text.trim().is_empty() {
                                actions.push(Action::AddServer);
                            }
                        });
                        ui.label(
                            egui::RichText::new("Opening a server page keeps a copy here; it reconnects whenever you open it, and what you did offline goes up.")
                                .small()
                                .weak(),
                        );
                    };
                    if tall {
                        egui::ScrollArea::vertical()
                            .id_salt("pages_scroll")
                            .max_height(max_h)
                            .show(ui, body);
                    } else {
                        body(ui);
                    }
                });
        });
}

/// Share live: host this canvas or join one, who is here, the links.
fn live_panel(ctx: &egui::Context, st: &mut UiState, actions: &mut Vec<Action>) {
    let screen = ctx.content_rect();
    let w = 400.0f32.min(screen.width() - 24.0);
    let web = cfg!(target_arch = "wasm32");
    egui::Area::new(Id::new("live"))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_CENTER)
        .fixed_pos(screen.center())
        .show(ctx, |ui| {
            ui.style_mut().visuals = egui::Visuals::light();
            egui::Frame::new()
                .fill(FACE)
                .stroke(Stroke::new(1.0, EDGE))
                .corner_radius(12.0)
                .inner_margin(12.0)
                .shadow(egui::Shadow { offset: [0, 3], blur: 14, spread: 0, color: Color32::from_black_alpha(45) })
                .show(ui, |ui| {
                    ui.set_width(w);
                    ui.horizontal(|ui| {
                        ui.strong("Share live");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("×").on_hover_text("Close").clicked() {
                                st.live_open = false;
                            }
                        });
                    });
                    // Your name.
                    let name = st.name_text.get_or_insert_with(crate::presence::my_name);
                    ui.horizontal(|ui| {
                        ui.label("Your name");
                        let r = ui.add(egui::TextEdit::singleline(name).desired_width(180.0));
                        if r.lost_focus() {
                            actions.push(Action::SetName);
                        }
                    });
                    ui.separator();
                    match st.live.clone() {
                        None => {
                            ui.label(
                                egui::RichText::new(
                                    "Draw together: host this canvas so others join with a link, or join someone's.",
                                )
                                .small(),
                            );
                            ui.add_space(4.0);
                            if web {
                                if ui
                                    .button("Host in this browser (no server)")
                                    .on_hover_text("Invite people with one-time links; keep this tab open")
                                    .clicked()
                                {
                                    actions.push(Action::RtcPanel);
                                }
                                ui.label(
                                    egui::RichText::new("A server host (desktop app or og-paper --serve) lets people come and go any time.")
                                        .small()
                                        .weak(),
                                );
                            } else if ui
                                .button("Host this canvas")
                                .on_hover_text("Others join with a link; everyone keeps a copy")
                                .clicked()
                            {
                                actions.push(Action::HostStart);
                            }
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new("Or through a relay (keeps changes for people who come and go; it cannot read them):")
                                    .small(),
                            );
                            let relay = st.relay_text.get_or_insert_with(|| {
                                crate::prefs::load()
                                    .get("relay")
                                    .cloned()
                                    .unwrap_or_else(|| "ws://localhost:8993".into())
                            });
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(relay).desired_width(w - 130.0));
                                if ui.button("Share via relay").clicked() {
                                    actions.push(Action::RelayShare);
                                }
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut st.join_text)
                                        .hint_text("Paste a link (ws://… or …#join=…)")
                                        .desired_width(w - 70.0),
                                );
                                if ui.button("Join").clicked() && !st.join_text.trim().is_empty() {
                                    actions.push(Action::Join);
                                }
                            });
                        }
                        Some(info) => {
                            ui.label(egui::RichText::new(&info.state).strong());
                            if info.view_only {
                                ui.label(egui::RichText::new("View link: you can look around and follow, not draw.").small().weak());
                            }
                            if info.people.is_empty() {
                                ui.label(egui::RichText::new("Nobody else yet").small().weak());
                            }
                            for (peer, who) in &info.people {
                                ui.horizontal(|ui| {
                                    ui.label(who);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button("Follow").on_hover_text("Ride along with their view").clicked() {
                                            actions.push(Action::GoToPeer(*peer, true));
                                        }
                                        if ui.small_button("Go to").clicked() {
                                            actions.push(Action::GoToPeer(*peer, false));
                                        }
                                    });
                                });
                            }
                            if !info.links.is_empty() {
                                ui.add_space(6.0);
                                for (what, link) in &info.links {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(what).small());
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.small_button("Copy").clicked() {
                                                ui.ctx().copy_text(link.clone());
                                            }
                                        });
                                    });
                                    ui.label(egui::RichText::new(link).small().monospace().weak());
                                }
                                ui.label(
                                    egui::RichText::new(
                                        "Browsers on https pages need a wss:// address (e.g. Tailscale serve in front of this port).",
                                    )
                                    .small()
                                    .weak(),
                                );
                            }
                            if info.hosting {
                                ui.add_space(6.0);
                                ui.label("When two people change the same thing apart:");
                                ui.horizontal(|ui| {
                                    for (code, label) in [(0u8, "Newest wins"), (1, "Host wins"), (2, "Guest wins")] {
                                        if ui.selectable_label(info.policy == code, label).clicked() {
                                            actions.push(Action::SetPolicy(code));
                                        }
                                    }
                                });
                            }
                            if web && info.hosting && ui.button("Invite someone…").clicked() {
                                actions.push(Action::RtcPanel);
                            }
                            ui.add_space(6.0);
                            let can_rotate = info.hosting || (!info.links.is_empty() && !info.view_only);
                            if can_rotate
                                && ui
                                    .button("New links")
                                    .on_hover_text("Links handed out so far stop working (people need the new ones)")
                                    .clicked()
                            {
                                actions.push(Action::NewLinks);
                            }
                            if ui
                                .button(if info.hosting { "Stop hosting" } else { "Leave" })
                                .clicked()
                            {
                                actions.push(Action::NetStop);
                            }
                        }
                    }
                });
        });
}

fn hotkeys_panel(ctx: &egui::Context, st: &mut UiState, actions: &mut Vec<Action>) {
    let screen = ctx.content_rect();
    let w = 380.0f32.min(screen.width() - 24.0);
    let mut changed = false;
    egui::Area::new(Id::new("hotkeys"))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_CENTER)
        .fixed_pos(screen.center())
        .show(ctx, |ui| {
            ui.style_mut().visuals = egui::Visuals::light();
            egui::Frame::new()
                .fill(FACE)
                .stroke(Stroke::new(1.0, EDGE))
                .corner_radius(12.0)
                .inner_margin(12.0)
                .shadow(egui::Shadow { offset: [0, 3], blur: 14, spread: 0, color: Color32::from_black_alpha(45) })
                .show(ui, |ui| {
                    ui.set_width(w);
                    ui.horizontal(|ui| {
                        ui.strong("Hotkeys");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Close").clicked() {
                                st.keys_open = false;
                                st.key_capture = None;
                            }
                            if ui.button("Reset all").on_hover_text("Back to the default keys").clicked() {
                                st.keys.reset();
                                changed = true;
                            }
                        });
                    });
                    ui.label(
                        egui::RichText::new("Tap a key to change it, then press the new key (Esc cancels, Backspace clears).")
                            .small()
                            .weak(),
                    );
                    ui.add_space(4.0);
                    let all = crate::hotkeys::bindings();
                    egui::ScrollArea::vertical().max_height(screen.height() * 0.7).show(ui, |ui| {
                        let mut group = "";
                        for b in &all {
                            if b.group != group {
                                group = b.group;
                                ui.add_space(6.0);
                                ui.label(egui::RichText::new(group).strong());
                            }
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    vec2(w - 130.0, 20.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.set_min_width(w - 130.0);
                                        ui.add(egui::Label::new(&b.label).truncate());
                                    },
                                );
                                let waiting = st.key_capture.as_deref() == Some(b.id.as_str());
                                let text = if waiting {
                                    "press a key…".to_string()
                                } else {
                                    st.keys.keys.get(&b.id).cloned().flatten().map_or("—".to_string(), |c| c.label())
                                };
                                if ui.add_sized([86.0, 20.0], egui::Button::selectable(waiting, text)).clicked() {
                                    st.key_capture = if waiting { None } else { Some(b.id.clone()) };
                                }
                                if ui.small_button("×").on_hover_text("No key").clicked() {
                                    st.keys.set(&b.id, None);
                                    changed = true;
                                }
                            });
                        }
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("Fixed keys").strong());
                        ui.label(
                            egui::RichText::new(
                                "1–9 quick bar slots · Alt+1–9 toolbars · [ ] cycle toolbars · Ctrl+C / X / V copy, cut, paste · \
                                 Ctrl+A select all · Ctrl+D duplicate · Ctrl+Y redo · Delete · arrows nudge · Esc · Enter · Home · Space pan",
                            )
                            .small()
                            .weak(),
                        );
                    });
                });
        });
    if changed {
        actions.push(Action::SaveKeys);
    }
}

/// A saved view in a slot: a bookmark ribbon with the name's first letter.
fn view_icon(p: &egui::Painter, c: Pos2, r: f32, name: &str) {
    let s = r * 0.55;
    let pts = vec![
        c + vec2(-0.6, -1.0) * s,
        c + vec2(0.6, -1.0) * s,
        c + vec2(0.6, 1.0) * s,
        c + vec2(0.0, 0.55) * s,
        c + vec2(-0.6, 1.0) * s,
    ];
    p.add(Shape::convex_polygon(
        pts.clone(),
        Color32::from_rgb(255, 236, 200),
        Stroke::NONE,
    ));
    let mut closed = pts;
    closed.push(closed[0]);
    p.add(Shape::line(closed, Stroke::new(1.3, ACCENT)));
    let letter: String = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    p.text(
        c + vec2(0.0, -0.2 * s),
        Align2::CENTER_CENTER,
        letter,
        egui::FontId::proportional(r * 0.55),
        INKY,
    );
}

/// The radial toolbar: a button bottom right whose fan holds the active
/// toolbar's slots, then the toolbar list and the bag.
fn radial_bar(ctx: &egui::Context, st: &mut UiState, g: &Geo) {
    let open = ctx.animate_bool_with_time(Id::new("bar_open"), st.menu == Menu::Bar, 0.12);
    let n = st.hotbar.len();
    // Toolbars and the bag sit on a small inner ring; the rings beyond hold
    // only tools.
    let slots = ring_slots(n, g.r, g.bar_arc.1);
    // Other toolbars ticked to show: a ring each, further out.
    let others: Vec<usize> = (0..st.toolbars.len())
        .filter(|&k| k != st.active_bar && st.toolbars[k].shown)
        .collect();
    let inner = slots.iter().map(|s| s.0).fold(0.0, f32::max);
    let step = 2.0 * g.r * 0.9 + 16.0;
    let full = g.bar_arc.1 >= TAU - 1e-3;
    let mut outer: Vec<(usize, usize, f32, f32)> = Vec::new(); // (bar, slot, radius, frac)
    for (j, &k) in others.iter().enumerate() {
        let m = st.toolbars[k].slots.len();
        let radius = inner + (j + 1) as f32 * step;
        for i in 0..m {
            let frac = if full {
                i as f32 / m as f32
            } else if m > 1 {
                i as f32 / (m - 1) as f32
            } else {
                0.5
            };
            outer.push((k, i, radius, frac));
        }
    }
    let mut bbox = Rect::from_center_size(g.bar, Vec2::splat(2.0 * g.r));
    if open > 0.0 {
        let far = outer
            .iter()
            .map(|o| o.2)
            .fold(fan_reach(&slots, g.r), |a, b| a.max(b + g.r));
        let reach = far * open + g.r * 1.2;
        bbox = bbox.union(Rect::from_center_size(
            g.bar,
            Vec2::splat(2.0 * (reach + 30.0)),
        ));
    }
    let screen = ctx.content_rect();
    let current = st.preset();
    let mut changed = false;
    egui::Area::new(Id::new("radial_bar"))
        .order(Order::Foreground)
        .fixed_pos(bbox.min)
        .show(ctx, |ui| {
            ui.set_clip_rect(screen);
            ui.allocate_exact_size(bbox.size(), Sense::hover());
            let p = ui.painter().clone();
            if open > 0.0 {
                // The slots, then Toolbars (which opens the inventory too)
                // on the inner ring.
                for i in 0..n + 1 {
                    let (radius, frac) = if i < n {
                        slots[i]
                    } else {
                        (g.r * 2.4, if full { 0.0 } else { 0.5 })
                    };
                    let a = g.bar_arc.0 + g.bar_arc.1 * frac;
                    let pc = g.bar + Vec2::angled(a) * radius * open;
                    let rr = g.r * 0.9 * open.max(0.3);
                    let resp = ui.interact(
                        Rect::from_center_size(pc, Vec2::splat(2.0 * rr)),
                        Id::new(("rbar", i)),
                        Sense::click(),
                    );
                    let hover = resp.hovered();
                    let label_text: String;
                    if i < n {
                        let item = st.hotbar[i];
                        let on = item.is_some_and(|it| it == current);
                        disc(&p, pc, rr, if hover { Color32::WHITE } else { FACE }, on);
                        match item {
                            Some(it) if it.cmd.is_some() => {
                                let redo = it.cmd == Some(hotbar::SlotCmd::Redo);
                                undo_icon(&p, pc, rr * 0.8, redo, INKY);
                            }
                            Some(it) if it.view.is_some() => {
                                let name = it
                                    .view
                                    .and_then(|v| st.views.get(&v))
                                    .map_or("View".into(), |v| v.name.clone());
                                view_icon(&p, pc, rr, &name);
                            }
                            Some(it) => preset_icon(&p, pc, rr, &it, &st.egui_fonts),
                            None => {
                                p.text(
                                    pc,
                                    Align2::CENTER_CENTER,
                                    "+",
                                    egui::FontId::proportional(rr * 0.7),
                                    Color32::from_gray(170),
                                );
                            }
                        }
                        p.text(
                            pc + vec2(-rr * 0.62, -rr * 0.62),
                            Align2::CENTER_CENTER,
                            format!("{}", i + 1),
                            egui::FontId::proportional(9.0),
                            Color32::from_gray(140),
                        );
                        label_text = match item {
                            Some(it) if it.view.is_some() => "View".into(),
                            Some(it) => describe(&it),
                            None => "Empty: save the current tool".into(),
                        };
                        if resp.clicked() {
                            match item {
                                Some(it) => {
                                    st.apply(&it);
                                    if it.cmd.is_none() {
                                        st.menu = Menu::None;
                                    }
                                }
                                None => {
                                    st.hotbar[i] = Some(current);
                                    changed = true;
                                }
                            }
                        }
                    } else if i == n {
                        disc(
                            &p,
                            pc,
                            rr,
                            if hover { Color32::WHITE } else { FACE },
                            st.bar_menu,
                        );
                        toolbars_icon(&p, pc, rr * 0.9, st.active_bar + 1);
                        label_text = "Toolbars and inventory".into();
                        if resp.clicked() {
                            st.bar_menu = !st.bar_menu;
                            st.bag_open = st.bar_menu;
                        }
                    } else {
                        disc(
                            &p,
                            pc,
                            rr,
                            if hover { Color32::WHITE } else { FACE },
                            st.bag_open,
                        );
                        bag_icon(&p, pc, rr * 0.9);
                        label_text = "Inventory".into();
                        if resp.clicked() {
                            st.bag_open = !st.bag_open;
                            st.bar_menu = false;
                        }
                    }
                    if open > 0.9 && hover {
                        label(
                            &p,
                            pc + Vec2::angled(a) * (rr + 16.0) + vec2(0.0, 2.0),
                            &label_text,
                        );
                    }
                }
                // The other shown toolbars' slots, a ring each.
                for &(k, i, radius, frac) in &outer {
                    let a = g.bar_arc.0 + g.bar_arc.1 * frac;
                    let pc = g.bar + Vec2::angled(a) * radius * open;
                    let rr = g.r * 0.82 * open.max(0.3);
                    let resp = ui.interact(
                        Rect::from_center_size(pc, Vec2::splat(2.0 * rr)),
                        Id::new(("rbar_o", k, i)),
                        Sense::click(),
                    );
                    let hover = resp.hovered();
                    let item = st.toolbars[k].slots[i];
                    disc(
                        &p,
                        pc,
                        rr,
                        if hover { Color32::WHITE } else { FACE },
                        item.is_some_and(|it| it == current),
                    );
                    match item {
                        Some(it) if it.cmd.is_some() => undo_icon(
                            &p,
                            pc,
                            rr * 0.8,
                            it.cmd == Some(hotbar::SlotCmd::Redo),
                            INKY,
                        ),
                        Some(it) if it.view.is_some() => {
                            let name = it
                                .view
                                .and_then(|v| st.views.get(&v))
                                .map_or("View".into(), |v| v.name.clone());
                            view_icon(&p, pc, rr, &name);
                        }
                        Some(it) => preset_icon(&p, pc, rr, &it, &st.egui_fonts),
                        None => {
                            p.text(
                                pc,
                                Align2::CENTER_CENTER,
                                "+",
                                egui::FontId::proportional(rr * 0.7),
                                Color32::from_gray(170),
                            );
                        }
                    }
                    // Which toolbar and slot, top left.
                    p.text(
                        pc + vec2(-rr * 0.62, -rr * 0.62),
                        Align2::CENTER_CENTER,
                        format!("{}·{}", k + 1, i + 1),
                        egui::FontId::proportional(8.5),
                        Color32::from_gray(140),
                    );
                    if open > 0.9 && hover {
                        let t = match item {
                            Some(it) if it.view.is_some() => "View".to_string(),
                            Some(it) => describe(&it),
                            None => "Empty: save the current tool".into(),
                        };
                        label(
                            &p,
                            pc + Vec2::angled(a) * (rr + 16.0) + vec2(0.0, 2.0),
                            &format!("Toolbar {}: {t}", k + 1),
                        );
                    }
                    if resp.clicked() {
                        match item {
                            Some(it) => {
                                st.apply(&it);
                                if it.cmd.is_none() {
                                    st.menu = Menu::None;
                                }
                            }
                            None => {
                                st.toolbars[k].slots[i] = Some(current);
                                changed = true;
                            }
                        }
                    }
                }
            }
            let resp = ui.interact(
                Rect::from_center_size(g.bar, Vec2::splat(2.0 * g.r)),
                Id::new("rbar_btn"),
                Sense::click(),
            );
            disc(&p, g.bar, g.r, FACE, st.menu == Menu::Bar);
            // The current tool's slot look, with the toolbar number.
            toolbars_icon(&p, g.bar, g.r * 0.8, st.active_bar + 1);
            if resp.clicked() {
                st.menu = if st.menu == Menu::Bar {
                    Menu::None
                } else {
                    Menu::Bar
                };
                if st.menu == Menu::None {
                    st.bar_menu = false;
                }
            }
            resp.on_hover_text(format!("Toolbar {} — tap to open", st.active_bar + 1));
        });
    if changed {
        st.presets_dirty = true;
    }
}
