// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! The floating toolbar, file menu and status chip (egui).

use egui::{Align2, Color32, Id, Sense, Stroke, Vec2};
use ogpaper_core::Brush;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Pen,
    Marker,
    Highlighter,
    Eraser,
    Hand,
}

impl Tool {
    pub fn brush(self) -> Option<Brush> {
        match self {
            Tool::Pen => Some(Brush::Pen),
            Tool::Marker => Some(Brush::Marker),
            Tool::Highlighter => Some(Brush::Highlighter),
            _ => None,
        }
    }
}

/// Per-brush settings; width is in screen pixels at the zoom you draw at.
#[derive(Clone, Copy)]
pub struct InkSettings {
    pub color: Color32,
    pub width: f32,
}

pub struct UiState {
    pub tool: Tool,
    pub pen: InkSettings,
    pub marker: InkSettings,
    pub highlighter: InkSettings,
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
            pen: InkSettings {
                color: Color32::from_rgb(28, 28, 36),
                width: 3.0,
            },
            marker: InkSettings {
                color: Color32::from_rgb(30, 90, 200),
                width: 8.0,
            },
            highlighter: InkSettings {
                color: Color32::from_rgb(255, 214, 0),
                width: 22.0,
            },
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
        match self.tool {
            Tool::Pen => Some(&mut self.pen),
            Tool::Marker => Some(&mut self.marker),
            Tool::Highlighter => Some(&mut self.highlighter),
            _ => None,
        }
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

const INK: [Color32; 10] = [
    Color32::from_rgb(28, 28, 36),
    Color32::from_rgb(110, 110, 120),
    Color32::from_rgb(30, 90, 200),
    Color32::from_rgb(210, 40, 60),
    Color32::from_rgb(20, 140, 80),
    Color32::from_rgb(240, 130, 20),
    Color32::from_rgb(130, 60, 190),
    Color32::from_rgb(220, 60, 150),
    Color32::from_rgb(0, 160, 170),
    Color32::from_rgb(250, 250, 250),
];

const HIGHLIGHT: [Color32; 6] = [
    Color32::from_rgb(255, 214, 0),
    Color32::from_rgb(120, 230, 90),
    Color32::from_rgb(255, 120, 200),
    Color32::from_rgb(90, 200, 255),
    Color32::from_rgb(255, 160, 60),
    Color32::from_rgb(190, 140, 255),
];

/// Draw the UI; returns actions for the app to perform.
pub fn draw(ctx: &egui::Context, st: &mut UiState) -> Vec<Action> {
    let mut actions = Vec::new();
    let big = st.touch_ui;

    // Toolbar, top centre.
    egui::Area::new(Id::new("toolbar"))
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 10.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                if big {
                    ui.style_mut().spacing.button_padding = Vec2::new(10.0, 8.0);
                }
                ui.horizontal(|ui| {
                    for (tool, label, tip) in [
                        (Tool::Pen, "Pen", "Pen: width follows pressure (1)"),
                        (Tool::Marker, "Marker", "Marker: constant width (2)"),
                        (
                            Tool::Highlighter,
                            "Highlighter",
                            "Highlighter: tints without hiding ink (3)",
                        ),
                        (Tool::Eraser, "Eraser", "Stroke eraser (E)"),
                        (Tool::Hand, "Pan", "Pan the canvas (H, or hold Space)"),
                    ] {
                        if ui
                            .selectable_label(st.tool == tool, label)
                            .on_hover_text(tip)
                            .clicked()
                        {
                            st.tool = tool;
                        }
                    }
                    ui.separator();
                    let hl = st.tool == Tool::Highlighter;
                    if let Some(ink) = st.ink() {
                        let palette: &[Color32] = if hl { &HIGHLIGHT } else { &INK };
                        for &c in palette {
                            if swatch(ui, c, ink.color == c, big).clicked() {
                                ink.color = c;
                            }
                        }
                        ui.color_edit_button_srgba(&mut ink.color)
                            .on_hover_text("Custom color");
                        ui.separator();
                        let max = if hl { 80.0 } else { 48.0 };
                        ui.add(
                            egui::Slider::new(&mut ink.width, 0.5..=max)
                                .logarithmic(true)
                                .suffix(" px"),
                        )
                        .on_hover_text("Width at the current zoom");
                        ui.separator();
                    }
                    if ui
                        .add_enabled(st.can_undo, egui::Button::new("Undo"))
                        .on_hover_text("Undo (Ctrl+Z)")
                        .clicked()
                    {
                        actions.push(Action::Undo);
                    }
                    if ui
                        .add_enabled(st.can_redo, egui::Button::new("Redo"))
                        .on_hover_text("Redo (Ctrl+Shift+Z)")
                        .clicked()
                    {
                        actions.push(Action::Redo);
                    }
                });
            });
        });

    // File menu, top left.
    egui::Area::new(Id::new("file"))
        .anchor(Align2::LEFT_TOP, Vec2::new(10.0, 10.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.menu_button("☰", |ui| {
                        if ui.button("New canvas  (Ctrl+N)").clicked() {
                            actions.push(Action::New);
                        }
                        if ui.button("Open…  (Ctrl+O)").clicked() {
                            actions.push(Action::Open);
                        }
                        if ui.button("Save as…  (Ctrl+Shift+S)").clicked() {
                            actions.push(Action::SaveAs);
                        }
                    });
                    ui.label(&st.file_name)
                        .on_hover_text("Saved automatically after every stroke");
                });
            });
        });

    // Zoom / status, bottom right.
    egui::Area::new(Id::new("status"))
        .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-10.0, -10.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("zoom 10^{:.1}", st.zoom_log10));
                    ui.label(format!("{} strokes", st.strokes));
                    if ui
                        .button("Home")
                        .on_hover_text("Back to the start (Home key)")
                        .clicked()
                    {
                        actions.push(Action::Home);
                    }
                });
            });
        });

    if let Some(msg) = &st.message {
        egui::Area::new(Id::new("msg"))
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -14.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(msg);
                });
            });
    }
    actions
}

fn swatch(ui: &mut egui::Ui, c: Color32, selected: bool, big: bool) -> egui::Response {
    let d = if big { 30.0 } else { 20.0 };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(d), Sense::click());
    let p = ui.painter();
    p.circle_filled(rect.center(), d * 0.4, c);
    p.circle_stroke(
        rect.center(),
        d * 0.4,
        Stroke::new(1.0, Color32::from_gray(150)),
    );
    if selected {
        p.circle_stroke(
            rect.center(),
            d * 0.5 - 1.0,
            Stroke::new(2.0, ui.visuals().selection.bg_fill),
        );
    }
    resp
}
