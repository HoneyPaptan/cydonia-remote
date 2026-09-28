use anyhow::{Context as _, Result};
use std::{fs, io::Write as _, path::Path};

const BYTES: usize = 32;

fn fresh() -> Result<String> {
    let mut bytes = [0u8; BYTES];
    getrandom::fill(&mut bytes).map_err(|error| anyhow::anyhow!("no randomness: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn create(path: &Path, token: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.write_all(token.as_bytes())?;
    Ok(())
}

pub fn load_or_create(path: &Path) -> Result<String> {
    if let Ok(text) = fs::read_to_string(path) {
        let token = text.trim();
        anyhow::ensure!(!token.is_empty(), "{} is empty", path.display());
        return Ok(token.to_owned());
    }
    let token = fresh()?;
    create(path, &token)?;
    Ok(token)
}
