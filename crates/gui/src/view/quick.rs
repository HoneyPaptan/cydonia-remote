use crate::view::{
    component::panel::Launch,
    root::{Cydonia, NewArticle, NewBoard, NewSession, OpenProject, narrow},
};
use bezel::{
    gpui::{self, AnyElement, Context, Window, div, prelude::*, px},
    theme::{TextStyle, Theme, Typeset},
    ui::{
        icons::{self, Icon},
        touch,
    },
};
use std::path::PathBuf;

const QUICK_LIST_HEIGHT: f32 = 420.;
const QUICK_LIST_SHARE: f32 = 0.55;
const QUICK_ROW_HEIGHT: f32 = 64.;

#[derive(Clone, Copy)]
pub(crate) enum Quick {
    Session,
    Article,
    Board,
    Project,
    Tool(Launch),
}

impl Quick {
    fn all() -> impl Iterator<Item = Self> {
        [Self::Session, Self::Article, Self::Board, Self::Project]
            .into_iter()
            .chain(Launch::ALL.map(Self::Tool))
    }

    fn label(self) -> &'static str {
        match self {
            Self::Session => "New session",
            Self::Article => "New article",
            Self::Board => "New board",
            Self::Project => "Open project",
            Self::Tool(launch) => launch.label(),
        }
    }

    fn needs_project(self) -> bool {
        !matches!(self, Self::Project)
    }

    fn icon(self) -> &'static [u8] {
        match self {
            Self::Session => icons::social::MessageCirclePlus,
            Self::Article => icons::files::FilePlus,
            Self::Board => icons::development::SquareKanban,
            Self::Project => icons::files::FolderOpen,
            Self::Tool(launch) => launch.icon(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Step {
    Project(Quick),
    Agent,
}

pub(crate) struct Pending {
    quick: Quick,
    known: Vec<PathBuf>,
    pub(crate) wanted: Option<PathBuf>,
}

impl Cydonia {
    pub(crate) fn open_quick_actions(&mut self, cx: &mut Context<Self>) {
        self.quick = true;
        self.quick_step = None;
        self.quick_pending = None;
        cx.notify();
    }

    pub(crate) fn close_quick_actions(&mut self, cx: &mut Context<Self>) -> bool {
        let was_open = std::mem::take(&mut self.quick);
        self.quick_step = None;
        cx.notify();
        was_open
    }

    pub(crate) fn choose_quick(&mut self, quick: Quick, window: &mut Window, cx: &mut Context<Self>) {
        match quick.needs_project() {
            true => {
                self.quick_step = Some(Step::Project(quick));
                cx.notify();
            }
            false => self.run_quick(quick, window, cx),
        }
    }

    fn project_step(&mut self) -> Option<Quick> {
        match self.quick_step.take() {
            Some(Step::Project(quick)) => Some(quick),
            _ => None,
        }
    }

    pub(crate) fn quick_in_project(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(quick) = self.project_step() else {
            return;
        };
        self.workspace
            .update(cx, |workspace, cx| workspace.select_project(ix, cx));
        self.continue_quick(quick, window, cx);
    }

    /// A new session asks for its agent once a project is picked, unless only
    /// one is installed, which leaves nothing to choose.
    fn continue_quick(&mut self, quick: Quick, window: &mut Window, cx: &mut Context<Self>) {
        let choices = self.workspace.read(cx).settings.agents.len();
        match quick {
            Quick::Session if choices > 1 => {
                self.quick = true;
                self.quick_step = Some(Step::Agent);
                cx.notify();
            }
            _ => self.run_quick(quick, window, cx),
        }
    }

    pub(crate) fn quick_with_agent(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_quick(window);
        self.pick_agent(ix, window, cx);
        cx.notify();
    }

    pub(crate) fn quick_in_new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(quick) = self.project_step() else {
            return;
        };
        let known = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .map(|project| project.path.clone())
            .collect();
        self.run_quick(Quick::Project, window, cx);
        self.quick_pending = Some(Pending {
            quick,
            known,
            wanted: None,
        });
    }

    pub(crate) fn settle_pending_quick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pending) = self.quick_pending.as_ref() else {
            return;
        };
        let added = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .position(|project| {
                pending.wanted.as_ref() == Some(&project.path) || !pending.known.contains(&project.path)
            });
        let Some(ix) = added else {
            return;
        };
        let quick = pending.quick;
        self.quick_pending = None;
        self.workspace
            .update(cx, |workspace, cx| workspace.select_project(ix, cx));
        self.continue_quick(quick, window, cx);
    }

    fn leave_quick(&mut self, window: &Window) {
        self.quick = false;
        self.quick_step = None;
        if !self.sidebar_docked(window) {
            self.sidebar_open = false;
        }
    }

    fn run_quick(&mut self, quick: Quick, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_quick(window);
        match quick {
            Quick::Session => self.new_session_action(&NewSession, window, cx),
            Quick::Article => self.new_article_action(&NewArticle, window, cx),
            Quick::Board => self.new_board_action(&NewBoard, window, cx),
            Quick::Project => self.open_project_action(&OpenProject, window, cx),
            Quick::Tool(launch) => self.launch_tool(launch, window, cx),
        }
        cx.notify();
    }

    fn quick_button(&self, index: usize, quick: Quick, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
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
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| this.choose_quick(quick, window, cx)))
            .child(icons::icon(quick.icon()).size(px(16.)).text_color(theme.text_muted))
            .child(div().min_w_0().truncate().child(quick.label()))
            .into_any_element()
    }

    fn project_row(
        &self,
        id: (&'static str, usize),
        icon: impl Into<Icon>,
        name: String,
        path: Option<String>,
        theme: &Theme,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .flex_none()
            .min_h(px(52.))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .py(px(8.))
            .rounded(px(8.))
            .bg(theme.element_hover)
            .cursor_pointer()
            .child(icons::icon(icon).size(px(16.)).text_color(theme.text_muted))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().min_w_0().truncate().child(name))
                    .children(path.map(|path| {
                        div()
                            .min_w_0()
                            .truncate()
                            .text_style(TextStyle::Caption)
                            .text_color(theme.text_muted)
                            .child(path)
                    })),
            )
    }

    fn quick_projects(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows: Vec<AnyElement> = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .enumerate()
            .map(|(ix, project)| {
                self.project_row(
                    ("quick-project", ix),
                    icons::files::Folder,
                    project.name(),
                    Some(project.path.to_string_lossy().into_owned()),
                    theme,
                )
                .on_click(cx.listener(move |this, _, window, cx| this.quick_in_project(ix, window, cx)))
                .into_any_element()
            })
            .collect();
        rows.push(
            self.project_row(("quick-project-add", 0), icons::files::FolderPlus, "Add project".into(), None, theme)
                .on_click(cx.listener(|this, _, window, cx| this.quick_in_new_project(window, cx)))
                .into_any_element(),
        );
        rows
    }

    fn quick_agents(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let workspace = self.workspace.read(cx);
        workspace
            .settings
            .agents
            .iter()
            .enumerate()
            .map(|(ix, agent)| {
                let icon = workspace
                    .agent_icon(&agent.name)
                    .unwrap_or_else(|| icons::development::Bot.into());
                self.project_row(("quick-agent", ix), icon, agent.name.clone(), None, theme)
                    .on_click(cx.listener(move |this, _, window, cx| this.quick_with_agent(ix, window, cx)))
                    .into_any_element()
            })
            .collect()
    }

    fn quick_list(&self, id: &'static str, rows: Vec<AnyElement>, window: &Window) -> AnyElement {
        let height = px(QUICK_LIST_HEIGHT).min(window.viewport_size().height * QUICK_LIST_SHARE);
        let scrolls = rows.len() as f32 * QUICK_ROW_HEIGHT > f32::from(height);
        div()
            .relative()
            .child(
                div()
                    .id(id)
                    .max_h(height)
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(rows),
            )
            .when(scrolls, |list| list.child(touch::scrolls()))
            .into_any_element()
    }

    pub(crate) fn quick_actions(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.quick {
            return None;
        }
        let theme = Theme::of(cx).clone();
        let columns = if narrow(window) { 2 } else { 4 };
        let (caption, body) = match self.quick_step {
            Some(Step::Project(quick)) => (
                format!("{} in which project?", quick.label()),
                self.quick_list("quick-projects", self.quick_projects(&theme, cx), window),
            ),
            Some(Step::Agent) => (
                "New session with which agent?".to_owned(),
                self.quick_list("quick-agents", self.quick_agents(&theme, cx), window),
            ),
            None => (
                "Quick actions".to_owned(),
                div()
                    .grid()
                    .grid_cols(columns)
                    .gap(px(8.))
                    .children(
                        Quick::all()
                            .enumerate()
                            .map(|(index, quick)| self.quick_button(index, quick, &theme, cx)),
                    )
                    .into_any_element(),
            ),
        };
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
                        .occlude()
                        .bg(theme.bg)
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
                                .child(caption),
                        )
                        .child(body),
                )
                .into_any_element(),
        )
    }
}
