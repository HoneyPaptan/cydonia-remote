//! Running the workspace with no window — the gate the remote daemon stands on.

use anyhow::Result;
use std::sync::Arc;
use bezel::{
    gpui::{App, AppContext},
    gpui_platform,
};
use gui::{
    boot,
    model::{settings::Settings, state::State, workspace::Workspace},
    remote::Options,
};

const BUNDLE_ID: &str = "sh.cydonia";

/// Hold the workspace for the life of the process.
///
/// A daemon has no window to own it, and a released `Entity` takes every
/// session and agent with it, so the one owner is a leak on purpose.
pub fn run(settings: Settings, state: State, remote: Option<Options>) -> Result<()> {
    let remote = remote.map(|options| Options {
        step_down: Some(Arc::new(|| std::process::exit(0))),
        ..options
    });
    if let Some(options) = &remote
        && gui::remote::occupied(&options.listen)
    {
        eprintln!("another Cydonia is hosting the phone, standing by");
        return Ok(());
    }
    let app = gpui_platform::headless();
    app.run(move |cx: &mut App| {
        cx.set_app_identity(BUNDLE_ID, "Cydonia");
        boot::init(&settings, cx);
        let workspace = cx.new(|cx| Workspace::new(settings, state, cx));
        if let Some(options) = remote
            && let Err(error) = gui::remote::start(workspace.clone(), options, cx)
        {
            eprintln!("remote server failed to start: {error:#}");
            cx.quit();
        }
        let _owned = Box::leak(Box::new(workspace));
    });
    Ok(())
}
