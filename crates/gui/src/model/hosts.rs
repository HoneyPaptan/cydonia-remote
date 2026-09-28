use std::{cell::RefCell, rc::Rc};

pub const PORT: u16 = 7878;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    pub address: String,
    pub current: bool,
    pub reachable: Option<bool>,
}

pub trait Hosts {
    fn list(&self) -> Vec<Host>;
    fn add(&self, address: &str, token: &str);
    fn remove(&self, address: &str);
    fn open(&self, address: &str);
    fn connected(&self) -> bool;
}

thread_local! {
    static HOSTS: RefCell<Option<Rc<dyn Hosts>>> = const { RefCell::new(None) };
}

pub fn install(hosts: Rc<dyn Hosts>) {
    HOSTS.with(|held| *held.borrow_mut() = Some(hosts));
}

pub fn get() -> Option<Rc<dyn Hosts>> {
    HOSTS.with(|held| held.borrow().clone())
}

pub fn normalized(address: &str) -> String {
    let bare = address
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    match bare.rsplit_once(':') {
        Some((_, port)) if port.parse::<u16>().is_ok() => bare.to_owned(),
        _ if bare.is_empty() => String::new(),
        _ => format!("{bare}:{PORT}"),
    }
}

#[cfg(feature = "desktop")]
pub mod saved {
    use super::{Host, Hosts, normalized};
    use crate::model::settings;
    use serde::{Deserialize, Serialize};
    use std::{
        collections::HashMap,
        net::{TcpStream, ToSocketAddrs},
        sync::Mutex,
        time::{Duration, Instant},
    };

    const FILE: &str = "hosts.toml";
    const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);
    const PROBE_AGE: Duration = Duration::from_secs(5);

    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    struct File {
        #[serde(default)]
        hosts: Vec<Saved>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct Saved {
        address: String,
        #[serde(default)]
        token: String,
    }

    static PROBES: Mutex<Option<HashMap<String, (bool, Instant)>>> = Mutex::new(None);

    fn read() -> File {
        settings::dir()
            .ok()
            .and_then(|dir| std::fs::read_to_string(dir.join(FILE)).ok())
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn write(file: &File) {
        let Ok(dir) = settings::dir() else {
            return;
        };
        let Ok(text) = toml::to_string(file) else {
            return;
        };
        let path = dir.join(FILE);
        if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&path, text).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }

    fn reachable(address: &str) -> bool {
        address
            .to_socket_addrs()
            .ok()
            .into_iter()
            .flatten()
            .any(|at| TcpStream::connect_timeout(&at, PROBE_TIMEOUT).is_ok())
    }

    fn probed(address: &str) -> Option<bool> {
        let mut probes = PROBES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let probes = probes.get_or_insert_with(HashMap::new);
        let known = probes.get(address).copied();
        if known.is_none_or(|(_, at)| at.elapsed() > PROBE_AGE) {
            let fresh = known.map_or(false, |(was, _)| was);
            probes.insert(address.to_owned(), (fresh, Instant::now()));
            let address = address.to_owned();
            std::thread::spawn(move || {
                let up = reachable(&address);
                let mut probes = PROBES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                probes
                    .get_or_insert_with(HashMap::new)
                    .insert(address, (up, Instant::now()));
            });
        }
        known.map(|(up, _)| up)
    }

    pub struct Desktop;

    impl Hosts for Desktop {
        fn list(&self) -> Vec<Host> {
            read()
                .hosts
                .into_iter()
                .map(|saved| Host {
                    reachable: probed(&saved.address),
                    address: saved.address,
                    current: false,
                })
                .collect()
        }

        fn add(&self, address: &str, token: &str) {
            let address = normalized(address);
            if address.is_empty() {
                return;
            }
            let mut file = read();
            file.hosts.retain(|saved| saved.address != address);
            file.hosts.push(Saved {
                address,
                token: token.trim().to_owned(),
            });
            write(&file);
        }

        fn remove(&self, address: &str) {
            let mut file = read();
            file.hosts.retain(|saved| saved.address != address);
            write(&file);
        }

        fn open(&self, _: &str) {}

        fn connected(&self) -> bool {
            true
        }
    }
}
