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

async fn fetch_all(endpoint: &Endpoint, paths: &[&str]) -> Vec<Vec<u8>> {
    futures::future::join_all(paths.iter().map(|path| endpoint.asset(path)))
        .await
        .into_iter()
        .flatten()
        .collect()
}

pub async fn fetch(endpoint: &Endpoint) -> Faces {
    let sans = fetch_all(endpoint, &SANS).await;
    let mono = fetch_all(endpoint, &MONO).await;
    let symbols = fetch_all(endpoint, &[SYMBOLS]).await;
    let (has_sans, has_mono) = (!sans.is_empty(), !mono.is_empty());
    Faces {
        files: sans.into_iter().chain(mono).chain(symbols).collect(),
        sans: has_sans,
        mono: has_mono,
    }
}
