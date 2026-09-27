//! Writing a file whole, or not at all.
//!
//! Every entry a project holds is read back by listing a directory and parsing
//! what is in it, and a file caught halfway through that write parses as
//! nothing. The listing drops it, so a torn write does not fail loudly once —
//! it deletes the entry. Writing beside the file and renaming over it is what
//! makes a reader see the old body or the new one and never a third thing.

use anyhow::{Context as _, Result};
use std::path::Path;

/// Write `body` to `path` so a reader never sees a partial file.
///
/// The temporary is a sibling rather than somewhere under a temp directory,
/// because the rename has to stay inside one filesystem to be atomic, and
/// because a reader listing this directory is the thing being protected: the
/// name carries `.tmp` and no entry reader claims it.
pub fn write(path: &Path, body: &[u8]) -> Result<()> {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(anyhow::anyhow!(
            "{} has no file name to write beside",
            path.display()
        ));
    };
    let beside = path.with_file_name(format!("{name}.tmp"));
    let written = std::fs::write(&beside, body)
        .with_context(|| format!("writing {}", beside.display()))
        .and_then(|()| {
            // The rename is the atomic step, and it is only ordered against
            // other writes if the body reached the disk before it.
            Ok(std::fs::File::open(&beside)?.sync_all()?)
        });
    if let Err(err) = written {
        let _ = std::fs::remove_file(&beside);
        return Err(err).with_context(|| format!("writing {}", beside.display()));
    }
    if let Err(err) = std::fs::rename(&beside, path) {
        let _ = std::fs::remove_file(&beside);
        return Err(err).with_context(|| format!("renaming into {}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("cydonia-atomic-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn write_replaces_a_body_and_leaves_no_temporary_behind() {
        let dir = scratch("replace");
        let target = dir.join("entry.json");
        write(&target, b"first").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "first");
        write(&target, b"second").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "second");
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["entry.json"], "the temporary outlived the rename");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_body_larger_than_a_read_sees_whole() {
        let dir = scratch("large");
        let target = dir.join("big.json");
        let body = "x".repeat(512 * 1024);
        write(&target, body.as_bytes()).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap().len(), body.len());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
