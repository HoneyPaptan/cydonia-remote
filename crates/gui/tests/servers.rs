use cydonia_gui::model::servers;
use std::{
    net::TcpStream,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Site(Child);

impl Site {
    fn start(port: u16, page: &std::path::Path) -> Self {
        let child = Command::new("python3")
            .args(["-m", "http.server", &port.to_string(), "--bind", "127.0.0.1"])
            .current_dir(page)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(started.elapsed() < Duration::from_secs(10), "site never came up");
            std::thread::sleep(Duration::from_millis(50));
        }
        Self(child)
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn network_address() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    Some(socket.local_addr().ok()?.ip()).filter(|ip| !ip.is_loopback())
}

fn page_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cydonia-servers-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("index.html"),
        "<html><head><title>Storefront</title></head><body>hi</body></html>",
    )
    .unwrap();
    dir
}

#[test]
fn a_running_site_is_listed_with_its_title_and_can_be_stopped() {
    let port = free_port();
    let _site = Site::start(port, &page_dir());

    let listed = servers::fresh();
    let server = listed
        .iter()
        .find(|server| server.port == port)
        .expect("the site is listed");
    assert_eq!(server.title.as_deref(), Some("Storefront"));
    assert!(!server.shared);
    assert!(server.folder.is_some());

    servers::stop_now(server.pid, port).unwrap();
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_ok() {
        assert!(started.elapsed() < Duration::from_secs(5), "site still answers");
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(2100));
    assert!(servers::fresh().iter().all(|server| server.port != port));
}

#[test]
fn a_loopback_site_is_reachable_through_a_shared_address() {
    let port = free_port();
    let _site = Site::start(port, &page_dir());
    servers::fresh();

    let Some(address) = network_address() else {
        return;
    };
    servers::expose(port, &[address]).unwrap();

    let mut stream = TcpStream::connect((address, port)).unwrap();
    std::io::Write::write_all(&mut stream, b"GET / HTTP/1.0\r\n\r\n").unwrap();
    let mut body = String::new();
    std::io::Read::read_to_string(&mut stream, &mut body).unwrap();
    assert!(body.contains("Storefront"), "{body}");
}

#[test]
fn a_port_with_no_network_address_cannot_be_shared() {
    assert!(servers::expose(free_port(), &[]).is_err());
}
