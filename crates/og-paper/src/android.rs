// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Android window chrome through JNI: immersive mode (system bars hidden,
//! a swipe from the edge shows them for a moment) and the insets the app's
//! own controls must stay inside (status bar, gesture/navigation bar,
//! display cutout).

use jni::objects::{JObject, JValue};
use jni::JNIEnv;

fn with_env<T>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>) -> Option<T> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
    let r = f(&mut env, &activity);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
        return None;
    }
    r.ok()
}

/// Hide (or show) the status and navigation bars. Swiping from an edge
/// shows them transiently. (Android 11+; older ones keep their bars.)
pub fn set_immersive(on: bool) {
    with_env(|env, activity| {
        let window = env
            .call_method(activity, "getWindow", "()Landroid/view/Window;", &[])?
            .l()?;
        let ctl = env
            .call_method(
                &window,
                "getInsetsController",
                "()Landroid/view/WindowInsetsController;",
                &[],
            )?
            .l()?;
        if ctl.is_null() {
            return Ok(());
        }
        // WindowInsets.Type.systemBars()
        let bars = env
            .call_static_method("android/view/WindowInsets$Type", "systemBars", "()I", &[])?
            .i()?;
        if on {
            // BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            env.call_method(&ctl, "setSystemBarsBehavior", "(I)V", &[JValue::Int(2)])?;
            env.call_method(&ctl, "hide", "(I)V", &[JValue::Int(bars)])?;
        } else {
            env.call_method(&ctl, "show", "(I)V", &[JValue::Int(bars)])?;
        }
        Ok(())
    });
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
