use futures::channel::oneshot;
use gui::model::pick::{Kind, Picked, Picker};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::{File, HtmlInputElement};

struct Chooser;

impl Picker for Chooser {
    fn pick(&self, kind: Kind) -> oneshot::Receiver<Vec<Picked>> {
        let (tx, rx) = oneshot::channel();
        if let Some(input) = file_input(kind) {
            wait_for_choice(&input, tx);
            input.click();
        }
        rx
    }
}

fn file_input(kind: Kind) -> Option<HtmlInputElement> {
    let document = web_sys::window()?.document()?;
    let input: HtmlInputElement = document.create_element("input").ok()?.dyn_into().ok()?;
    input.set_type("file");
    input.set_multiple(true);
    if kind == Kind::Photos {
        input.set_accept("image/*");
    }
    Some(input)
}

fn wait_for_choice(input: &HtmlInputElement, tx: oneshot::Sender<Vec<Picked>>) {
    let answer = Rc::new(RefCell::new(Some(tx)));
    let chosen = {
        let answer = answer.clone();
        let input = input.clone();
        Closure::once_into_js(move || {
            let Some(tx) = answer.take() else {
                return;
            };
            wasm_bindgen_futures::spawn_local(async move {
                let _ = tx.send(read_all(&input).await);
            });
        })
    };
    let cancelled = Closure::once_into_js(move || {
        if let Some(tx) = answer.take() {
            let _ = tx.send(Vec::new());
        }
    });
    input.set_onchange(Some(chosen.unchecked_ref()));
    let _ = input.add_event_listener_with_callback("cancel", cancelled.unchecked_ref());
}

async fn read_all(input: &HtmlInputElement) -> Vec<Picked> {
    let Some(list) = input.files() else {
        return Vec::new();
    };
    let mut picked = Vec::new();
    for ix in 0..list.length() {
        if let Some(file) = list.get(ix)
            && let Some(read) = read(&file).await
        {
            picked.push(read);
        }
    }
    picked
}

async fn read(file: &File) -> Option<Picked> {
    let buffer = JsFuture::from(file.array_buffer()).await.ok()?;
    Some(Picked {
        name: file.name(),
        mime: file.type_(),
        bytes: js_sys::Uint8Array::new(&buffer).to_vec(),
    })
}

pub fn install() {
    gui::model::pick::install(Rc::new(Chooser));
}
