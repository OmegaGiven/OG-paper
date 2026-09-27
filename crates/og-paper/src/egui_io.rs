// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Feeds winit events to egui. Native builds use `egui-winit` (clipboard, IME,
//! cursors); the web build uses a small translator, since `egui-winit` does
//! not target the browser.

use winit::event::WindowEvent;
use winit::window::Window;

pub struct EguiIo {
    #[cfg(not(target_arch = "wasm32"))]
    state: egui_winit::State,
    #[cfg(target_arch = "wasm32")]
    web: web::WebInput,
}

/// (consumed by egui, egui wants a repaint)
pub type Response = (bool, bool);

impl EguiIo {
    pub fn new(ctx: &egui::Context, window: &Window, max_texture_side: usize) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let state = egui_winit::State::new(
                ctx.clone(),
                egui::ViewportId::ROOT,
                window,
                Some(window.scale_factor() as f32),
                None,
                Some(max_texture_side),
            );
            Self { state }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self {
                web: web::WebInput::new(ctx.clone(), window, max_texture_side),
            }
        }
    }

    pub fn on_event(&mut self, window: &Window, event: &WindowEvent) -> Response {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let r = self.state.on_window_event(window, event);
            (r.consumed, r.repaint)
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.web.on_event(window, event)
        }
    }

    pub fn take_input(&mut self, window: &Window) -> egui::RawInput {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.state.take_egui_input(window)
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.web.take(window)
        }
    }

    pub fn output(&mut self, window: &Window, out: egui::PlatformOutput) {
        #[cfg(not(target_arch = "wasm32"))]
        self.state.handle_platform_output(window, out);
        #[cfg(target_arch = "wasm32")]
        let _ = (window, out);
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use egui::{Event, Modifiers, PointerButton, Pos2, Rect, Vec2};
    use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
    use winit::keyboard::{Key, NamedKey};
    use winit::window::Window;

    pub struct WebInput {
        ctx: egui::Context,
        events: Vec<Event>,
        pos: Option<Pos2>,
        mods: Modifiers,
        max_texture_side: usize,
        start: web_time::Instant,
    }

    impl WebInput {
        pub fn new(ctx: egui::Context, _window: &Window, max_texture_side: usize) -> Self {
            Self {
                ctx,
                events: Vec::new(),
                pos: None,
                mods: Modifiers::default(),
                max_texture_side,
                start: web_time::Instant::now(),
            }
        }

        fn ppp(window: &Window) -> f32 {
            window.scale_factor() as f32
        }

        pub fn on_event(&mut self, window: &Window, event: &WindowEvent) -> (bool, bool) {
            let ppp = Self::ppp(window);
            let wants_pointer =
                self.ctx.egui_wants_pointer_input() || self.ctx.is_pointer_over_egui();
            match event {
                WindowEvent::CursorMoved { position, .. } => {
                    let p = Pos2::new(position.x as f32 / ppp, position.y as f32 / ppp);
                    self.pos = Some(p);
                    self.events.push(Event::PointerMoved(p));
                    (self.ctx.egui_is_using_pointer(), true)
                }
                WindowEvent::CursorLeft { .. } => {
                    self.pos = None;
                    self.events.push(Event::PointerGone);
                    (false, true)
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    let button = match button {
                        MouseButton::Left => PointerButton::Primary,
                        MouseButton::Right => PointerButton::Secondary,
                        MouseButton::Middle => PointerButton::Middle,
                        _ => return (false, false),
                    };
                    if let Some(pos) = self.pos {
                        self.events.push(Event::PointerButton {
                            pos,
                            button,
                            pressed: *state == ElementState::Pressed,
                            modifiers: self.mods,
                        });
                    }
                    (wants_pointer, true)
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let (unit, d) = match delta {
                        MouseScrollDelta::LineDelta(x, y) => {
                            (egui::MouseWheelUnit::Line, Vec2::new(*x, *y))
                        }
                        MouseScrollDelta::PixelDelta(p) => (
                            egui::MouseWheelUnit::Point,
                            Vec2::new(p.x as f32, p.y as f32) / ppp,
                        ),
                    };
                    self.events.push(Event::MouseWheel {
                        unit,
                        delta: d,
                        phase: egui::TouchPhase::Move,
                        modifiers: self.mods,
                    });
                    (wants_pointer, true)
                }
                WindowEvent::Touch(t) => {
                    // Single-touch taps on the UI act like a mouse; the canvas
                    // handles multi-touch itself.
                    let p = Pos2::new(t.location.x as f32 / ppp, t.location.y as f32 / ppp);
                    match t.phase {
                        TouchPhase::Started => {
                            self.pos = Some(p);
                            self.events.push(Event::PointerMoved(p));
                            self.events.push(Event::PointerButton {
                                pos: p,
                                button: PointerButton::Primary,
                                pressed: true,
                                modifiers: self.mods,
                            });
                        }
                        TouchPhase::Moved => {
                            self.pos = Some(p);
                            self.events.push(Event::PointerMoved(p));
                        }
                        TouchPhase::Ended | TouchPhase::Cancelled => {
                            self.events.push(Event::PointerButton {
                                pos: p,
                                button: PointerButton::Primary,
                                pressed: false,
                                modifiers: self.mods,
                            });
                            self.events.push(Event::PointerGone);
                            self.pos = None;
                        }
                    }
                    // Decide with the position of this touch: is it on a panel?
                    let over = self.ctx.layer_id_at(p).is_some();
                    (over || self.ctx.egui_is_using_pointer(), true)
                }
                WindowEvent::ModifiersChanged(m) => {
                    let s = m.state();
                    self.mods = Modifiers {
                        alt: s.alt_key(),
                        ctrl: s.control_key(),
                        shift: s.shift_key(),
                        mac_cmd: false,
                        command: s.control_key() || s.super_key(),
                    };
                    (false, false)
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    let pressed = event.state == ElementState::Pressed;
                    let key = match &event.logical_key {
                        Key::Named(n) => match n {
                            NamedKey::Enter => Some(egui::Key::Enter),
                            NamedKey::Tab => Some(egui::Key::Tab),
                            NamedKey::Backspace => Some(egui::Key::Backspace),
                            NamedKey::Delete => Some(egui::Key::Delete),
                            NamedKey::Escape => Some(egui::Key::Escape),
                            NamedKey::ArrowLeft => Some(egui::Key::ArrowLeft),
                            NamedKey::ArrowRight => Some(egui::Key::ArrowRight),
                            NamedKey::ArrowUp => Some(egui::Key::ArrowUp),
                            NamedKey::ArrowDown => Some(egui::Key::ArrowDown),
                            NamedKey::Home => Some(egui::Key::Home),
                            NamedKey::End => Some(egui::Key::End),
                            _ => None,
                        },
                        Key::Character(c) => egui::Key::from_name(&c.to_uppercase()),
                        _ => None,
                    };
                    if let Some(key) = key {
                        self.events.push(Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: event.repeat,
                            modifiers: self.mods,
                        });
                    }
                    if pressed && !self.mods.command {
                        if let Some(text) = &event.text {
                            if text.chars().all(|c| !c.is_control()) {
                                self.events.push(Event::Text(text.to_string()));
                            }
                        }
                    }
                    (self.ctx.egui_wants_keyboard_input(), true)
                }
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => (false, true),
                _ => (false, false),
            }
        }

        pub fn take(&mut self, window: &Window) -> egui::RawInput {
            let ppp = Self::ppp(window);
            let size = window.inner_size();
            let mut raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(size.width as f32, size.height as f32) / ppp,
                )),
                max_texture_side: Some(self.max_texture_side),
                time: Some(self.start.elapsed().as_secs_f64()),
                events: std::mem::take(&mut self.events),
                focused: true,
                ..Default::default()
            };
            raw.viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .native_pixels_per_point = Some(ppp);
            raw
        }
    }
}
