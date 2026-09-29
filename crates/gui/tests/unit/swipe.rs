use super::*;
use crate::model::{settings::Settings, state};
use bezel::gpui::{PlatformInput, ScrollDelta, point, px, size};

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("cydonia-swipe-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("project")).unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", root.join("config")) };
        Self(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open_phone(
    scratch: &Scratch,
    cx: &mut gpui::TestAppContext,
) -> (gpui::Entity<Cydonia>, gpui::VisualTestContext) {
    cx.update(|cx| bezel::theme::Theme::install(bezel::theme::Appearance::Dark, cx));
    let handle = cx.open_window(size(px(390.), px(800.)), |window, cx| {
        Cydonia::new(Settings::default(), state::State::default(), window, cx)
    });
    let root = handle.root(cx).unwrap();
    let mut visual = gpui::VisualTestContext::from_window(handle.into(), cx);
    let path = scratch.0.join("project");
    root.update(&mut visual, |root, cx| {
        root.workspace.update(cx, |workspace, cx| {
            workspace.open_project(path, cx);
            workspace.new_article(cx);
        });
    });
    (root, visual)
}

fn pan(across: f32, down: f32, visual: &mut gpui::VisualTestContext) {
    pan_from(point(px(200.), px(400.)), across, down, visual);
}

fn pan_from(
    at: gpui::Point<gpui::Pixels>,
    across: f32,
    down: f32,
    visual: &mut gpui::VisualTestContext,
) {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let step = |touch_phase, x: f32, y: f32| {
        PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(x), px(y))),
            modifiers: gpui::Modifiers::default(),
            touch_phase,
        })
    };
    visual.update(|window, cx| {
        window.dispatch_event(step(TouchPhase::Started, across / 4., down / 4.), cx);
        for _ in 0..3 {
            window.dispatch_event(step(TouchPhase::Moved, across / 4., down / 4.), cx);
        }
        window.dispatch_event(step(TouchPhase::Ended, 0., 0.), cx);
    });
}

fn open(root: &gpui::Entity<Cydonia>, visual: &mut gpui::VisualTestContext) -> (bool, bool) {
    root.read_with(visual, |root, _| (root.sidebar_open, root.changes_open))
}

#[gpui::test]
fn a_phone_starts_with_the_sidebar_closed(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("start");
    let (root, mut visual) = open_phone(&scratch, cx);

    assert_eq!(open(&root, &mut visual), (false, false));
}

#[gpui::test]
fn swiping_right_opens_the_sidebar_and_left_closes_it(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("sidebar");
    let (root, mut visual) = open_phone(&scratch, cx);

    pan(200., 0., &mut visual);
    assert_eq!(open(&root, &mut visual), (true, false));
    pan(-200., 0., &mut visual);
    assert_eq!(open(&root, &mut visual), (false, false));
}

#[gpui::test]
fn swiping_left_opens_the_right_panel_and_right_closes_it(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("panel");
    let (root, mut visual) = open_phone(&scratch, cx);

    pan(-200., 0., &mut visual);
    assert_eq!(open(&root, &mut visual), (false, true));
    pan(200., 0., &mut visual);
    assert_eq!(open(&root, &mut visual), (false, false));
}

#[gpui::test]
fn a_vertical_or_short_pan_opens_nothing(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("still");
    let (root, mut visual) = open_phone(&scratch, cx);

    pan(0., 300., &mut visual);
    pan(12., 0., &mut visual);
    assert_eq!(open(&root, &mut visual), (false, false));
}

#[gpui::test]
fn a_quick_short_flick_still_opens_the_sidebar(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("flick");
    let (root, mut visual) = open_phone(&scratch, cx);

    pan(24., 0., &mut visual);
    assert_eq!(open(&root, &mut visual), (true, false));
}

#[gpui::test]
fn swiping_up_from_the_bottom_opens_quick_actions_and_down_closes_them(
    cx: &mut gpui::TestAppContext,
) {
    let scratch = Scratch::new("quick");
    let (root, mut visual) = open_phone(&scratch, cx);

    pan_from(point(px(200.), px(760.)), 0., -200., &mut visual);
    assert!(root.read_with(&visual, |root, _| root.quick));
    pan(0., 200., &mut visual);
    assert!(!root.read_with(&visual, |root, _| root.quick));
}

#[gpui::test]
fn swiping_up_from_the_lower_half_of_a_settled_transcript_opens_quick_actions(
    cx: &mut gpui::TestAppContext,
) {
    let scratch = Scratch::new("quick-transcript");
    let (root, mut visual) = open_phone(&scratch, cx);

    visual.update(|window, cx| window.draw(cx).clear(cx));
    bezel::ui::touch::mark_pull(gpui::Bounds::new(point(px(0.), px(100.)), size(px(390.), px(600.))));
    let at = point(px(200.), px(450.));
    let step = |touch_phase, y: f32| {
        PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(0.), px(y))),
            modifiers: gpui::Modifiers::default(),
            touch_phase,
        })
    };
    visual.update(|window, cx| {
        window.dispatch_event(step(TouchPhase::Started, -8.), cx);
        window.dispatch_event(step(TouchPhase::Moved, -8.), cx);
        window.dispatch_event(step(TouchPhase::Moved, -10.), cx);
        window.dispatch_event(step(TouchPhase::Ended, 0.), cx);
    });
    assert!(root.read_with(&visual, |root, _| root.quick));
}

#[gpui::test]
fn swiping_up_from_the_middle_leaves_quick_actions_closed(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("quick-middle");
    let (root, mut visual) = open_phone(&scratch, cx);

    pan(0., -200., &mut visual);
    assert!(!root.read_with(&visual, |root, _| root.quick));
}
