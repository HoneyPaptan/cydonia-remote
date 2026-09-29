use artifact::{project::memory, session::record::Record};
use gui::model::{
    settings::{Agent, Settings},
    state::{self, State},
    store,
};
use remote::proto::{ProjectView, SessionView, Snapshot};

pub fn record(id: &str, view: &SessionView) -> Record {
    let header = &view.header;
    Record {
        id: id.to_owned(),
        number: header.number,
        agent: header.agent.clone(),
        agent_id: None,
        session: None,
        title: header.title.clone(),
        name: header.name.clone(),
        updated: header.updated,
        closed: header.closed,
        fork: header.fork.clone(),
        draft: String::new(),
        items: view.items.clone(),
        sent_at: header.sent_at.clone(),
    }
}

pub fn files(project: &ProjectView) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = project
        .files
        .iter()
        .map(|(path, file)| (path.clone(), file.0.clone()))
        .collect();
    for (id, view) in &project.sessions {
        if let Ok(bytes) = serde_json::to_vec(&record(id, view)) {
            files.push((format!("sessions/{id}.json"), bytes));
        }
    }
    files
}

pub fn memory_project(project: &ProjectView) -> memory::Project {
    let files = files(project);
    memory::Project::seed(
        files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    )
}

pub fn project(project: &ProjectView) {
    store::relay(project.path.clone(), memory_project(project));
}

pub fn agent(name: &str) -> Agent {
    Agent {
        name: name.to_owned(),
        id: None,
        command: "remote".into(),
        args: Vec::new(),
        env: Default::default(),
    }
}

pub fn settings(snapshot: &Snapshot) -> Settings {
    let mut names: Vec<&str> = snapshot.agents.iter().map(String::as_str).collect();
    for project in &snapshot.projects {
        for view in project.sessions.values() {
            if !names.contains(&view.header.agent.as_str()) {
                names.push(&view.header.agent);
            }
        }
    }
    Settings {
        agents: names.into_iter().map(agent).collect(),
        ..Settings::default()
    }
}

pub fn state(snapshot: &Snapshot) -> State {
    let projects = snapshot
        .projects
        .iter()
        .map(|project| project.path.clone().into())
        .collect();
    state::reconcile(state::kept().unwrap_or_default(), projects)
}
