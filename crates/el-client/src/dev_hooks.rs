//! `window.__el`: a small status object the Playwright tests (and curious humans) can read.

use bevy::prelude::*;
use wasm_bindgen::{JsCast, JsValue};

/// Fields published to `window.__el` a few times a second (every frame on a slow page).
#[derive(Resource, Default)]
pub struct DevStatus {
    pub fields: Vec<(&'static str, JsValue)>,
}

impl DevStatus {
    pub fn set(&mut self, key: &'static str, value: impl Into<JsValue>) {
        let value = value.into();
        match self.fields.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.fields.push((key, value)),
        }
    }
}

pub struct DevHooksPlugin;

impl Plugin for DevHooksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DevStatus>().add_systems(Last, publish);
    }
}

/// Below this frame rate every frame publishes: frames come in bunches on a page drawing in
/// software (a quick one, then one seconds long, even a minute with a terminal open over the city),
/// and a frame skipped for coming soon after the last would leave a test reading a state the game
/// has left (a terminal Esc has closed) for as long as the next one takes.
const SLOW_FPS: f32 = 8.0;

fn publish(
    time: Res<Time<Real>>,
    mut last: Local<f32>,
    mut frames: Local<u32>,
    mut fps_window: Local<(f32, u32)>,
    mut slow: Local<bool>,
    mut status: ResMut<DevStatus>,
) {
    *frames += 1;
    let now = time.elapsed_secs();
    if *frames == 1 {
        hide_boot_overlay();
    }
    if now - fps_window.0 >= 2.0 {
        let fps = (*frames - fps_window.1) as f32 / (now - fps_window.0);
        *fps_window = (now, *frames);
        *slow = fps < SLOW_FPS;
        status.set("fps", fps);
    }
    if now - *last < 0.25 && !*slow {
        return;
    }
    *last = now;
    let frames = *frames;
    status.set("frames", frames);
    let Some(window) = web_sys::window() else { return };
    let obj = js_sys::Object::new();
    for (k, v) in &status.fields {
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), v);
    }
    let _ = js_sys::Reflect::set(&window, &JsValue::from_str("__el"), &obj);
}

/// Fades out the HTML "loading" overlay once Bevy renders its first frame (the page's
/// `elHideBoot`, which hides it once it has faded; or at once, without it).
fn hide_boot_overlay() {
    let Some(window) = web_sys::window() else { return };
    let fade = js_sys::Reflect::get(&window, &JsValue::from_str("elHideBoot"))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
    if let Some(f) = fade {
        let _ = f.call0(&window);
    } else if let Some(el) = window.document().and_then(|d| d.get_element_by_id("boot")) {
        let _ = el.class_list().add_1("hidden");
    }
}
