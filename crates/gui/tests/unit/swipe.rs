use super::*;
use crate::model::{settings::Settings, state};
use bezel::gpui::{Focusable as _, PlatformInput, ScrollDelta, point, px, size};

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

fn open_bare_phone(
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
        });
    });
    (root, visual)
}

fn open_phone(
    scratch: &Scratch,
    cx: &mut gpui::TestAppContext,
) -> (gpui::Entity<Cydonia>, gpui::VisualTestContext) {
    let (root, mut visual) = open_bare_phone(scratch, cx);
    root.update(&mut visual, |root, cx| {
        root.workspace.update(cx, |workspace, cx| {
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
    let (root, mut visual) = open_bare_phone(&scratch, cx);

    pan(-200., 0., &mut visual);
    assert_eq!(open(&root, &mut visual), (false, true));
    pan(200., 0., &mut visual);
    assert_eq!(open(&root, &mut visual), (false, false));
}

#[gpui::test]
fn swiping_left_over_an_article_leaves_the_right_panel_shut(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("panel-article");
    let (root, mut visual) = open_phone(&scratch, cx);
    root.update_in(&mut visual, |root, window, cx| root.open_article(0, 0, window, cx));

    pan(-200., 0., &mut visual);
    assert_eq!(open(&root, &mut visual), (false, false));
    pan_from(point(px(385.), px(400.)), -200., 0., &mut visual);
    assert_eq!(open(&root, &mut visual), (false, false));
}

fn open_resting_session(root: &gpui::Entity<Cydonia>, visual: &mut gpui::VisualTestContext) {
    let record = serde_json::from_value(serde_json::json!({
        "id": "test", "agent": "test", "title": "", "name": null,
        "updated": 1, "items": []
    }))
    .expect("a record");
    root.update(visual, |root, cx| {
        root.workspace.update(cx, |workspace, cx| {
            let path = workspace.projects[0].path.clone();
            let agent = crate::model::settings::Agent {
                name: "test".into(),
                id: None,
                command: String::new(),
                args: Vec::new(),
                env: Default::default(),
            };
            let chat = crate::model::session::ChatSession::restore(7, path, agent, record);
            workspace.projects[0].sessions.push(chat);
            workspace.projects[0].active = Some(7);
            cx.notify();
        });
    });
}

#[gpui::test]
fn swiping_left_over_a_session_leaves_the_right_panel_shut(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("panel-session");
    let (root, mut visual) = open_bare_phone(&scratch, cx);
    open_resting_session(&root, &mut visual);

    pan(-200., 0., &mut visual);
    assert_eq!(open(&root, &mut visual), (false, false));
    pan_from(point(px(385.), px(400.)), -200., 0., &mut visual);
    assert_eq!(open(&root, &mut visual), (false, false));
}

#[gpui::test]
fn swiping_left_over_an_article_still_closes_the_sidebar(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("panel-article-drawer");
    let (root, mut visual) = open_phone(&scratch, cx);
    root.update_in(&mut visual, |root, window, cx| root.open_article(0, 0, window, cx));

    pan(200., 0., &mut visual);
    assert_eq!(open(&root, &mut visual), (true, false));
    pan(-200., 0., &mut visual);
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

#[gpui::test]
fn a_quick_action_asks_for_its_project_and_runs_there(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("quick-project");
    let (root, mut visual) = open_phone(&scratch, cx);
    let second = scratch.0.join("second");
    std::fs::create_dir_all(&second).unwrap();

    root.update_in(&mut visual, |root, window, cx| {
        root.open_quick_actions(cx);
        root.choose_quick(crate::view::quick::Quick::Article, window, cx);
    });
    assert!(root.read_with(&visual, |root, _| root.quick && root.quick_step.is_some()));

    root.update_in(&mut visual, |root, window, cx| {
        root.quick_in_new_project(window, cx);
        root.workspace
            .update(cx, |workspace, cx| workspace.open_project(second.clone(), cx));
    });
    root.read_with(&visual, |root, cx| {
        assert!(!root.quick);
        assert!(root.quick_pending.is_none());
        let workspace = root.workspace.read(cx);
        let active = workspace.active.unwrap();
        assert_eq!(workspace.projects[active].path, second);
    });
}

fn named_agent(name: &str) -> crate::model::settings::Agent {
    crate::model::settings::Agent {
        name: name.into(),
        id: None,
        command: String::new(),
        args: vec![],
        env: Default::default(),
    }
}

#[gpui::test]
fn a_quick_session_asks_for_its_agent_after_its_project(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("quick-agent");
    let (root, mut visual) = open_phone(&scratch, cx);
    root.update_in(&mut visual, |root, window, cx| {
        root.workspace.update(cx, |workspace, _| {
            workspace.settings.features.sessions = true;
            workspace.settings.agents = vec![named_agent("first"), named_agent("second")];
        });
        root.open_quick_actions(cx);
        root.choose_quick(crate::view::quick::Quick::Session, window, cx);
        root.quick_in_project(0, window, cx);
    });
    assert!(root.read_with(&visual, |root, _| {
        root.quick && matches!(root.quick_step, Some(crate::view::quick::Step::Agent))
    }));

    root.update_in(&mut visual, |root, window, cx| root.quick_with_agent(1, window, cx));
    root.read_with(&visual, |root, cx| {
        assert!(!root.quick);
        let agent = root.workspace.read(cx).preferred_agent().unwrap();
        assert_eq!(agent.name, "second");
    });
}

fn grips_found(visual: &mut gpui::VisualTestContext) -> Vec<gpui::Point<gpui::Pixels>> {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.update(|window, cx| {
        (0..39)
            .flat_map(|x| (0..80).map(move |y| point(px(x as f32 * 10.), px(y as f32 * 10.))))
            .filter(|at| touch::grip_at(*at, window, cx).is_some())
            .collect()
    })
}

fn focus_the_article(root: &gpui::Entity<Cydonia>, visual: &mut gpui::VisualTestContext) {
    root.update_in(visual, |root, window, cx| {
        let editor = root.pane_doc(cx).and_then(|article| article.editor.clone());
        let editor = editor.expect("the open article has an editor");
        window.focus(&editor.focus_handle(cx), cx);
    });
}

fn open_article_with_grips(
    scratch: &Scratch,
    cx: &mut gpui::TestAppContext,
) -> (gpui::Entity<Cydonia>, gpui::VisualTestContext) {
    let (root, mut visual) = open_phone(scratch, cx);
    root.update_in(&mut visual, |root, window, cx| root.open_article(0, 0, window, cx));
    focus_the_article(&root, &mut visual);
    assert!(!grips_found(&mut visual).is_empty(), "the article shows its block grips");
    (root, visual)
}

#[gpui::test]
fn a_grip_under_quick_actions_leaves_the_touch_to_the_sheet(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("grip-quick");
    let (root, mut visual) = open_article_with_grips(&scratch, cx);

    root.update(&mut visual, |root, cx| root.open_quick_actions(cx));
    assert_eq!(grips_found(&mut visual), vec![]);
}

#[gpui::test]
fn a_grip_under_the_drawer_or_the_panel_leaves_the_touch_to_them(cx: &mut gpui::TestAppContext) {
    let scratch = Scratch::new("grip-drawer");
    let (root, mut visual) = open_article_with_grips(&scratch, cx);

    root.update(&mut visual, |root, cx| root.toggle_sidebar(cx));
    assert_eq!(grips_found(&mut visual), vec![]);
    root.update(&mut visual, |root, cx| root.toggle_sidebar(cx));
    root.update_in(&mut visual, |root, window, cx| root.toggle_changes(&ToggleChanges, window, cx));
    assert_eq!(open(&root, &mut visual), (false, true));
    focus_the_article(&root, &mut visual);
    assert_eq!(grips_found(&mut visual), vec![]);
}
