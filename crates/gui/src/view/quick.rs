use crate::view::{
    component::panel::Launch,
    root::{Cydonia, NewArticle, NewBoard, NewSession, content_bg, narrow},
};
use bezel::{
    gpui::{AnyElement, Context, Window, div, prelude::*, px},
    theme::{TextStyle, Theme, Typeset},
    ui::icons,
};

#[derive(Clone, Copy)]
enum Quick {
    Session,
    Article,
    Board,
    Tool(Launch),
}

impl Quick {
    fn all() -> impl Iterator<Item = Self> {
        [Self::Session, Self::Article, Self::Board]
            .into_iter()
            .chain(Launch::ALL.map(Self::Tool))
    }

    fn label(self) -> &'static str {
        match self {
            Self::Session => "New session",
            Self::Article => "New article",
            Self::Board => "New board",
            Self::Tool(launch) => launch.label(),
        }
    }

    fn icon(self) -> &'static [u8] {
        match self {
            Self::Session => icons::social::MessageCirclePlus,
            Self::Article => icons::files::FilePlus,
            Self::Board => icons::development::SquareKanban,
            Self::Tool(launch) => launch.icon(),
        }
    }
}

impl Cydonia {
    pub(crate) fn open_quick_actions(&mut self, cx: &mut Context<Self>) {
        self.quick = true;
        cx.notify();
    }

    pub(crate) fn close_quick_actions(&mut self, cx: &mut Context<Self>) -> bool {
        let was_open = std::mem::take(&mut self.quick);
        cx.notify();
        was_open
    }

    fn quick_ready(&self, quick: Quick, cx: &Context<Self>) -> bool {
        match quick {
            Quick::Session => true,
            Quick::Article | Quick::Board => self.workspace.read(cx).active.is_some(),
            Quick::Tool(_) => self.shell_cwd(cx).is_some(),
        }
    }

    fn run_quick(&mut self, quick: Quick, window: &mut Window, cx: &mut Context<Self>) {
        self.quick = false;
        if !self.sidebar_docked(window) {
            self.sidebar_open = false;
        }
        match quick {
            Quick::Session => self.new_session_action(&NewSession, window, cx),
            Quick::Article => self.new_article_action(&NewArticle, window, cx),
            Quick::Board => self.new_board_action(&NewBoard, window, cx),
            Quick::Tool(launch) => self.launch_tool(launch, window, cx),
        }
        cx.notify();
    }

    fn quick_button(&self, index: usize, quick: Quick, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let ready = self.quick_ready(quick, cx);
        div()
            .id(("quick-action", index))
            .h(px(48.))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .rounded(px(8.))
            .bg(theme.element_hover)
            .when(!ready, |button| button.opacity(0.4))
            .when(ready, |button| {
                button
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| this.run_quick(quick, window, cx)))
            })
            .child(icons::icon(quick.icon()).size(px(16.)).text_color(theme.text_muted))
            .child(div().min_w_0().truncate().child(quick.label()))
            .into_any_element()
    }

    pub(crate) fn quick_actions(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.quick {
            return None;
        }
        let theme = Theme::of(cx).clone();
        let columns = if narrow(window) { 2 } else { 4 };
        let buttons: Vec<AnyElement> = Quick::all()
            .enumerate()
            .map(|(index, quick)| self.quick_button(index, quick, &theme, cx))
            .collect();
        Some(
            div()
                .id("quick-actions")
                .occlude()
                .absolute()
                .inset_0()
                .bg(theme.scrim())
                .child(bezel::ui::cover::cover())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.close_quick_actions(cx);
                }))
                .child(
                    div()
                        .id("quick-actions-sheet")
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .px(px(16.))
                        .pt(px(12.))
                        .pb(px(24.))
                        .rounded_t(px(16.))
                        .bg(content_bg(&theme))
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .mx_auto()
                                .mb(px(4.))
                                .w(px(36.))
                                .h(px(4.))
                                .rounded_full()
                                .bg(theme.text_faint),
                        )
                        .child(
                            div()
                                .text_style(TextStyle::Caption)
                                .text_color(theme.text_muted)
                                .child("Quick actions"),
                        )
                        .child(div().grid().grid_cols(columns).gap(px(8.)).children(buttons)),
                )
                .into_any_element(),
        )
    }
}
