use super::relay::Pending;
use remote::proto::LocalServer;

#[cfg(not(feature = "desktop"))]
pub fn list() -> Result<Vec<LocalServer>, Pending> {
    use remote::proto::{Answer, Query};
    match super::relay::ask(Query::Servers) {
        Ok(Answer::Servers { servers }) => Ok(servers),
        Ok(_) => Ok(Vec::new()),
        Err(pending) => Err(pending),
    }
}

#[cfg(not(feature = "desktop"))]
pub fn stop(server: &LocalServer) {
    super::relay::tell(remote::proto::Query::Stop {
        pid: server.pid,
        port: server.port,
    });
}

#[cfg(feature = "desktop")]
pub fn list() -> Result<Vec<LocalServer>, Pending> {
    scan::seen().ok_or(Pending)
}

#[cfg(feature = "desktop")]
pub fn stop(server: &LocalServer) {
    let (pid, port) = (server.pid, server.port);
    std::thread::spawn(move || {
        let _ = scan::stop(pid, port);
    });
}

#[cfg(feature = "desktop")]
pub use scan::{expose, fresh, stop as stop_now};

#[cfg(feature = "desktop")]
mod scan {
    use crate::model::link;
    use remote::proto::LocalServer;
    use std::{
        collections::{HashMap, HashSet},
        io::Read as _,
        net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream},
        process::Command,
        sync::{
            Arc, Mutex, MutexGuard,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };
    use url::Url;

    const FRESH: Duration = Duration::from_millis(2000);
    const PROBE_FRESH: Duration = Duration::from_secs(15);
    const PROBE_TIMEOUT: Duration = Duration::from_millis(800);
    const PAGE_LIMIT: u64 = 64 * 1024;
    const GRACE: Duration = Duration::from_secs(3);
    const FORCED: Duration = Duration::from_secs(1);
    const CHECK_EVERY: Duration = Duration::from_millis(100);
    const LOOPBACK: [&str; 2] = ["127.0.0.1", "[::1]"];

    struct Listener {
        pid: u32,
        process: String,
        host: String,
        port: u16,
    }

    impl Listener {
        fn shared(&self) -> bool {
            !LOOPBACK.contains(&self.host.as_str()) && !self.host.starts_with("127.")
        }

        fn target(&self) -> &str {
            match self.host.as_str() {
                "*" => LOOPBACK[0],
                host => host,
            }
        }
    }

    #[derive(Default)]
    struct Cache {
        at: Option<Instant>,
        servers: Option<Vec<LocalServer>>,
    }

    static CACHE: Mutex<Cache> = Mutex::new(Cache {
        at: None,
        servers: None,
    });
    static REFRESHING: AtomicBool = AtomicBool::new(false);
    static PROBES: Mutex<Option<HashMap<(u32, u16), (Instant, Option<Option<String>>)>>> =
        Mutex::new(None);
    static FORWARDS: Mutex<Option<HashMap<u16, Arc<AtomicBool>>>> = Mutex::new(None);

    fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn parse(raw: &str) -> Vec<Listener> {
        let (mut pid, mut process) = (None, String::new());
        let mut found = Vec::new();
        for line in raw.lines() {
            let (tag, value) = line.split_at(line.len().min(1));
            match tag {
                "p" => pid = value.parse().ok(),
                "c" => process = value.to_owned(),
                "n" => {
                    let Some(pid) = pid else { continue };
                    let Some((host, port)) = value.rsplit_once(':') else {
                        continue;
                    };
                    let Ok(port) = port.parse() else { continue };
                    found.push(Listener {
                        pid,
                        process: process.clone(),
                        host: host.to_owned(),
                        port,
                    });
                }
                _ => {}
            }
        }
        found
    }

    fn listeners() -> Vec<Listener> {
        let own = std::process::id();
        let uid = unsafe { libc::geteuid() }.to_string();
        let output = Command::new("lsof")
            .args(["-a", "-u", &uid, "-iTCP", "-sTCP:LISTEN", "-nP", "-F", "pcn"])
            .output();
        let Ok(output) = output else {
            return Vec::new();
        };
        parse(&String::from_utf8_lossy(&output.stdout))
            .into_iter()
            .filter(|listener| listener.pid != own)
            .collect()
    }

    fn folder(pid: u32) -> Option<String> {
        let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).ok()?;
        Some(cwd.file_name()?.to_string_lossy().into_owned())
    }

    fn title_of(host: &str, port: u16) -> Option<Option<String>> {
        let url = Url::parse(&format!("http://{host}:{port}/")).ok()?;
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(PROBE_TIMEOUT))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .new_agent();
        let mut response = agent.get(url.as_str()).call().ok()?;
        let html = response
            .headers()
            .get("content-type")
            .and_then(|kind| kind.to_str().ok())
            .is_some_and(|kind| kind.contains("html"));
        if !html {
            return Some(None);
        }
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(PAGE_LIMIT)
            .read_to_end(&mut bytes)
            .ok()?;
        let title = link::parse(&String::from_utf8_lossy(&bytes), &url).title;
        Some(title.map(|title| title.trim().to_owned()).filter(|title| !title.is_empty()))
    }

    fn probed(listener: &Listener) -> Option<Option<String>> {
        let key = (listener.pid, listener.port);
        if let Some((at, result)) = locked(&PROBES).get_or_insert_default().get(&key)
            && at.elapsed() < PROBE_FRESH
        {
            return result.clone();
        }
        let result = title_of(listener.target(), listener.port);
        locked(&PROBES)
            .get_or_insert_default()
            .insert(key, (Instant::now(), result.clone()));
        result
    }

    fn serving(listeners: Vec<Listener>) -> Vec<LocalServer> {
        let mut by_port: HashMap<u16, Vec<Listener>> = HashMap::new();
        for listener in listeners {
            by_port.entry(listener.port).or_default().push(listener);
        }
        let mut servers: Vec<LocalServer> = thread::scope(|scope| {
            let probes: Vec<_> = by_port
                .into_values()
                .map(|group| {
                    scope.spawn(move || {
                        let shared = group.iter().any(Listener::shared);
                        let first = group.iter().min_by_key(|listener| listener.pid)?;
                        let title = probed(first)?;
                        Some(LocalServer {
                            port: first.port,
                            pid: first.pid,
                            process: first.process.clone(),
                            folder: folder(first.pid),
                            title,
                            shared,
                        })
                    })
                })
                .collect();
            probes
                .into_iter()
                .filter_map(|probe| probe.join().ok().flatten())
                .collect()
        });
        servers.sort_by_key(|server| server.port);
        servers
    }

    fn retire_forwards(listening: &HashSet<u16>) {
        let mut forwards = locked(&FORWARDS);
        let Some(forwards) = forwards.as_mut() else {
            return;
        };
        forwards.retain(|port, stop| {
            let alive = listening.contains(port);
            if !alive {
                stop.store(true, Ordering::Relaxed);
            }
            alive
        });
    }

    fn scanned() -> Vec<LocalServer> {
        let all = listeners();
        retire_forwards(&all.iter().map(|listener| listener.port).collect());
        serving(all)
    }

    pub fn fresh() -> Vec<LocalServer> {
        let mut cache = locked(&CACHE);
        if let (Some(at), Some(servers)) = (cache.at, &cache.servers)
            && at.elapsed() < FRESH
        {
            return servers.clone();
        }
        let servers = scanned();
        cache.at = Some(Instant::now());
        cache.servers = Some(servers.clone());
        servers
    }

    pub fn seen() -> Option<Vec<LocalServer>> {
        let (stale, servers) = {
            let cache = locked(&CACHE);
            let stale = cache.at.is_none_or(|at| at.elapsed() >= FRESH);
            (stale, cache.servers.clone())
        };
        if stale && !REFRESHING.swap(true, Ordering::AcqRel) {
            thread::spawn(|| {
                fresh();
                REFRESHING.store(false, Ordering::Release);
            });
        }
        servers
    }

    fn invalidate() {
        locked(&CACHE).at = None;
    }

    fn listening(pid: u32, port: u16) -> bool {
        listeners()
            .iter()
            .any(|listener| listener.pid == pid && listener.port == port)
    }

    fn signal(pid: u32, signal: libc::c_int) {
        unsafe { libc::kill(pid as libc::pid_t, signal) };
    }

    fn gone(pid: u32, port: u16, within: Duration) -> bool {
        let started = Instant::now();
        while started.elapsed() < within {
            if !listening(pid, port) {
                return true;
            }
            thread::sleep(CHECK_EVERY);
        }
        !listening(pid, port)
    }

    pub fn stop(pid: u32, port: u16) -> Result<(), String> {
        if pid <= 1 || pid == std::process::id() {
            return Err("That one is not stoppable".to_owned());
        }
        if !fresh()
            .iter()
            .any(|server| server.pid == pid && server.port == port)
        {
            return Err("That is not one of the listed servers".to_owned());
        }
        if !listening(pid, port) {
            invalidate();
            return Ok(());
        }
        signal(pid, libc::SIGTERM);
        if !gone(pid, port, GRACE) {
            signal(pid, libc::SIGKILL);
            if !gone(pid, port, FORCED) {
                return Err("It did not stop".to_owned());
            }
        }
        invalidate();
        Ok(())
    }

    fn pipe(mut from: TcpStream, mut to: TcpStream) {
        let _ = std::io::copy(&mut from, &mut to);
        let _ = to.shutdown(Shutdown::Write);
    }

    fn relay(client: TcpStream, port: u16) {
        let targets = [
            SocketAddr::from(([127, 0, 0, 1], port)),
            SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port)),
        ];
        let Ok(server) = TcpStream::connect(&targets[..]) else {
            return;
        };
        let (Ok(client_out), Ok(server_out)) = (client.try_clone(), server.try_clone()) else {
            return;
        };
        thread::spawn(move || pipe(client_out, server_out));
        pipe(server, client);
    }

    fn forward(listener: TcpListener, port: u16, stop: Arc<AtomicBool>) {
        let _ = listener.set_nonblocking(true);
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((client, _)) => {
                    let _ = client.set_nonblocking(false);
                    thread::spawn(move || relay(client, port));
                }
                Err(_) => thread::sleep(CHECK_EVERY),
            }
        }
    }

    pub fn expose(port: u16, hosts: &[IpAddr]) -> Result<(), String> {
        if !fresh().iter().any(|server| server.port == port) {
            return Err("That is not one of the listed servers".to_owned());
        }
        let mut forwards = locked(&FORWARDS);
        let forwards = forwards.get_or_insert_default();
        if forwards.contains_key(&port) {
            return Ok(());
        }
        let hosts: Vec<&IpAddr> = hosts
            .iter()
            .filter(|host| !host.is_loopback() && !host.is_unspecified())
            .collect();
        if hosts.is_empty() {
            return Err("The laptop is not listening on a network address".to_owned());
        }
        let bound = hosts
            .into_iter()
            .map(|host| {
                TcpListener::bind(SocketAddr::new(*host, port))
                    .map_err(|error| format!("Could not share port {port}: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let stop = Arc::new(AtomicBool::new(false));
        for listener in bound {
            let stop = stop.clone();
            thread::spawn(move || forward(listener, port, stop));
        }
        forwards.insert(port, stop);
        Ok(())
    }
}
