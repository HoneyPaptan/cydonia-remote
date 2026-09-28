use cydonia_gui::remote::token;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cydonia-token-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("remote-token")
}

#[test]
fn first_start_creates_a_private_token_and_later_starts_reuse_it() {
    let path = scratch("reuse");
    let first = token(&path).unwrap();
    assert_eq!(first.len(), 64);
    assert_eq!(token(&path).unwrap(), first);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn a_token_written_by_hand_is_used_as_is() {
    let path = scratch("hand");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "chosen-token\n").unwrap();
    assert_eq!(token(&path).unwrap(), "chosen-token");
}

#[test]
fn an_empty_token_file_is_refused() {
    let path = scratch("empty");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "  \n").unwrap();
    assert!(token(&path).is_err());
}
