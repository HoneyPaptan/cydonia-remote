use gui::model::hosts::{Host, Hosts};
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::{JsCast as _, JsValue};

thread_local! {
    static LINKED: Cell<bool> = const { Cell::new(false) };
}

pub fn linked(up: bool) {
    LINKED.set(up);
    let body = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.body());
    if let Some(body) = body {
        let _ = body.set_attribute("data-linked", if up { "true" } else { "false" });
    }
}

fn bridge() -> Option<JsValue> {
    let window = web_sys::window()?;
    js_sys::Reflect::get(&window, &"CydoniaHosts".into())
        .ok()
        .filter(|bridge| !bridge.is_undefined())
}

fn call(method: &str, args: &[&str]) -> Option<JsValue> {
    let bridge = bridge()?;
    let function: js_sys::Function = js_sys::Reflect::get(&bridge, &method.into())
        .ok()?
        .dyn_into()
        .ok()?;
    let args: js_sys::Array = args.iter().map(|arg| JsValue::from_str(arg)).collect();
    function.apply(&bridge, &args).ok()
}

fn host(value: &serde_json::Value) -> Option<Host> {
    Some(Host {
        address: value.get("address")?.as_str()?.to_owned(),
        current: value.get("current").and_then(|current| current.as_bool()).unwrap_or(false),
        reachable: value.get("reachable").and_then(|reachable| reachable.as_bool()),
    })
}

struct Shell;

impl Hosts for Shell {
    fn list(&self) -> Vec<Host> {
        call("list", &[])
            .and_then(|text| text.as_string())
            .and_then(|text| serde_json::from_str::<Vec<serde_json::Value>>(&text).ok())
            .map(|hosts| hosts.iter().filter_map(host).collect())
            .unwrap_or_default()
    }

    fn add(&self, address: &str, token: &str) {
        call("add", &[address, token]);
    }

    fn remove(&self, address: &str) {
        call("remove", &[address]);
    }

    fn open(&self, address: &str) {
        call("open", &[address]);
    }

    fn connected(&self) -> bool {
        LINKED.get()
    }
}

pub fn install() {
    if bridge().is_some() {
        gui::model::hosts::install(Rc::new(Shell));
    }
}
