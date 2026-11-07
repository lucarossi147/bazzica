//! Bridge to `public/fx/effetti.js` (sounds and animations, ported from the old web client).
//! Every call is a no-op if the script is missing or fails.

use js_sys::{Array, Function, JSON, Reflect};
use wasm_bindgen::{JsCast, JsValue};

fn chiama(metodo: &str, args: &[JsValue]) -> Option<JsValue> {
    let w = web_sys::window()?;
    let fx = Reflect::get(&w, &"BazzicaFx".into()).ok()?;
    if fx.is_undefined() {
        return None;
    }
    let f: Function = Reflect::get(&fx, &metodo.into()).ok()?.dyn_into().ok()?;
    f.apply(&fx, &args.iter().collect::<Array>()).ok()
}

fn json(s: &str) -> JsValue {
    JSON::parse(s).unwrap_or(JsValue::NULL)
}

/// Remember where cards and seats are, right before the screen shows a new state.
pub fn istantanea() {
    chiama("istantanea", &[]);
}

/// Play the effects for a batch of engine events (JSON array) seen from `ctx` (JSON object).
pub fn eventi(eventi: &str, ctx: &str) {
    chiama("eventi", &[json(eventi), json(ctx)]);
}

pub fn turno() {
    chiama("turno", &[]);
}

pub fn tic() {
    chiama("tic", &[]);
}

pub fn set_muto(muto: bool) {
    chiama("setMuto", &[JsValue::from_bool(muto)]);
}

/// Saved mute preference (read from storage: the script may not be loaded yet at startup).
pub fn muto() -> bool {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item("bazzica.muto").ok().flatten())
        .is_some_and(|v| v == "1")
}
