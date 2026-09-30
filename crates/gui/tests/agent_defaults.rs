use cydonia_gui::model::{
    session_preferences::{self, Choices},
    settings::Agent,
};
use std::path::PathBuf;

fn agent(name: &str) -> Agent {
    Agent {
        name: name.to_owned(),
        id: None,
        command: "agent".to_owned(),
        args: Vec::new(),
        env: Default::default(),
    }
}

#[test]
fn a_choice_made_in_one_project_is_the_start_of_the_agent_in_another() {
    let config = std::env::temp_dir().join(format!("cydonia-defaults-{}", std::process::id()));
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &config) };
    let agent = agent("Worker");
    let here = PathBuf::from("/work/here");
    let there = PathBuf::from("/work/there");
    session_preferences::remember_mode(&here, &agent, "bypass").unwrap();
    assert_eq!(
        session_preferences::load(&there, &agent, None),
        Choices {
            mode: Some("bypass".to_owned()),
            ..Choices::default()
        }
    );
}
