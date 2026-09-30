use crate::view::component::image_preview::Preview;
use bezel::{
    gpui::{
        self, AnyElement, Context, Entity, MouseButton, ObjectFit, Pixels, Point, Render,
        SharedString, Window, div, img, prelude::*, px, relative,
    },
    theme::Theme,
    ui::{icons, tooltip::Tooltip},
};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

pub(crate) fn document(text: &str) -> (markdown::Doc, Vec<String>, Vec<String>) {
    let mut doc = markdown::parse(text);
    let mut images = Vec::new();
    let mut files = Vec::new();
    doc.blocks.retain(|block| match &block.kind {
        markdown::BlockKind::Image { url, .. } => {
            images.push(url.clone());
            false
        }
        markdown::BlockKind::Paragraph(text) => match attached_file(text) {
            Some(title) => {
                files.push(title);
                false
            }
            None => true,
        },
        _ => true,
    });
    (doc, images, files)
}

fn attached_file(text: &markdown::Text) -> Option<String> {
    let [span] = text.marks.as_slice() else {
        return None;
    };
    let markdown::Mark::Link(url) = &span.mark else {
        return None;
    };
    let whole = span.range == (0..text.text.len());
    (whole && !url.is_empty() && !url.contains("://") && !text.text.is_empty())
        .then(|| text.text.clone())
}

/// Where an image URL in a message points: a `file:` URL or a path against
/// the session's directory reads from disk, any other URL is fetched.
pub(crate) fn source(url: &str, cwd: &Path) -> gpui::ImageSource {
    if let Ok(url) = url::Url::parse(url) {
        if let Some(path) = crate::model::file_url::to_path(&url) {
            return Arc::<Path>::from(path).into();
        }
        return SharedString::from(url.to_string()).into();
    }
    Arc::<Path>::from(cwd.join(url)).into()
}

const CHIP_HEIGHT: f32 = 26.;
const CHIP_MAX_WIDTH: f32 = 180.;

pub(crate) fn chips(titles: Vec<String>, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .gap(px(6.))
        .children(titles.into_iter().map(|title| {
            div()
                .h(px(CHIP_HEIGHT))
                .max_w(px(CHIP_MAX_WIDTH))
                .px(px(8.))
                .rounded(px(6.))
                .bg(theme.surface)
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    icons::icon(icons::files::FileText)
                        .size(px(14.))
                        .flex_none()
                        .text_color(theme.text_muted),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_xs()
                        .text_color(theme.text)
                        .truncate()
                        .child(title),
                )
        }))
        .into_any_element()
}

fn local_path(url: &str, cwd: &Path) -> Option<PathBuf> {
    match url::Url::parse(url) {
        Ok(url) => crate::model::file_url::to_path(&url),
        Err(_) => Some(cwd.join(url)),
    }
}

#[cfg(target_family = "wasm")]
fn remote_picture(path: &Path) -> Option<Arc<gpui::Image>> {
    use std::{cell::RefCell, collections::HashMap};
    thread_local! {
        static DRAWN: RefCell<HashMap<PathBuf, Arc<gpui::Image>>> = RefCell::new(HashMap::new());
    }
    if let Some(drawn) = DRAWN.with_borrow(|drawn| drawn.get(path).cloned()) {
        return Some(drawn);
    }
    let bytes = crate::model::disk::read(path).ok()?;
    let format =
        gpui::ImageFormat::from_mime_type(image::guess_format(&bytes).ok()?.to_mime_type())?;
    let drawn = Arc::new(gpui::Image::from_bytes(format, bytes));
    DRAWN.with_borrow_mut(|held| held.insert(path.to_owned(), drawn.clone()));
    Some(drawn)
}

#[cfg(not(target_family = "wasm"))]
fn remote_picture(_: &Path) -> Option<Arc<gpui::Image>> {
    None
}

pub(crate) struct Gallery {
    images: Vec<gpui::ImageSource>,
    paths: Vec<Option<PathBuf>>,
    loaded: usize,
    selected: usize,
    preview: Entity<Preview>,
    press: Option<Point<Pixels>>,
    drag: f32,
    moved: bool,
    width: Rc<Cell<f32>>,
}

impl Gallery {
    pub(crate) fn new(images: Vec<String>, cwd: &Path, cx: &mut Context<Self>) -> Self {
        let paths = images.iter().map(|url| local_path(url, cwd)).collect();
        let images: Vec<_> = images.iter().map(|url| source(url, cwd)).collect();
        let preview = cx.new(|cx| Preview::new(images.clone(), cx));
        // Closing the preview leaves the strip on the image it was showing.
        cx.observe(&preview, |this, preview, cx| {
            let preview = preview.read(cx);
            if !preview.open && this.selected != preview.selected {
                this.selected = preview.selected;
                cx.notify();
            }
        })
        .detach();
        Self {
            images,
            paths,
            loaded: 0,
            selected: 0,
            preview,
            press: None,
            drag: 0.,
            moved: false,
            width: Default::default(),
        }
    }

    pub(crate) fn is_preview_open(&self, cx: &gpui::App) -> bool {
        self.preview.read(cx).open
    }

    fn load_remote(&mut self, cx: &mut Context<Self>) {
        let mut loaded = 0;
        for (source, path) in self.images.iter_mut().zip(&self.paths) {
            if matches!(source, gpui::ImageSource::Image(_)) {
                loaded += 1;
            } else if let Some(picture) = path.as_deref().and_then(remote_picture) {
                *source = picture.into();
                loaded += 1;
            }
        }
        if loaded != self.loaded {
            self.loaded = loaded;
            let images = self.images.clone();
            self.preview
                .update(cx, |preview, _| preview.images = images);
        }
    }

    fn finish(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.press.take().is_none() {
            return;
        }
        let threshold = (self.width.get() * 0.15).clamp(24., 80.);
        if self.drag < -threshold {
            self.selected = (self.selected + 1).min(self.images.len() - 1);
        } else if self.drag > threshold {
            self.selected = self.selected.saturating_sub(1);
        } else if !self.moved && open {
            let selected = self.selected;
            self.preview
                .update(cx, |preview, cx| preview.show(selected, window, cx));
        }
        self.drag = 0.;
        self.moved = false;
        cx.notify();
    }
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.load_remote(cx);
        let measured = self.width.clone();
        let theme = Theme::of(cx).clone();
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .id("sent-image")
                    .debug_selector(|| "sent-image".into())
                    .w_full()
                    .h(px(240.))
                    .relative()
                    .overflow_hidden()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                            this.press = Some(event.position);
                            this.drag = 0.;
                            this.moved = false;
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                        if let Some(start) = this.press {
                            let delta = f32::from(event.position.x - start.x);
                            this.drag = if this.images.len() > 1 { delta } else { 0. };
                            this.moved |=
                                delta.abs() > 5. || (event.position.y - start.y).abs() > px(5.);
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.finish(true, window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.finish(false, window, cx)),
                    )
                    .children(
                        self.images
                            .iter()
                            .enumerate()
                            .filter(|(index, _)| index.abs_diff(self.selected) <= 1)
                            .map(|(index, source)| {
                                div()
                                    .absolute()
                                    .top_0()
                                    .left(relative(index as f32 - self.selected as f32))
                                    .ml(px(self.drag))
                                    .size_full()
                                    .child(crate::view::picture::framed(
                                        source,
                                        crate::model::pictures::Fit::Contain,
                                        px(0.),
                                        img(source.clone())
                                            .absolute()
                                            .inset_0()
                                            .size_full()
                                            .object_fit(ObjectFit::Contain),
                                    ))
                            }),
                    )
                    .child(
                        gpui::canvas(
                            move |bounds, _, _| measured.set(f32::from(bounds.size.width)),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    ),
            )
            .when(self.images.len() > 1, |gallery| {
                gallery.child(
                    div()
                        .flex()
                        .justify_center()
                        .children((0..self.images.len()).map(|index| {
                            div()
                                .id(("image-dot", index))
                                .debug_selector(move || format!("image-dot-{index}"))
                                .size(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .tooltip(move |window, cx| {
                                    Tooltip::text(format!("Image {}", index + 1), window, cx)
                                })
                                .child(div().size(px(6.)).rounded_full().bg(
                                    if index == self.selected {
                                        theme.text
                                    } else {
                                        theme.text_faint.opacity(0.4)
                                    },
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.selected = index;
                                    this.drag = 0.;
                                    this.press = None;
                                    cx.notify();
                                }))
                        })),
                )
            })
            .child(self.preview.clone())
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/transcript_gallery.rs"]
mod tests;
