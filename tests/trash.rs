use rmtrash::Trash;
use std::{
    fs,
    os::unix::{ffi::OsStringExt, fs::symlink},
    path::PathBuf,
    process::Command,
    sync::Arc,
};
use tempfile::tempdir;

#[test]
fn round_trip_files_dirs_symlinks_and_bytes() {
    let t = tempdir().unwrap();
    let trash = Trash::open(&t.path().join("trash")).unwrap();
    let name = std::ffi::OsString::from_vec(b"a %\xff".to_vec());
    let file = t.path().join(name);
    fs::write(&file, b"hello").unwrap();
    let uri = trash.put(&file).unwrap();
    assert!(uri.contains("%FF"));
    assert_eq!(trash.list().unwrap()[0].1, file);
    trash.restore(&uri).unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"hello");
    let dir = t.path().join("dir");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("x"), b"x").unwrap();
    let uri = trash.put(&dir).unwrap();
    trash.restore(&uri).unwrap();
    assert!(dir.join("x").exists());
    let link = t.path().join("link");
    symlink("missing", &link).unwrap();
    let uri = trash.put(&link).unwrap();
    trash.restore(&uri).unwrap();
    assert_eq!(fs::read_link(link).unwrap(), PathBuf::from("missing"));
}
#[test]
fn collisions_concurrency_and_conflicts() {
    let t = tempdir().unwrap();
    let trash = Arc::new(Trash::open(&t.path().join("trash")).unwrap());
    let mut threads = vec![];
    for i in 0..8 {
        let parent = t.path().join(i.to_string());
        fs::create_dir(&parent).unwrap();
        let file = parent.join("same");
        fs::write(&file, i.to_string()).unwrap();
        let tr = trash.clone();
        threads.push(std::thread::spawn(move || tr.put(&file).unwrap()));
    }
    let uris: Vec<_> = threads.into_iter().map(|x| x.join().unwrap()).collect();
    assert_eq!(trash.list().unwrap().len(), 8);
    let dest = trash
        .list()
        .unwrap()
        .into_iter()
        .find(|x| x.0 == uris[0])
        .unwrap()
        .1;
    fs::write(&dest, "conflict").unwrap();
    assert!(trash.restore(&uris[0]).is_err());
    assert_eq!(fs::read_to_string(dest).unwrap(), "conflict");
    assert_eq!(trash.list().unwrap().len(), 8);
}
#[test]
fn parents_metadata_empty_and_safety() {
    let t = tempdir().unwrap();
    let root = t.path().join("trash");
    let trash = Trash::open(&root).unwrap();
    let parent = t.path().join("parent");
    fs::create_dir(&parent).unwrap();
    let file = parent.join("file");
    fs::write(&file, "a").unwrap();
    let uri = trash.put(&file).unwrap();
    let info = root.join("info/file.trashinfo");
    let text = fs::read_to_string(&info).unwrap();
    assert!(text.starts_with("[Trash Info]\nPath=/"));
    assert!(text.contains("\nDeletionDate="));
    fs::remove_dir(&parent).unwrap();
    trash.restore(&uri).unwrap();
    assert!(file.exists());
    let uri = trash.put(&file).unwrap();
    fs::write(
        &info,
        "[Trash Info]\nPath=/../../evil\nDeletionDate=2026-01-01T00:00:00\n",
    )
    .unwrap();
    assert!(trash.restore(&uri).is_err());
    assert!(root.join("files/file").exists());
    assert!(trash.put(t.path()).is_err());
    let outside = t.path().join("outside");
    fs::write(&outside, "safe").unwrap();
    trash.empty().unwrap();
    assert_eq!(fs::read_to_string(outside).unwrap(), "safe");
}
#[test]
fn cli_force_and_missing_configuration() {
    let t = tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_rmtrash");
    assert!(!Command::new(bin)
        .env_remove("RMTRASH_DIR")
        .env_remove("XDG_DATA_HOME")
        .env_remove("HOME")
        .arg("--list")
        .status()
        .unwrap()
        .success());
    let missing = t.path().join("missing");
    assert!(!Command::new(bin)
        .arg("--trash-dir")
        .arg(t.path().join("trash"))
        .arg(&missing)
        .status()
        .unwrap()
        .success());
    assert!(Command::new(bin)
        .arg("--trash-dir")
        .arg(t.path().join("trash"))
        .arg("-f")
        .arg(missing)
        .status()
        .unwrap()
        .success());
}
#[test]
fn cli_trash_directory_precedence_and_defaults() {
    for (explicit, configured, xdg, home, expected) in [
        (true, true, Some("xdg"), true, "explicit"),
        (false, true, Some("xdg"), true, "configured"),
        (false, false, Some("xdg"), true, "xdg/Trash"),
        (false, false, Some("xdg"), false, "xdg/Trash"),
        (false, false, None, true, "home/.local/share/Trash"),
        (false, false, Some(""), true, "home/.local/share/Trash"),
        (
            false,
            false,
            Some("relative"),
            true,
            "home/.local/share/Trash",
        ),
        (true, false, None, false, "explicit"),
        (false, true, None, false, "configured"),
    ] {
        let t = tempdir().unwrap();
        let file = t.path().join("file");
        fs::write(&file, "content").unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rmtrash"));
        cmd.current_dir(t.path())
            .env_remove("RMTRASH_DIR")
            .env_remove("XDG_DATA_HOME")
            .env_remove("HOME");
        if explicit {
            cmd.arg("--trash-dir").arg(t.path().join("explicit"));
        }
        if configured {
            cmd.env("RMTRASH_DIR", t.path().join("configured"));
        }
        if let Some(xdg) = xdg {
            let path = if xdg == "xdg" {
                t.path().join(xdg)
            } else {
                PathBuf::from(xdg)
            };
            cmd.env("XDG_DATA_HOME", path);
        }
        if home {
            cmd.env("HOME", t.path().join("home"));
        }
        let output = cmd.arg(&file).output().unwrap();
        assert!(output.status.success(), "{expected}: {:?}", output);
        let root = t.path().join(expected);
        assert_eq!(fs::read(root.join("files/file")).unwrap(), b"content");
        assert!(root.join("info/file.trashinfo").is_file());
        assert!(!file.exists());
    }
}
#[test]
fn cross_device_keeps_source_when_available() {
    let t = tempdir().unwrap();
    let other = PathBuf::from("/dev/shm");
    if !other.is_dir() {
        return;
    }
    use std::os::unix::fs::MetadataExt;
    if fs::metadata(t.path()).unwrap().dev() == fs::metadata(&other).unwrap().dev() {
        return;
    }
    let external = tempfile::Builder::new()
        .prefix("rmtrash-test-")
        .tempdir_in(other)
        .unwrap();
    let src = external.path().join("file");
    fs::write(&src, "safe").unwrap();
    let trash = Trash::open(&t.path().join("trash")).unwrap();
    assert!(trash.put(&src).is_err());
    assert_eq!(fs::read_to_string(src).unwrap(), "safe");
    assert!(trash.list().unwrap().is_empty());
}
