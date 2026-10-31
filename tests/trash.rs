use rmtrash::Trash;
use std::{
    fs,
    os::unix::{ffi::OsStringExt, fs::{symlink, PermissionsExt}},
    path::PathBuf,
    process::Command,
    sync::Arc,
};
use tempfile::tempdir;

#[test]
fn explicitly_selected_external_trash_is_verified() {
    let t = tempdir().unwrap();
    let uid = unsafe { libc::geteuid() };
    let shared = t.path().join(".Trash");
    let user = shared.join(uid.to_string());
    fs::create_dir(&shared).unwrap();
    assert!(Trash::open(&user).is_err());
    assert!(!user.exists());
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o1777)).unwrap();
    Trash::open(&user).unwrap();
    assert!(Trash::open(&shared.join(uid.to_string()).join("..").join(uid.to_string())).is_err());
    assert_eq!(fs::metadata(&user).unwrap().permissions().mode() & 0o777, 0o700);
    assert!(Trash::open(&shared.join(format!("{}", u64::from(uid) + 1))).is_err());

    let private = t.path().join(format!(".Trash-{uid}"));
    Trash::open(&private).unwrap();
    assert!(Trash::open(&t.path().join(format!(".Trash-{}", u64::from(uid) + 1))).is_err());
    let other = tempdir().unwrap();
    symlink(&shared, other.path().join(".Trash")).unwrap();
    assert!(Trash::open(&other.path().join(".Trash").join(uid.to_string())).is_err());
    let other_private = other.path().join(format!(".Trash-{uid}"));
    symlink(&private, &other_private).unwrap();
    assert!(Trash::open(&other_private).is_err());
}

#[test]
fn relative_metadata_paths_list_and_restore() {
    let t = tempdir().unwrap();
    let base = t.path().join("data");
    let root = base.join("Trash");
    let trash = Trash::open(&root).unwrap();
    fs::write(root.join("files/item"), b"content").unwrap();
    let info = root.join("info/item.trashinfo");
    fs::write(&info, "[Trash Info]\nPath=documents/a%20%FF\nDeletionDate=2026-01-01T00:00:00\n").unwrap();
    let destination = base.join("documents").join(std::ffi::OsString::from_vec(b"a \xff".to_vec()));
    assert_eq!(trash.list().unwrap().entries, vec![("trash://item".into(), destination.clone())]);
    trash.restore("trash://item").unwrap();
    assert_eq!(fs::read(destination).unwrap(), b"content");
}

#[test]
fn relative_metadata_rejects_escape_and_trash_overlap() {
    let t = tempdir().unwrap();
    let base = t.path().join("data");
    let root = base.join("custom-trash");
    let trash = Trash::open(&root).unwrap();
    fs::write(root.join("files/item"), b"content").unwrap();
    let info = root.join("info/item.trashinfo");
    for path in ["../outside", "%2E%2E/outside", "custom-trash/files/other", ""] {
        fs::write(&info, format!("[Trash Info]\nPath={path}\nDeletionDate=2026-01-01T00:00:00\n")).unwrap();
        assert_eq!(trash.list().unwrap().errors.len(), 1, "{path}");
        assert!(trash.restore("trash://item").is_err(), "{path}");
    }
    let outside = t.path().join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, base.join("link")).unwrap();
    fs::write(&info, "[Trash Info]\nPath=link/new/item\nDeletionDate=2026-01-01T00:00:00\n").unwrap();
    assert!(trash.restore("trash://item").is_err());
    assert!(!outside.join("new").exists());
    assert!(root.join("files/item").exists());
}

#[test]
fn duplicate_metadata_keys_use_first_values() {
    let t = tempdir().unwrap();
    let root = t.path().join("trash");
    let trash = Trash::open(&root).unwrap();
    let file = t.path().join("file");
    fs::write(&file, "content").unwrap();
    let uri = trash.put(&file).unwrap();
    let info = root.join("info/file.trashinfo");
    let original = fs::read_to_string(&info).unwrap();
    fs::write(
        &info,
        format!("{original}Path=/../../evil\nDeletionDate=invalid\n"),
    )
    .unwrap();

    assert_eq!(trash.list().unwrap().entries, vec![(uri.clone(), file.clone())]);
    trash.restore(&uri).unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"content");

    let uri = trash.put(&file).unwrap();
    let original = fs::read_to_string(&info).unwrap();
    for invalid in ["Path=/../../evil\n", "DeletionDate=invalid\n"] {
        fs::write(
            &info,
            format!(
                "[Trash Info]\n{invalid}{}",
                original.strip_prefix("[Trash Info]\n").unwrap()
            ),
        )
        .unwrap();
        assert_eq!(trash.list().unwrap().errors.len(), 1);
        assert!(trash.restore(&uri).is_err());
    }
}

#[test]
fn round_trip_files_dirs_symlinks_and_bytes() {
    let t = tempdir().unwrap();
    let trash = Trash::open(&t.path().join("trash")).unwrap();
    let name = std::ffi::OsString::from_vec(b"a %\xff".to_vec());
    let file = t.path().join(name);
    fs::write(&file, b"hello").unwrap();
    let uri = trash.put(&file).unwrap();
    assert!(uri.contains("%FF"));
    assert_eq!(trash.list().unwrap().entries[0].1, file);
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
fn directorysizes_tracks_directories_and_removes_stale_entries() {
    use std::os::unix::fs::MetadataExt;
    let t = tempdir().unwrap();
    let root = t.path().join("trash");
    let trash = Trash::open(&root).unwrap();
    let name = std::ffi::OsString::from_vec(b"dir %\xff".to_vec());
    let dir = t.path().join(&name);
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("file"), vec![1; 8192]).unwrap();
    fs::hard_link(dir.join("file"), dir.join("hardlink")).unwrap();
    symlink("file", dir.join("link")).unwrap();
    let size: u64 = [dir.clone(), dir.join("file"), dir.join("link")]
        .iter().map(|p| fs::symlink_metadata(p).unwrap().blocks() * 512).sum();
    let uri = trash.put(&dir).unwrap();
    let info = root.join("info").join({ let mut n = name.clone(); n.push(".trashinfo"); n });
    let expected = format!("{} {} dir%20%25%FF\n", size, fs::metadata(info).unwrap().mtime());
    assert_eq!(fs::read_to_string(root.join("directorysizes")).unwrap(), expected);
    trash.restore(&uri).unwrap();
    assert_eq!(fs::read_to_string(root.join("directorysizes")).unwrap(), "");
    trash.put(&dir).unwrap();
    fs::write(root.join("directorysizes"), "123 456 stale\n").unwrap();
    trash.empty().unwrap();
    assert_eq!(fs::read_to_string(root.join("directorysizes")).unwrap(), "");
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
    assert_eq!(trash.list().unwrap().entries.len(), 8);
    let dest = trash
        .list()
        .unwrap()
        .entries
        .into_iter()
        .find(|x| x.0 == uris[0])
        .unwrap()
        .1;
    fs::write(&dest, "conflict").unwrap();
    assert!(trash.restore(&uris[0]).is_err());
    assert_eq!(fs::read_to_string(dest).unwrap(), "conflict");
    assert_eq!(trash.list().unwrap().entries.len(), 8);
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

    let trash_root = t.path().join("trash");
    let file = t.path().join("file");
    fs::write(&file, "data").unwrap();
    let output = Command::new(bin)
        .arg("--trash-dir")
        .arg(&trash_root)
        .arg("--force")
        .arg(&trash_root)
        .arg(&file)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot trash"));
    assert!(!file.exists(), "other inputs should still be processed");
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
    assert!(trash.list().unwrap().entries.is_empty());
}

#[test]
fn list_reports_bad_metadata_and_keeps_valid_entries() {
    let t = tempdir().unwrap();
    let root = t.path().join("trash");
    let trash = Trash::open(&root).unwrap();
    let file = t.path().join("good");
    fs::write(&file, "content").unwrap();
    let uri = trash.put(&file).unwrap();
    fs::write(root.join("files/missing"), "data").unwrap();
    fs::write(root.join("files/invalid"), "data").unwrap();
    fs::write(root.join("info/invalid.trashinfo"), "invalid").unwrap();

    let result = trash.list().unwrap();
    assert_eq!(result.entries, vec![(uri, file.clone())]);
    assert_eq!(result.errors.len(), 2);
    assert_eq!(result.errors[0].0, "trash://invalid");
    assert_eq!(result.errors[1].0, "trash://missing");
    assert_eq!(result.errors[1].1.kind(), std::io::ErrorKind::NotFound);

    let output = Command::new(env!("CARGO_BIN_EXE_rmtrash"))
        .arg("--trash-dir").arg(&root).arg("--list").output().unwrap();
    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), format!("trash://good\t{}\n", file.display()));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("trash://missing: cannot read metadata"));
    assert!(stderr.contains("trash://invalid: cannot read metadata: invalid metadata header"));
}
