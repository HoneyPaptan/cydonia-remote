use anyhow::{Context as _, Result, bail};
use gui::{model::settings, remote::Options};
use std::{net::SocketAddr, path::PathBuf};

const DEFAULT_LISTEN: &str = "127.0.0.1:7878";
const TOKEN_FILE: &str = "remote-token";

pub fn options(args: &[String]) -> Result<Option<Options>> {
    if !args.iter().any(|arg| arg == "--remote") {
        return Ok(None);
    }
    let mut listen = Vec::new();
    let mut ui = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--listen" => {
                let value = rest.next().context("--listen needs an address")?;
                listen.push(
                    value
                        .parse::<SocketAddr>()
                        .with_context(|| format!("bad address {value}"))?,
                );
            }
            "--remote-ui" => {
                ui = Some(PathBuf::from(
                    rest.next().context("--remote-ui needs a directory")?,
                ));
            }
            _ => {}
        }
    }
    if listen.is_empty() {
        listen.push(DEFAULT_LISTEN.parse()?);
    }
    if let Some(dir) = &ui
        && !dir.join("index.html").is_file()
    {
        bail!("{} has no index.html", dir.display());
    }
    let token = gui::remote::token(&settings::dir()?.join(TOKEN_FILE))?;
    Ok(Some(Options { listen, token, ui }))
}
