use super::settings::{Feature, Features, Mcp};
use remote::proto::Setup;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Door {
    Serve,
    Write,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    Feature(Feature),
    Mcp(Door),
}

const DOORS: [(Door, &str); 3] = [(Door::Serve, "serve"), (Door::Write, "write"), (Door::Delete, "delete")];

static LAPTOP_MCP: Mutex<Option<String>> = Mutex::new(None);

impl Switch {
    pub fn all() -> impl Iterator<Item = Self> {
        Feature::ALL
            .into_iter()
            .chain(Feature::PANEL)
            .map(Self::Feature)
            .chain(DOORS.map(|(door, _)| Self::Mcp(door)))
    }

    pub fn key(self) -> String {
        match self {
            Self::Feature(feature) => format!("feature.{}", feature.key()),
            Self::Mcp(door) => {
                let name = DOORS.iter().find(|(held, _)| *held == door).map_or("", |(_, name)| name);
                format!("mcp.{name}")
            }
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::all().find(|switch| switch.key() == key)
    }

    pub fn read(self, features: &Features, mcp: &Mcp) -> bool {
        match self {
            Self::Feature(feature) => feature.on(features),
            Self::Mcp(Door::Serve) => mcp.serve,
            Self::Mcp(Door::Write) => mcp.write,
            Self::Mcp(Door::Delete) => mcp.delete,
        }
    }

    fn write(self, features: &mut Features, mcp: &mut Mcp, on: bool) {
        match self {
            Self::Feature(feature) => feature.set(features, on),
            Self::Mcp(Door::Serve) => mcp.serve = on,
            Self::Mcp(Door::Write) => mcp.write = on,
            Self::Mcp(Door::Delete) => mcp.delete = on,
        }
    }
}

pub fn setup(features: &Features, mcp: &Mcp, mcp_url: Option<String>) -> Setup {
    Setup {
        switches: Switch::all()
            .map(|switch| (switch.key(), switch.read(features, mcp)))
            .collect(),
        mcp_url,
    }
}

pub fn adopt(setup: &Setup, features: &mut Features, mcp: &mut Mcp) {
    for (key, on) in &setup.switches {
        if let Some(switch) = Switch::parse(key) {
            switch.write(features, mcp, *on);
        }
    }
    *LAPTOP_MCP.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = setup.mcp_url.clone();
}

pub fn laptop_mcp_url() -> Option<String> {
    LAPTOP_MCP.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

#[cfg(test)]
#[path = "../../tests/unit/switches.rs"]
mod tests;
