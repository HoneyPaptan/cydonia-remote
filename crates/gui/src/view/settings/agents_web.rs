use crate::{
    model::{relay, sink},
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
use remote::proto::{AgentListing, Answer, Query};

enum Offer {
    Working,
    Remove,
    Install,
    Confirm,
    Unavailable,
}

impl Offer {
    fn of(listing: &AgentListing, arming: Option<&str>) -> Self {
        match listing {
            AgentListing { busy: true, .. } => Self::Working,
            AgentListing {
                installed: Some(_), ..
            } => Self::Remove,
            AgentListing {
                installable: false, ..
            } => Self::Unavailable,
            _ if arming == Some(listing.id.as_str()) => Self::Confirm,
            _ => Self::Install,
        }
    }
}

impl SettingsWindow {
    pub(super) fn load(&mut self, cx: &mut Context<Self>) {
        self.arming = None;
        relay::tell(Query::Agents);
        cx.notify();
    }

    pub(super) fn trust_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let _ = cx;
        None
    }

    pub(super) fn agents_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let catalogue = match relay::ask(Query::Agents) {
            Ok(Answer::Agents { mut agents }) => {
                agents.sort_by_key(|listing| (listing.installed.is_none(), listing.name.to_lowercase()));
                self.catalogue(agents, cx)
            }
            Ok(_) => self.agents_note("The laptop could not list the registry.", cx),
            Err(_) => self.agents_note("Reading the registry on the laptop", cx),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(super::GROUP_GAP))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(super::LABEL_GAP))
                    .child(theme.field_label("On your laptop"))
                    .child(self.configured(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(super::LABEL_GAP))
                    .child(theme.field_label("Registry"))
                    .child(catalogue),
            )
            .into_any_element()
    }

    fn configured(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let names: Vec<String> = self
            .workspace
            .read(cx)
            .settings
            .agents
            .iter()
            .map(|agent| agent.name.clone())
            .collect();
        if names.is_empty() {
            return self.agents_note("No agent is set up on the laptop yet.", cx);
        }
        theme
            .group_box()
            .children(names.into_iter().enumerate().map(|(ix, name)| {
                theme
                    .card_row(ix == 0)
                    .gap(px(12.))
                    .child(
                        icons::icon(icons::development::Bot)
                            .size(px(16.))
                            .text_color(theme.text_muted),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(theme.row_title(name)))
            }))
            .into_any_element()
    }

    fn catalogue(&self, agents: Vec<AgentListing>, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let rows: Vec<AnyElement> = agents
            .into_iter()
            .enumerate()
            .map(|(ix, listing)| self.listing_row(ix, listing, cx))
            .collect();
        theme.group_box().children(rows).into_any_element()
    }

    fn listing_row(&self, ix: usize, listing: AgentListing, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let painter = Painter::of(cx);
        let offer = Offer::of(&listing, self.arming.as_deref());
        let version = match &listing.installed {
            Some(installed) => format!("Installed {installed}"),
            None => format!("Version {}", listing.version),
        };
        let about = listing
            .description
            .clone()
            .map_or(version.clone(), |description| format!("{version}. {description}"));
        let id = listing.id.clone();
        let key = SharedString::from(format!("agent-offer-{ix}"));
        let control = match offer {
            Offer::Working => div()
                .text_style(TextStyle::Callout)
                .text_color(theme.text_muted)
                .child("Working")
                .into_any_element(),
            Offer::Unavailable => div()
                .text_style(TextStyle::Callout)
                .text_color(theme.text_faint)
                .child("Not for this laptop")
                .into_any_element(),
            Offer::Remove | Offer::Install | Offer::Confirm => {
                let (label, style) = match offer {
                    Offer::Remove => ("Remove", ButtonStyle::Ghost),
                    Offer::Confirm => ("Confirm install", ButtonStyle::Prominent),
                    _ => ("Install", ButtonStyle::Ghost),
                };
                theme
                    .button(label, style, Some(Fade::new(painter, key.clone())))
                    .id(key)
                    .flex_none()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.offer_agent(&id, cx);
                    }))
                    .into_any_element()
            }
        };
        theme
            .card_row(ix == 0)
            .gap(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(theme.row_title(listing.name))
                    .child(
                        div()
                            .mt(px(4.))
                            .line_clamp(2)
                            .text_style(TextStyle::Subheadline)
                            .text_color(theme.text_muted)
                            .child(about),
                    ),
            )
            .child(control)
            .into_any_element()
    }

    fn offer_agent(&mut self, id: &str, cx: &mut Context<Self>) {
        let Ok(Answer::Agents { agents }) = relay::ask(Query::Agents) else {
            return;
        };
        let Some(listing) = agents.iter().find(|listing| listing.id == id) else {
            return;
        };
        let Some(sink) = sink::get() else {
            return;
        };
        match Offer::of(listing, self.arming.as_deref()) {
            Offer::Install => self.arming = Some(id.to_owned()),
            Offer::Confirm => {
                self.arming = None;
                sink.agent(id, true);
                self.mark_working(id, agents.clone());
            }
            Offer::Remove => {
                sink.agent(id, false);
                self.mark_working(id, agents.clone());
            }
            Offer::Working | Offer::Unavailable => {}
        }
        cx.notify();
    }

    fn mark_working(&self, id: &str, mut agents: Vec<AgentListing>) {
        for listing in agents.iter_mut().filter(|listing| listing.id == id) {
            listing.busy = true;
        }
        relay::assume(Query::Agents, Answer::Agents { agents });
    }

    fn agents_note(&self, copy: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        theme
            .group_box()
            .child(
                theme.card_row(true).child(
                    div()
                        .text_style(TextStyle::Callout)
                        .text_color(theme.text_muted)
                        .child(copy),
                ),
            )
            .into_any_element()
    }
}
