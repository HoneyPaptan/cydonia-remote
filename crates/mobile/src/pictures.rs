use bezel::gpui::Pixels;
use gui::model::pictures::{self, Fit, Pictures, Placed};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, hash_map::Entry},
    rc::Rc,
};
use wasm_bindgen::JsCast as _;
use web_sys::{Blob, BlobPropertyBag, Document, HtmlElement, HtmlImageElement, Url};

#[derive(Clone, Copy)]
struct Rect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Rect {
    fn is_empty(self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }

    fn overlaps(self, other: Rect) -> bool {
        self.left < other.right && other.left < self.right && self.top < other.bottom && other.top < self.bottom
    }

    fn without(self, other: Rect) -> Vec<Rect> {
        if !self.overlaps(other) {
            return vec![self];
        }
        let middle_top = self.top.max(other.top);
        let middle_bottom = self.bottom.min(other.bottom);
        [
            Rect { bottom: other.top, ..self },
            Rect { top: other.bottom, ..self },
            Rect { top: middle_top, bottom: middle_bottom, right: other.left, ..self },
            Rect { top: middle_top, bottom: middle_bottom, left: other.right, ..self },
        ]
        .into_iter()
        .map(|part| Rect {
            top: part.top.max(self.top),
            bottom: part.bottom.min(self.bottom),
            ..part
        })
        .filter(|part| !part.is_empty())
        .collect()
    }
}

fn disjoint(holes: Vec<Rect>) -> Vec<Rect> {
    let mut kept: Vec<Rect> = Vec::new();
    for hole in holes {
        let mut parts = vec![hole];
        for held in &kept {
            parts = parts.into_iter().flat_map(|part| part.without(*held)).collect();
        }
        kept.extend(parts);
    }
    kept
}

struct Shown {
    frame: HtmlElement,
    picture: HtmlImageElement,
    placed: String,
    seen: bool,
}

struct Layer {
    document: Document,
    root: HtmlElement,
    scale: f32,
    urls: RefCell<HashMap<u64, String>>,
    shown: RefCell<HashMap<u64, Shown>>,
    sweeping: Cell<bool>,
}

thread_local! {
    static LAYER: RefCell<Option<Rc<Layer>>> = const { RefCell::new(None) };
}

fn layer() -> Option<Rc<Layer>> {
    LAYER.with(|held| held.borrow().clone())
}

fn interface_scale(window: &web_sys::Window) -> f32 {
    js_sys::Reflect::get(window, &"cydoniaScale".into())
        .ok()
        .and_then(|scale| scale.as_f64())
        .unwrap_or(1.) as f32
}

fn url_of(bytes: &[u8], mime: &str) -> Option<String> {
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let options = BlobPropertyBag::new();
    options.set_type(mime);
    let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &options).ok()?;
    Url::create_object_url_with_blob(&blob).ok()
}

fn fit(fit: Fit) -> &'static str {
    match fit {
        Fit::Cover => "cover",
        Fit::Contain => "contain",
    }
}

impl Layer {
    fn url(&self, placed: &Placed) -> Option<String> {
        let id = placed.image.id();
        if let Some(url) = self.urls.borrow().get(&id) {
            return Some(url.clone());
        }
        let url = url_of(&placed.image.bytes, placed.image.format.mime_type())?;
        self.urls.borrow_mut().insert(id, url.clone());
        Some(url)
    }

    fn make(&self) -> Option<(HtmlElement, HtmlImageElement)> {
        let frame: HtmlElement = self.document.create_element("div").ok()?.dyn_into().ok()?;
        let picture: HtmlImageElement = self.document.create_element("img").ok()?.dyn_into().ok()?;
        picture.set_draggable(false);
        let _ = picture.set_attribute("decoding", "async");
        let _ = frame.append_child(&picture);
        let _ = self.root.append_child(&frame);
        Some((frame, picture))
    }

    fn styles(&self, placed: &Placed) -> (String, String) {
        let at = |value: Pixels| f32::from(value) * self.scale;
        let (clip, bounds) = (placed.clip, placed.bounds);
        let frame = format!(
            "position:fixed;overflow:hidden;pointer-events:none;left:{}px;top:{}px;width:{}px;height:{}px{}",
            at(clip.origin.x),
            at(clip.origin.y),
            at(clip.size.width),
            at(clip.size.height),
            self.holes(placed),
        );
        let picture = format!(
            "position:absolute;display:block;max-width:none;left:{}px;top:{}px;width:{}px;height:{}px;object-fit:{};border-radius:{}px",
            at(bounds.origin.x - clip.origin.x),
            at(bounds.origin.y - clip.origin.y),
            at(bounds.size.width),
            at(bounds.size.height),
            fit(placed.fit),
            at(placed.radius),
        );
        (frame, picture)
    }

    fn holes(&self, placed: &Placed) -> String {
        if placed.holes.is_empty() {
            return String::new();
        }
        let at = |value: Pixels| f32::from(value) * self.scale;
        let clip = placed.clip;
        let cut: Vec<Rect> = placed
            .holes
            .iter()
            .map(|hole| Rect {
                left: at(hole.origin.x - clip.origin.x),
                top: at(hole.origin.y - clip.origin.y),
                right: at(hole.origin.x - clip.origin.x + hole.size.width),
                bottom: at(hole.origin.y - clip.origin.y + hole.size.height),
            })
            .collect();
        let mut path = format!("M0 0H{}V{}H0Z", at(clip.size.width), at(clip.size.height));
        for hole in disjoint(cut) {
            path.push_str(&format!(
                "M{} {}H{}V{}H{}Z",
                hole.left, hole.top, hole.right, hole.bottom, hole.left
            ));
        }
        format!(";clip-path:path(evenodd,'{path}')")
    }

    fn put(&self, placed: Placed) {
        let Some(url) = self.url(&placed) else {
            return;
        };
        let (frame, picture) = self.styles(&placed);
        let placement = format!("{url}{frame}{picture}");
        let mut shown = self.shown.borrow_mut();
        let entry = match shown.entry(placed.key) {
            Entry::Occupied(held) => held.into_mut(),
            Entry::Vacant(free) => {
                let Some((frame, picture)) = self.make() else {
                    return;
                };
                free.insert(Shown {
                    frame,
                    picture,
                    placed: String::new(),
                    seen: false,
                })
            }
        };
        entry.seen = true;
        if entry.placed == placement {
            return;
        }
        let _ = entry.frame.set_attribute("style", &frame);
        let _ = entry.picture.set_attribute("style", &picture);
        if entry.picture.src() != url {
            entry.picture.set_src(&url);
        }
        entry.placed = placement;
    }

    fn sweep(&self) {
        self.sweeping.set(false);
        for entry in self.shown.borrow_mut().values_mut() {
            if !entry.seen && !entry.placed.is_empty() {
                let _ = entry.frame.set_attribute("style", "display:none");
                entry.placed.clear();
            }
        }
    }
}

struct Dom;

impl Pictures for Dom {
    fn frame(&self) {
        let Some(layer) = layer() else {
            return;
        };
        for entry in layer.shown.borrow_mut().values_mut() {
            entry.seen = false;
        }
        if layer.sweeping.replace(true) {
            return;
        }
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(layer) = self::layer() {
                layer.sweep();
            }
        });
    }

    fn place(&self, placed: Placed) {
        if let Some(layer) = layer() {
            layer.put(placed);
        }
    }
}

fn build() -> Option<Layer> {
    let window = web_sys::window()?;
    let document = window.document()?;
    let root: HtmlElement = document.create_element("div").ok()?.dyn_into().ok()?;
    root.set_id("pictures");
    let _ = root.set_attribute("style", "position:fixed;inset:0;pointer-events:none;overflow:hidden");
    document.body()?.append_child(&root).ok()?;
    Some(Layer {
        document,
        root,
        scale: interface_scale(&window),
        urls: RefCell::default(),
        shown: RefCell::default(),
        sweeping: Cell::new(false),
    })
}

pub fn install() {
    let Some(layer) = build() else {
        return;
    };
    LAYER.with(|held| *held.borrow_mut() = Some(Rc::new(layer)));
    pictures::install(Rc::new(Dom));
}
