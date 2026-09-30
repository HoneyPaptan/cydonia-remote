use base64::{Engine as _, engine::general_purpose::STANDARD};
use bezel::gpui::RenderImage;
use gui::model::{
    backdrop::{self, Effect, Stage, Vault},
    settings::Appearance,
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use wasm_bindgen::{Clamped, JsCast as _};
use web_sys::{CanvasRenderingContext2d, Document, HtmlCanvasElement, HtmlElement, ImageData};

const IMAGE: &str = "cydonia.backdrop.image";
const LOOK: &str = "cydonia.backdrop.look";
const SHELL: &str = "cydonia-backdrop";

#[derive(Serialize, Deserialize)]
struct Look {
    effect: Effect,
    intensity: f32,
    #[serde(default)]
    blur: f32,
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn read(key: &str) -> Option<String> {
    storage()?.get_item(key).ok()?
}

fn write(key: &str, body: &str) {
    if let Some(storage) = storage() {
        let _ = storage.set_item(key, body);
    }
}

struct Shelf;

impl Vault for Shelf {
    fn image(&self) -> Option<Vec<u8>> {
        STANDARD.decode(read(IMAGE)?).ok()
    }

    fn keep_image(&self, bytes: &[u8]) {
        write(IMAGE, &STANDARD.encode(bytes));
    }

    fn drop_image(&self) {
        if let Some(storage) = storage() {
            let _ = storage.remove_item(IMAGE);
        }
    }

    fn keep_look(&self, effect: Effect, intensity: f32, blur: f32) {
        if let Ok(body) = serde_json::to_string(&Look { effect, intensity, blur }) {
            write(LOOK, &body);
        }
    }
}

struct Wall {
    shell: HtmlElement,
    canvas: HtmlCanvasElement,
    document: Document,
    shown: RefCell<Option<Arc<RenderImage>>>,
}

impl Wall {
    fn new(document: Document) -> Option<Self> {
        let body = document.body()?;
        let shell: HtmlElement = document.create_element("div").ok()?.dyn_into().ok()?;
        shell.set_id(SHELL);
        let canvas: HtmlCanvasElement = document.create_element("canvas").ok()?.dyn_into().ok()?;
        let _ = canvas.set_attribute(
            "style",
            "display:block;width:100%;height:100%;object-fit:cover",
        );
        let _ = shell.append_child(&canvas);
        let _ = body.insert_before(&shell, body.first_child().as_ref());
        Some(Self {
            shell,
            canvas,
            document,
            shown: RefCell::new(None),
        })
    }

    fn app_canvas(&self) -> Option<HtmlElement> {
        self.document
            .query_selector("body > canvas")
            .ok()??
            .dyn_into()
            .ok()
    }

    fn blend(&self, mode: &str) {
        if let Some(canvas) = self.app_canvas() {
            let _ = canvas.style().set_property("mix-blend-mode", mode);
        }
    }

    fn paint(&self, art: &RenderImage) -> Option<()> {
        let size = art.size(0);
        let (width, height) = (size.width.0 as u32, size.height.0 as u32);
        let mut pixels = art.as_bytes(0)?.to_vec();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        self.canvas.set_width(width);
        self.canvas.set_height(height);
        let context: CanvasRenderingContext2d =
            self.canvas.get_context("2d").ok()??.dyn_into().ok()?;
        let data =
            ImageData::new_with_u8_clamped_array_and_sh(Clamped(&pixels), width, height).ok()?;
        context.put_image_data(&data, 0., 0.).ok()
    }

    fn hide(&self) {
        let _ = self.shell.set_attribute("style", "display:none");
        self.blend("normal");
        self.shown.borrow_mut().take();
    }
}

impl Stage for Wall {
    fn show(&self, art: Option<Arc<RenderImage>>, opacity: f32, light: bool) {
        let Some(art) = art else {
            self.hide();
            return;
        };
        let fresh = self
            .shown
            .borrow()
            .as_ref()
            .is_none_or(|held| !Arc::ptr_eq(held, &art));
        if fresh && self.paint(&art).is_some() {
            *self.shown.borrow_mut() = Some(art);
        }
        let (base, mode) = if light { ("#fff", "multiply") } else { ("#000", "screen") };
        let _ = self.shell.set_attribute(
            "style",
            &format!("position:fixed;inset:0;z-index:0;pointer-events:none;background:{base}"),
        );
        let _ = self
            .canvas
            .style()
            .set_property("opacity", &opacity.to_string());
        self.blend(mode);
    }
}

pub fn install(appearance: &mut Appearance) {
    backdrop::install_vault(Rc::new(Shelf));
    if let Some(look) = read(LOOK).and_then(|body| serde_json::from_str::<Look>(&body).ok()) {
        appearance.background_effect = look.effect;
        appearance.background_intensity = backdrop::clamp_intensity(look.intensity);
        appearance.background_blur = backdrop::clamp_blur(look.blur);
    }
    if let Some(wall) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(Wall::new)
    {
        backdrop::install_stage(Rc::new(wall));
    }
}
