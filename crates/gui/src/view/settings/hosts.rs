use crate::{
    model::hosts::{self, Host, Hosts},
    view::settings::SettingsWindow,
};
use bezel::{
    gpui::{AnyElement, Context, SharedString, div, prelude::*, px},
    motion::{Fade, Painter},
    theme::{TextStyle, Theme, Typeset},
    ui::{
        icons,
        widgets::{ButtonStyle, Buttons, Scaffolding as _},
    },
};
use std::{cell::Cell, rc::Rc, time::Duration};

const RECHECK: Duration = Duration::from_secs(2);

thread_local! {
    static TICKING: Cell<bool> = const { Cell::new(false) };
}

#[cfg(feature = "desktop")]
fn shelf() -> Option<Rc<dyn Hosts>> {
    Some(Rc::new(hosts::saved::Desktop))
}

#[cfg(not(feature = "desktop"))]
fn shelf() -> Option<Rc<dyn Hosts>> {
    hosts::get()
}

fn standing(host: &Host) -> &'static str {
    match (host.current, host.reachable) {
        (true, _) => "In use",
        (false, Some(true)) => "Answering",
        (false, Some(false)) => "Not answering",
        (false, None) => "Checking",
    }
}

impl SettingsWindow {
    pub(super) fn hosts_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        self.recheck_hosts(cx);
        let saved = shelf().map(|shelf| shelf.list()).unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .gap(px(super::GROUP_GAP))
            .child(self.hosts_group("This device", self.this_device(cx), &theme))
            .child(self.hosts_group("Saved hosts", self.saved_hosts(saved, cx), &theme))
            .child(self.hosts_group("Add a host", self.add_host(cx), &theme))
            .into_any_element()
    }

    fn hosts_group(&self, label: &'static str, body: AnyElement, theme: &Theme) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(super::LABEL_GAP))
            .child(theme.field_label(label))
            .child(body)
            .into_any_element()
    }

    fn recheck_hosts(&self, cx: &mut Context<Self>) {
        if TICKING.replace(true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RECHECK).await;
            TICKING.set(false);
            let _ = this.update(cx, |this, cx| {
                if this.section == super::Section::Hosts {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn host_line(
        &self,
        first: bool,
        glyph: &'static [u8],
        title: impl Into<SharedString>,
        about: impl Into<SharedString>,
        theme: &Theme,
    ) -> bezel::gpui::Div {
        theme
            .card_row(first)
            .gap(px(12.))
            .child(icons::icon(glyph).size(px(16.)).flex_none().text_color(theme.text_muted))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().child(theme.row_title(title.into())))
                    .child(
                        div()
                            .mt(px(4.))
                            .truncate()
                            .text_style(TextStyle::Subheadline)
                            .text_color(theme.text_muted)
                            .child(about.into()),
                    ),
            )
    }

    #[cfg(feature = "desktop")]
    fn this_device(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let painter = Painter::of(cx);
        let about = match crate::remote::served() {
            Some(served) => {
                let at: Vec<String> = served.listen.iter().map(ToString::to_string).collect();
                let devices = match served.watchers {
                    1 => "1 device connected".to_owned(),
                    count => format!("{count} devices connected"),
                };
                format!("Serving on {}. {devices}", at.join(", "))
            }
            None => "Not serving. Start it with the remote flag to reach it from other devices.".to_owned(),
        };
        let row = self.host_line(true, icons::devices::Laptop, "This computer", about, &theme);
        let row = match crate::remote::pairing_link() {
            Some(link) => row.child(
                theme
                    .button("Copy pairing link", ButtonStyle::Ghost, Some(Fade::new(painter, "host-pair")))
                    .id("host-pair")
                    .flex_none()
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(bezel::gpui::ClipboardItem::new_string(link.clone()));
                    }),
            ),
            None => row,
        };
        theme.group_box().child(row).into_any_element()
    }

    #[cfg(not(feature = "desktop"))]
    fn this_device(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let address = crate::model::relay::laptop().unwrap_or("Unknown");
        let about = match shelf().is_some_and(|shelf| shelf.connected()) {
            true => "Connected",
            false => "Reconnecting",
        };
        theme
            .group_box()
            .child(self.host_line(true, icons::devices::Laptop, address.to_owned(), about, &theme))
            .into_any_element()
    }

    fn saved_hosts(&self, saved: Vec<Host>, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let painter = Painter::of(cx);
        if saved.is_empty() {
            return theme
                .group_box()
                .child(
                    theme.card_row(true).child(
                        div()
                            .text_style(TextStyle::Callout)
                            .text_color(theme.text_muted)
                            .child("No other host saved yet."),
                    ),
                )
                .into_any_element();
        }
        let rows: Vec<AnyElement> = saved
            .into_iter()
            .enumerate()
            .map(|(ix, host)| {
                let about = standing(&host);
                let opened = host.address.clone();
                let removed = host.address.clone();
                let open_key = SharedString::from(format!("host-open-{ix}"));
                let remove_key = SharedString::from(format!("host-remove-{ix}"));
                let row = self.host_line(
                    ix == 0,
                    icons::development::Server,
                    host.address.clone(),
                    about,
                    &theme,
                );
                let row = match cfg!(feature = "desktop") || host.current {
                    true => row,
                    false => row.child(
                        theme
                            .button("Open", ButtonStyle::Ghost, Some(Fade::new(painter, open_key.clone())))
                            .id(open_key)
                            .flex_none()
                            .on_click(move |_, _, _| {
                                if let Some(shelf) = shelf() {
                                    shelf.open(&opened);
                                }
                            }),
                    ),
                };
                match host.current {
                    true => row,
                    false => row.child(
                        theme
                            .button("Remove", ButtonStyle::Ghost, Some(Fade::new(painter, remove_key.clone())))
                            .id(remove_key)
                            .flex_none()
                            .on_click(cx.listener(move |_, _, _, cx| {
                                if let Some(shelf) = shelf() {
                                    shelf.remove(&removed);
                                }
                                cx.notify();
                            })),
                    ),
                }
                .into_any_element()
            })
            .collect();
        theme.group_box().children(rows).into_any_element()
    }

    fn add_host(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let painter = Painter::of(cx);
        theme
            .group_box()
            .child(
                theme
                    .card_row(true)
                    .child(div().flex_1().min_w_0().child(self.host_address.clone())),
            )
            .child(
                theme
                    .card_row(false)
                    .child(div().flex_1().min_w_0().child(self.host_token.clone())),
            )
            .child(
                theme.card_row(false).justify_end().child(
                    theme
                        .button("Add host", ButtonStyle::Prominent, Some(Fade::new(painter, "host-add")))
                        .id("host-add")
                        .flex_none()
                        .on_click(cx.listener(|this, _, _, cx| this.save_host(cx))),
                ),
            )
            .into_any_element()
    }

    fn save_host(&mut self, cx: &mut Context<Self>) {
        let address = self.host_address.read(cx).content().to_string();
        let token = self.host_token.read(cx).content().to_string();
        if hosts::normalized(&address).is_empty() {
            return;
        }
        if let Some(shelf) = shelf() {
            shelf.add(&address, &token);
        }
        self.host_address.update(cx, |field, cx| field.clear(cx));
        self.host_token.update(cx, |field, cx| field.clear(cx));
        cx.notify();
    }
}
