use crate::pick;
use bezel::gpui::{AsyncApp, WindowHandle};
use gui::{model::pick as held, view::root::Cydonia};
use std::rc::Rc;
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{ClipboardEvent, Event, File, KeyboardEvent};

fn is_paste_shortcut(event: &KeyboardEvent) -> bool {
    (event.ctrl_key() || event.meta_key())
        && !event.alt_key()
        && !event.shift_key()
        && event.key().eq_ignore_ascii_case("v")
}

fn pasted_pictures(event: &ClipboardEvent) -> Vec<File> {
    let Some(items) = event.clipboard_data().map(|data| data.items()) else {
        return Vec::new();
    };
    (0..items.length())
        .filter_map(|ix| items.get(ix))
        .filter(|item| item.kind() == "file" && item.type_().starts_with("image/"))
        .filter_map(|item| item.get_as_file().ok().flatten())
        .collect()
}

fn listen(name: &str, handler: impl FnMut(Event) + 'static) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let handler = Closure::<dyn FnMut(Event)>::new(handler);
    let _ = document.add_event_listener_with_callback_and_bool(
        name,
        handler.as_ref().unchecked_ref(),
        true,
    );
    handler.forget();
}

pub fn install(window: WindowHandle<Cydonia>, sender: AsyncApp) {
    listen("keydown", |event| {
        if let Some(key) = event.dyn_ref::<KeyboardEvent>()
            && is_paste_shortcut(key)
        {
            event.stop_propagation();
        }
    });
    held::on_paste(Rc::new(move |files| {
        let _ = window.update(&mut sender.clone(), |root, window, cx| {
            root.paste_files(files, window, cx)
        });
    }));
    listen("paste", |event| {
        let Some(event) = event.dyn_ref::<ClipboardEvent>() else {
            return;
        };
        let files = pasted_pictures(event);
        if files.is_empty() {
            return;
        }
        wasm_bindgen_futures::spawn_local(async move {
            let mut picked = Vec::new();
            for file in files {
                if let Some(read) = pick::read(&file).await {
                    picked.push(read);
                }
            }
            if !picked.is_empty() {
                held::pasted(picked);
            }
        });
    });
}
