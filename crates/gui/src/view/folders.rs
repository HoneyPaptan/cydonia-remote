use crate::{
    model::{relay, sink},
    view::root::Cydonia,
};
use bezel::{
    gpui::{
        self, AnyElement, Context, Entity, Focusable as _, KeyBinding, SharedString, Task, Window,
        actions, div, prelude::*, px,
    },
    theme::{TextStyle, Theme, Typeset},
    ui::{
        icons,
        input::TextField,
        widgets::{ButtonStyle, Buttons, Scaffolding as _},
    },
};
use remote::proto::{Answer, Folders, Query};
use std::{path::Path, time::Duration};

actions!(cydonia_folders, [CommitFolder, DismissFolders]);

const FOLDERS_CONTEXT: &str = "CydoniaFolders";

pub fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("enter", CommitFolder, Some(FOLDERS_CONTEXT)),
        KeyBinding::new("escape", DismissFolders, Some(FOLDERS_CONTEXT)),
    ]
}

pub(crate) struct Browsing {
    path: String,
    naming: Option<Entity<TextField>>,
    making: Option<Query>,
    error: Option<SharedString>,
    _poll: Task<()>,
}

enum Listing {
    Ready(Folders),
    Failed(SharedString),
    Waiting,
}

fn listing(path: &str) -> Listing {
    match relay::ask(Query::Folders {
        path: path.to_owned(),
    }) {
        Ok(Answer::Folders(folders)) => Listing::Ready(folders),
        Ok(Answer::Failed { message }) => Listing::Failed(message.into()),
        Ok(_) => Listing::Failed("The laptop gave an odd answer".into()),
        Err(_) => Listing::Waiting,
    }
}

fn child(path: &str, name: &str) -> String {
    format!("{}/{name}", path.trim_end_matches('/'))
}

fn plain_name(name: &str) -> Option<&str> {
    let name = name.trim();
    (!name.is_empty() && name != "." && name != ".." && !name.contains('/')).then_some(name)
}

fn last_part(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |name| name.to_string_lossy().into_owned())
}

impl Cydonia {
    pub(crate) fn browse_folders(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        self.quick = false;
        let poll = cx.spawn(async move |this, cx| {
            loop {
                relay::pause(cx.background_executor(), Duration::from_secs(2)).await;
                let open = this
                    .update(cx, |this, cx| {
                        this.settle_made_folder(cx);
                        cx.notify();
                        this.browsing.is_some()
                    })
                    .unwrap_or(false);
                if !open {
                    return;
                }
            }
        });
        self.browsing = Some(Browsing {
            path: String::new(),
            naming: None,
            making: None,
            error: None,
            _poll: poll,
        });
        cx.notify();
    }

    fn settle_made_folder(&mut self, cx: &mut Context<Self>) {
        let Some(browsing) = self.browsing.as_mut() else {
            return;
        };
        let Some(answer) = browsing.making.as_ref().and_then(relay::take) else {
            return;
        };
        browsing.making = None;
        match answer {
            Answer::Folders(made) => {
                browsing.path = made.path;
                browsing.naming = None;
                browsing.error = None;
            }
            Answer::Failed { message } => browsing.error = Some(message.into()),
            _ => {}
        }
        cx.notify();
    }

    fn enter_folder(&mut self, path: String, cx: &mut Context<Self>) {
        if let Some(browsing) = self.browsing.as_mut() {
            browsing.path = path;
            browsing.error = None;
        }
        cx.notify();
    }

    fn ask_folder_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| {
            TextField::new(cx)
                .with_key_context(FOLDERS_CONTEXT)
                .with_placeholder("Folder name")
        });
        crate::view::focus_for_typing(&field.read(cx).focus_handle(cx), window, cx);
        if let Some(browsing) = self.browsing.as_mut() {
            browsing.naming = Some(field);
            browsing.error = None;
        }
        cx.notify();
    }

    pub(crate) fn make_folder(&mut self, _: &CommitFolder, _: &mut Window, cx: &mut Context<Self>) {
        let Some(browsing) = self.browsing.as_mut() else {
            return;
        };
        let Listing::Ready(here) = listing(&browsing.path) else {
            return;
        };
        let Some(field) = browsing.naming.as_ref() else {
            return;
        };
        let Some(name) = plain_name(field.read(cx).content()).map(str::to_owned) else {
            browsing.error = Some("Type a name without slashes".into());
            cx.notify();
            return;
        };
        let query = Query::MakeFolder {
            path: child(&here.path, &name),
        };
        relay::take(&query);
        relay::tell(query.clone());
        browsing.making = Some(query);
        browsing.error = None;
        cx.notify();
    }

    pub(crate) fn dismiss_folders(&mut self, _: &DismissFolders, _: &mut Window, cx: &mut Context<Self>) {
        self.browsing = None;
        cx.notify();
    }

    fn open_folder(&mut self, path: String, cx: &mut Context<Self>) {
        if let Some(sink) = sink::get() {
            sink.project(Path::new(&path), true);
        }
        self.browsing = None;
        cx.notify();
    }

    fn folder_row(
        &self,
        id: (&'static str, usize),
        icon: &'static [u8],
        label: String,
        theme: &Theme,
        go: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .flex_none()
            .h(px(44.))
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|row| row.bg(theme.element_hover))
            .child(icons::icon(icon).size(px(16.)).text_color(theme.text_muted))
            .child(div().flex_1().min_w_0().truncate().child(label))
            .on_click(cx.listener(move |this, _, _, cx| this.enter_folder(go.clone(), cx)))
            .into_any_element()
    }

    fn folder_list(&self, listing: &Listing, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let list = div()
            .id("folder-list")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(2.))
            .overflow_y_scroll();
        let note = |text: SharedString| {
            div()
                .p(px(12.))
                .text_style(TextStyle::Subheadline)
                .text_color(theme.text_muted)
                .child(text)
        };
        match listing {
            Listing::Waiting => list.child(note("Reading the laptop".into())),
            Listing::Failed(why) => list.child(note(why.clone())),
            Listing::Ready(here) => {
                let up = here.parent.clone().map(|parent| {
                    self.folder_row(("folder-up", 0), icons::arrows::ArrowUp, "Up".into(), theme, parent, cx)
                });
                let rows: Vec<AnyElement> = here
                    .folders
                    .iter()
                    .enumerate()
                    .map(|(ix, name)| {
                        self.folder_row(("folder", ix), icons::files::Folder, name.clone(), theme, child(&here.path, name), cx)
                    })
                    .collect();
                let empty = (here.folders.is_empty()).then(|| note("No folders in here".into()));
                list.children(up).children(rows).children(empty)
            }
        }
        .into_any_element()
    }

    fn naming_row(&self, field: Entity<TextField>, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(div().flex_1().min_w_0().child(field))
            .child(
                theme
                    .button("Create", ButtonStyle::Prominent, None)
                    .id("folder-create")
                    .on_click(cx.listener(|this, _, window, cx| this.make_folder(&CommitFolder, window, cx))),
            )
            .into_any_element()
    }

    fn folder_actions(&self, ready: Option<String>, naming: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let open = theme
            .button("Open as project", ButtonStyle::Prominent, None)
            .id("folder-open");
        let open = match ready.clone() {
            Some(path) => open.on_click(cx.listener(move |this, _, _, cx| this.open_folder(path.clone(), cx))),
            None => open.opacity(0.4),
        };
        div()
            .flex_none()
            .flex()
            .flex_row()
            .flex_wrap()
            .justify_end()
            .gap(px(8.))
            .when(!naming && ready.is_some(), |bar| {
                bar.child(
                    theme
                        .button("New folder", ButtonStyle::Ghost, None)
                        .id("folder-new")
                        .on_click(cx.listener(|this, _, window, cx| this.ask_folder_name(window, cx))),
                )
            })
            .child(
                theme
                    .button("Cancel", ButtonStyle::Ghost, None)
                    .id("folder-cancel")
                    .on_click(cx.listener(|this, _, window, cx| this.dismiss_folders(&DismissFolders, window, cx))),
            )
            .child(open)
            .into_any_element()
    }

    pub(crate) fn folder_picker(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let browsing = self.browsing.as_ref()?;
        let theme = Theme::of(cx).clone();
        let listing = listing(&browsing.path);
        let (title, ready) = match &listing {
            Listing::Ready(here) => (last_part(&here.path), Some(here.path.clone())),
            _ => ("Laptop".to_owned(), None),
        };
        let shown = ready.clone().unwrap_or_default();
        let tall = f32::from(window.viewport_size().height) * 0.8;
        let naming = browsing.naming.clone();
        let error = browsing.error.clone();
        let busy = browsing.making.is_some();
        let list = self.folder_list(&listing, &theme, cx);
        let actions = self.folder_actions(ready, naming.is_some(), cx);
        Some(
            div()
                .id("folders-scrim")
                .occlude()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .p(px(16.))
                .bg(theme.scrim())
                .child(bezel::ui::cover::cover())
                .on_click(cx.listener(|this, _, window, cx| this.dismiss_folders(&DismissFolders, window, cx)))
                .child(
                    div()
                        .id("folders-dialog")
                        .key_context(FOLDERS_CONTEXT)
                        .w_full()
                        .max_w(px(480.))
                        .h(px(tall))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .p(px(16.))
                        .rounded(px(Theme::panel_radius()))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.surface)
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(theme.row_title(title))
                        .child(
                            div()
                                .flex_none()
                                .text_style(TextStyle::Caption)
                                .text_color(theme.text_muted)
                                .child(shown),
                        )
                        .child(list)
                        .children(naming.map(|field| self.naming_row(field, cx)))
                        .children(busy.then(|| {
                            div()
                                .text_style(TextStyle::Caption)
                                .text_color(theme.text_muted)
                                .child("Making the folder")
                        }))
                        .children(error.map(|why| {
                            div()
                                .text_style(TextStyle::Caption)
                                .text_color(theme.danger)
                                .child(why)
                        }))
                        .child(actions),
                )
                .into_any_element(),
        )
    }
}
