//! Running the workspace with no window — the gate the remote daemon stands on.

use anyhow::Result;
use bezel::{
    gpui::{App, AppContext},
    gpui_platform,
};
use gui::{
    boot,
    model::{settings::Settings, state::State, workspace::Workspace},
};

const BUNDLE_ID: &str = "sh.cydonia";

/// Hold the workspace for the life of the process.
///
/// A daemon has no window to own it, and a released `Entity` takes every
/// session and agent with it, so the one owner is a leak on purpose.
pub fn run(settings: Settings, state: State) -> Result<()> {
    let app = gpui_platform::headless();
    app.run(move |cx: &mut App| {
        cx.set_app_identity(BUNDLE_ID, "Cydonia");
        boot::init(&settings, cx);
        let workspace = cx.new(|cx| Workspace::new(settings, state, cx));
        let _owned = Box::leak(Box::new(workspace));
    });
    Ok(())
}
