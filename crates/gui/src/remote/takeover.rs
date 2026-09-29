use anyhow::{Context as _, Result};
use std::{
    io::{Read as _, Write as _},
    net::{SocketAddr, TcpStream},
    time::Duration,
};
use tokio::{net::TcpListener, runtime::Runtime};

const PROBE: Duration = Duration::from_millis(400);
const RETRIES: usize = 40;
const RETRY_EVERY: Duration = Duration::from_millis(250);

pub fn occupied(listen: &[SocketAddr]) -> bool {
    listen
        .iter()
        .any(|at| TcpStream::connect_timeout(at, PROBE).is_ok())
}

fn ask_to_leave(at: &SocketAddr, token: &str) -> Result<()> {
    let mut stream = TcpStream::connect_timeout(at, PROBE).context("nobody answers there")?;
    stream.set_read_timeout(Some(PROBE * 4))?;
    write!(
        stream,
        "POST /v1/handover HTTP/1.1\r\nHost: {at}\r\nAuthorization: Bearer {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )?;
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    Ok(())
}

pub fn bind(runtime: &Runtime, listen: &[SocketAddr], token: &str) -> Result<Vec<TcpListener>> {
    let error = match runtime.block_on(remote::server::bind(listen)) {
        Ok(listeners) => return Ok(listeners),
        Err(error) => error,
    };
    if error.kind() != std::io::ErrorKind::AddrInUse {
        return Err(error.into());
    }
    for at in listen {
        let _ = ask_to_leave(at, token);
    }
    for _ in 0..RETRIES {
        std::thread::sleep(RETRY_EVERY);
        if let Ok(listeners) = runtime.block_on(remote::server::bind(listen)) {
            return Ok(listeners);
        }
    }
    Err(error.into())
}
