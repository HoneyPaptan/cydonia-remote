use crate::net::Endpoint;
use gui::model::settings::Appearance;

const SANS_FAMILY: &str = "SF Pro Text";
const MONO_FAMILY: &str = "SFMono Nerd Font Mono";

const SANS: [&str; 5] = [
    "fonts/sans-regular.otf",
    "fonts/sans-italic.otf",
    "fonts/sans-medium.otf",
    "fonts/sans-semibold.otf",
    "fonts/sans-bold.otf",
];
const MONO: [&str; 2] = ["fonts/mono-regular.otf", "fonts/mono-bold.otf"];
const SYMBOLS: &str = "fonts/symbols.ttf";
const PICKABLE: [&str; 7] = [
    "fonts/geist-regular.otf",
    "fonts/geist-italic.otf",
    "fonts/geist-medium.otf",
    "fonts/geist-semibold.otf",
    "fonts/geist-bold.otf",
    "fonts/geist-mono-regular.otf",
    "fonts/geist-mono-bold.otf",
];

pub struct Faces {
    pub files: Vec<Vec<u8>>,
    sans: bool,
    mono: bool,
}

impl Faces {
    pub fn name_families(&self, appearance: &mut Appearance) {
        if self.sans {
            appearance.ui_font = Some(SANS_FAMILY.to_owned());
        }
        if self.mono {
            appearance.mono_font = Some(MONO_FAMILY.to_owned());
        }
    }
}

async fn fetch_all(endpoint: &Endpoint, paths: &[&str]) -> Vec<Option<Vec<u8>>> {
    futures::future::join_all(paths.iter().map(|path| endpoint.asset(path))).await
}

fn held(fetched: &[Option<Vec<u8>>]) -> bool {
    fetched.iter().any(Option::is_some)
}

pub async fn fetch(endpoint: &Endpoint) -> Faces {
    let paths: Vec<&str> = SANS
        .iter()
        .chain(&MONO)
        .chain([&SYMBOLS])
        .chain(&PICKABLE)
        .copied()
        .collect();
    let fetched = fetch_all(endpoint, &paths).await;
    let sans = held(&fetched[..SANS.len()]);
    let mono = held(&fetched[SANS.len()..SANS.len() + MONO.len()]);
    Faces {
        files: fetched.into_iter().flatten().collect(),
        sans,
        mono,
    }
}
