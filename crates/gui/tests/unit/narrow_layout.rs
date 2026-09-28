use super::*;
use crate::model::{settings::Settings, state};
use crate::view::leaf::Pane;
use bezel::gpui::{self, px, size};

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("cydonia-narrow-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("project")).unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", root.join("config")) };
        Self(root)
    }

    fn project(&self) -> std::path::PathBuf {
        self.0.join("project")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open_at_width<'a>(
    width: f32,
    scratch: &Scratch,
    cx: &'a mut gpui::TestAppContext,
) -> (gpui::Entity<Cydonia>, &'a mut gpui::VisualTestContext) {
    cx.update(|cx| Theme::install(bezel::theme::Appearance::Dark, cx));
    let (root, visual) = cx.add_window_view(|window, cx| {
        Cydonia::new(Settings::default(), state::State::default(), window, cx)
    });
    visual.simulate_resize(size(px(width), px(800.)));
    let path = scratch.project();
    root.update(visual, |root, cx| {
        root.workspace.update(cx, |workspace, cx| {
            workspace.open_project(path, cx);
            workspace.new_article(cx);
        });
    });
    (root, visual)
}

fn open_first_article(root: &mut Cydonia, cx: &mut gpui::Context<Cydonia>) {
    root.workspace
        .update(cx, |workspace, cx| workspace.open_article(0, 0, cx));
    root.leaf_mut().pane = Pane::Article;
}

fn tap_in_drawer(
    root: &gpui::Entity<Cydonia>,
    visual: &mut gpui::VisualTestContext,
    tap: impl FnOnce(&mut Cydonia, &mut gpui::Context<Cydonia>),
) {
    root.update(visual, |root, cx| {
        root.remember_drawer_front(cx);
        tap(root, cx);
        root.close_drawer_on_arrival(cx);
    });
}

fn docked(root: &gpui::Entity<Cydonia>, visual: &mut gpui::VisualTestContext) -> bool {
    visual.update(|window, cx| root.read(cx).sidebar_docked(window))
}

#[gpui::test]
fn a_phone_width_shows_the_sidebar_as_a_drawer(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("phone");
    let (root, visual) = open_at_width(390., &scratch, cx);

    assert!(root.read_with(visual, |root, _| root.sidebar_open));
    assert!(!docked(&root, visual), "the drawer floats over the detail");
}

#[gpui::test]
fn opening_an_entry_on_a_phone_closes_the_drawer(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("arrive");
    let (root, visual) = open_at_width(390., &scratch, cx);

    tap_in_drawer(&root, visual, open_first_article);

    assert!(!root.read_with(visual, |root, _| root.sidebar_open));
}

#[gpui::test]
fn a_tap_that_opens_nothing_leaves_the_drawer_open(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("fold");
    let (root, visual) = open_at_width(390., &scratch, cx);

    tap_in_drawer(&root, visual, |_, _| {});

    assert!(root.read_with(visual, |root, _| root.sidebar_open));
}

#[gpui::test]
fn a_desktop_width_keeps_the_sidebar_docked_and_open(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("desktop");
    let (root, visual) = open_at_width(1200., &scratch, cx);

    assert!(docked(&root, visual), "sidebar beside the detail as before");
    root.update(visual, open_first_article);
    assert!(root.read_with(visual, |root, _| root.sidebar_open));
}
