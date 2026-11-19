// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Android window chrome: full screen (status bar hidden, a swipe from the
//! top shows it for a moment, through the window flags) and, through JNI on
//! the Activity, the insets the app's own controls must stay inside (status
//! bar, gesture/navigation bar, display cutout).

use jni::objects::{JObject, JValue};
use jni::JNIEnv;
use winit::platform::android::activity::AndroidApp;

/// The Activity (`ndk_context`'s context is the Application, which has no
/// window), with the app held so that reference stays valid: it lives as
/// long as the `AndroidApp`. Read once, as `activity_as_ptr` takes the
/// app's lock.
static APP: std::sync::Mutex<Option<(AndroidApp, usize)>> = std::sync::Mutex::new(None);

/// Remember the app (from `android_main`) for the window calls below.
pub fn set_app(app: &AndroidApp) {
    let activity = app.activity_as_ptr() as usize;
    *APP.lock().unwrap_or_else(|e| e.into_inner()) = Some((app.clone(), activity));
}

fn with_env<T>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>) -> Option<T> {
    let raw = APP.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.1 as *mut std::ffi::c_void;
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    // Borrowed, never deleted (see `AndroidApp::activity_as_ptr`).
    let activity = unsafe { JObject::from_raw(raw.cast()) };
    let r = f(&mut env, &activity);
    // Said once: on Android 10 (no `WindowInsets.Type`) every frame fails.
    static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let first = || !SAID.swap(true, std::sync::atomic::Ordering::Relaxed);
    if env.exception_check().unwrap_or(false) {
        if first() {
            let _ = env.exception_describe();
        }
        let _ = env.exception_clear();
        return None;
    }
    if let Err(e) = &r {
        if first() {
            log::warn!("android: {e}");
        }
    }
    r.ok()
}

/// Full screen (Settings, off by default): hide the status bar, a swipe
/// from the top shows it for a moment. The window then starts below where
/// the bar was (a black band), as only the UI thread can change how it fits
/// the bars, and this app (no Java code) can't reach that thread; with the
/// bar shown, the controls keep clear of it through `insets`.
/// Called once from `android_main`, before the event loop runs:
/// `set_window_flags` takes a lock the loop holds while it runs the app, so
/// a later call would hang it. A change in Settings applies from the next
/// start.
pub fn set_fullscreen(app: &AndroidApp, on: bool) {
    use winit::platform::android::activity::WindowManagerFlags as F;
    if on {
        app.set_window_flags(F::FULLSCREEN, F::empty());
    }
}

/// Pixels the system bars and the display cutout take, whether shown now or
/// not: (left, top, right, bottom). Zero when unknown.
pub fn insets() -> [f32; 4] {
    with_env(|env, activity| {
        let window = env
            .call_method(activity, "getWindow", "()Landroid/view/Window;", &[])?
            .l()?;
        let decor = env
            .call_method(&window, "getDecorView", "()Landroid/view/View;", &[])?
            .l()?;
        let wi = env
            .call_method(
                &decor,
                "getRootWindowInsets",
                "()Landroid/view/WindowInsets;",
                &[],
            )?
            .l()?;
        if wi.is_null() {
            return Ok([0.0; 4]);
        }
        let bars = env
            .call_static_method("android/view/WindowInsets$Type", "systemBars", "()I", &[])?
            .i()?;
        let cut = env
            .call_static_method(
                "android/view/WindowInsets$Type",
                "displayCutout",
                "()I",
                &[],
            )?
            .i()?;
        let ins = env
            .call_method(
                &wi,
                "getInsetsIgnoringVisibility",
                "(I)Landroid/graphics/Insets;",
                &[JValue::Int(bars | cut)],
            )?
            .l()?;
        let mut o = [0.0f32; 4];
        for (k, f) in ["left", "top", "right", "bottom"].iter().enumerate() {
            o[k] = env.get_field(&ins, f, "I")?.i()? as f32;
        }
        Ok(o)
    })
    .unwrap_or([0.0; 4])
}
