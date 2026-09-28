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

fn panel_open(root: &gpui::Entity<Cydonia>, visual: &mut gpui::VisualTestContext) -> bool {
    visual.update(|window, cx| {
        root.update(cx, |root, cx| {
            root.sync_changes(window, cx);
            root.changes_open
        })
    })
}

#[gpui::test]
fn a_phone_width_keeps_the_right_panel_down_on_arrival(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("panel-phone");
    let (root, visual) = open_at_width(390., &scratch, cx);

    assert!(!panel_open(&root, visual), "the panel would hide the header");
}

#[gpui::test]
fn a_phone_width_opens_the_right_panel_when_asked(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("panel-ask");
    let (root, visual) = open_at_width(390., &scratch, cx);
    panel_open(&root, visual);

    visual.update(|window, cx| {
        root.update(cx, |root, cx| root.toggle_changes(&ToggleChanges, window, cx))
    });

    assert!(panel_open(&root, visual));
}

#[gpui::test]
fn a_desktop_width_opens_the_right_panel_on_arrival(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("panel-desktop");
    let (root, visual) = open_at_width(1200., &scratch, cx);

    assert!(panel_open(&root, visual), "a new directory opens with the panel up");
}

fn tap(selector: &'static str, visual: &mut gpui::VisualTestContext) {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let position = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"))
        .center();
    let down = gpui::MouseDownEvent {
        button: gpui::MouseButton::Left,
        position,
        modifiers: gpui::Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    };
    let up = gpui::MouseUpEvent {
        button: gpui::MouseButton::Left,
        position,
        modifiers: gpui::Modifiers::default(),
        click_count: 1,
    };
    visual.update(|window, cx| {
        window.dispatch_event(gpui::PlatformInput::MouseDown(down), cx);
        window.dispatch_event(gpui::PlatformInput::MouseUp(up), cx);
    });
}

#[gpui::test]
fn a_real_tap_on_an_entry_in_the_drawer_closes_it(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("real-tap");
    let (root, visual) = open_at_width(390., &scratch, cx);
    root.update(visual, |root, cx| {
        root.workspace.update(cx, |workspace, cx| workspace.new_article(cx));
    });

    tap("article-row-0-1", visual);
    assert!(!root.read_with(visual, |root, _| root.sidebar_open));
}

fn touch_tap(selector: &'static str, id: u64, visual: &mut gpui::VisualTestContext) {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let position = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"))
        .center();
    touch_tap_at(position, id, visual);
}

fn touch_tap_at(position: gpui::Point<gpui::Pixels>, id: u64, visual: &mut gpui::VisualTestContext) {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let touch = |phase| {
        gpui::PlatformInput::Touch(gpui::TouchEvent {
            id: gpui::TouchId(id),
            phase,
            position,
            predicted_position: None,
            force: None,
        })
    };
    visual.update(|window, cx| {
        window.dispatch_event(touch(gpui::TouchPhase::Started), cx);
        window.dispatch_event(touch(gpui::TouchPhase::Ended), cx);
    });
}

#[gpui::test]
fn taps_keep_working_after_one_opens_an_entry(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("touch");
    let (root, visual) = open_at_width(390., &scratch, cx);
    root.update(visual, |root, cx| {
        root.workspace.update(cx, |workspace, cx| workspace.new_article(cx));
    });

    touch_tap("article-row-0-1", 1, visual);
    assert!(!root.read_with(visual, |root, _| root.sidebar_open), "first tap closes");
    touch_tap("toggle-sidebar", 2, visual);
    assert!(root.read_with(visual, |root, _| root.sidebar_open), "second tap reopens");
    touch_tap("article-row-0-0", 3, visual);
    assert!(!root.read_with(visual, |root, _| root.sidebar_open), "third tap closes");
}

#[gpui::test]
fn taps_keep_working_after_a_tap_on_the_drawer_corner(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("corner");
    let (root, visual) = open_at_width(390., &scratch, cx);

    touch_tap_at(gpui::point(px(27.), px(18.)), 1, visual);
    touch_tap("article-row-0-0", 2, visual);
    assert!(!root.read_with(visual, |root, _| root.sidebar_open), "the row still answers");
}
