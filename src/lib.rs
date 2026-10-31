//! Independent Linux trash storage.
use std::{
    collections::HashSet,
    ffi::{CString, OsStr, OsString},
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

fn err(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, s)
}
fn encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
fn decode(s: &str) -> io::Result<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if i + 2 >= b.len() {
                return Err(err("invalid percent encoding"));
            }
            let hex = std::str::from_utf8(&b[i + 1..i + 3])
                .map_err(|_| err("invalid percent encoding"))?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| err("invalid percent encoding"))?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    if out.contains(&0) {
        return Err(err("NUL in path"));
    }
    Ok(out)
}
fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    let a = CString::new(from.as_os_str().as_bytes()).map_err(|_| err("NUL in path"))?;
    let b = CString::new(to.as_os_str().as_bytes()).map_err(|_| err("NUL in path"))?;
    let rc = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            a.as_ptr(),
            libc::AT_FDCWD,
            b.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
fn mkdir_real(p: &Path) -> io::Result<()> {
    match fs::create_dir(p) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let m = fs::symlink_metadata(p)?;
    if !m.file_type().is_dir() {
        return Err(err("trash directory component is not a real directory"));
    }
    Ok(())
}
fn absolute_source(p: &Path) -> io::Result<PathBuf> {
    let name = p
        .file_name()
        .ok_or_else(|| err("source must have a filename"))?;
    let parent = p
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(fs::canonicalize(parent)?.join(name))
}
fn remove_entry(p: &Path) -> io::Result<()> {
    let m = fs::symlink_metadata(p)?;
    if m.file_type().is_dir() {
        fs::remove_dir_all(p)
    } else {
        fs::remove_file(p)
    }
}

// Match du's default behavior: count allocated blocks, do not follow symlinks,
// and count hard-linked inodes only once per directory tree.
fn directory_size(path: &Path, seen: &mut HashSet<(u64, u64)>) -> io::Result<u64> {
    let meta = fs::symlink_metadata(path)?;
    if !seen.insert((meta.dev(), meta.ino())) {
        return Ok(0);
    }
    let mut size = meta.blocks().checked_mul(512)
        .ok_or_else(|| err("directory size overflow"))?;
    if meta.file_type().is_dir() {
        for entry in fs::read_dir(path)? {
            size = size.checked_add(directory_size(&entry?.path(), seen)?)
                .ok_or_else(|| err("directory size overflow"))?;
        }
    }
    Ok(size)
}

static CACHE_TEMP_ID: AtomicU64 = AtomicU64::new(0);

// Check explicitly selected freedesktop per-user trash locations before creating
// anything beneath them. Arbitrary custom trash directory names remain supported.
fn verify_external_trash(path: &Path) -> io::Result<()> {
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir))
        && path.components().any(|c| c.as_os_str() == ".Trash")
    {
        return Err(err("parent traversal in shared .Trash path"));
    }
    let uid = unsafe { libc::geteuid() };
    let name = path.file_name().unwrap_or_default().as_bytes();
    let parent = path.parent().unwrap_or(Path::new("."));
    let shared = parent.file_name().is_some_and(|n| n == ".Trash");
    let private = name.starts_with(b".Trash-")
        && !name[b".Trash-".len()..].is_empty()
        && name[b".Trash-".len()..].iter().all(u8::is_ascii_digit);
    if !shared && !private {
        return Ok(());
    }
    let expected = uid.to_string();
    if (shared && name != expected.as_bytes())
        || (private && &name[b".Trash-".len()..] != expected.as_bytes())
    {
        return Err(err("trash directory UID does not match current user"));
    }
    if shared {
        let meta = fs::symlink_metadata(parent)?;
        if !meta.file_type().is_dir() || meta.mode() & libc::S_ISVTX == 0 {
            return Err(err("shared .Trash must be a real directory with the sticky bit set"));
        }
    }
    match fs::symlink_metadata(path) {
        Ok(meta) if !meta.file_type().is_dir() || meta.uid() != uid => {
            Err(err("per-user trash must be a real directory owned by the current user"))
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub struct Trash {
    root: PathBuf,
}

pub struct ListResult {
    pub entries: Vec<(String, PathBuf)>,
    pub errors: Vec<(String, io::Error)>,
}
impl Trash {
    // The cache is optional: a failed scan or write must not turn a successful
    // move into an apparent failure. Never publish a partially written cache.
    fn update_directorysizes(&self) -> io::Result<()> {
        let mut lines = Vec::new();
        for entry in fs::read_dir(self.root.join("files"))? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let info = match fs::symlink_metadata(self.info(&name)) {
                Ok(info) if info.file_type().is_file() => info,
                _ => continue,
            };
            let size = match directory_size(&entry.path(), &mut HashSet::new()) {
                Ok(size) => size,
                Err(_) => continue,
            };
            lines.push(format!("{} {} {}\n", size, info.mtime(), encode(name.as_bytes())));
        }
        lines.sort_unstable();
        let (path, mut file) = loop {
            let id = CACHE_TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = self.root.join(format!(".directorysizes.{}.{}", std::process::id(), id));
            match OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path) {
                Ok(file) => break (path, file),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        };
        let result = (|| {
            for line in lines {
                file.write_all(line.as_bytes())?;
            }
            fs::rename(&path, self.root.join("directorysizes"))
        })();
        if result.is_err() {
            let _ = fs::remove_file(path);
        }
        result
    }
    pub fn open(path: &Path) -> io::Result<Self> {
        if path.as_os_str().is_empty() {
            return Err(err("empty trash directory"));
        }
        verify_external_trash(path)?;
        fs::DirBuilder::new().recursive(true).mode(0o700).create(path)?;
        verify_external_trash(path)?;
        let meta = fs::symlink_metadata(path)?;
        if !meta.file_type().is_dir() {
            return Err(err("trash root must be a real directory"));
        }
        let root = fs::canonicalize(path)?;
        if root == Path::new("/") {
            return Err(err("trash root cannot be /"));
        }
        mkdir_real(&root.join("files"))?;
        mkdir_real(&root.join("info"))?;
        Ok(Self { root })
    }
    pub fn put(&self, path: &Path) -> io::Result<String> {
        let src = absolute_source(path)?;
        let meta = fs::symlink_metadata(&src)?;
        if meta.file_type().is_dir() && self.root.starts_with(&src) {
            return Err(err("cannot trash the trash directory or its ancestor"));
        }
        if src.starts_with(&self.root) {
            return Err(err("cannot trash an entry inside the trash directory"));
        }
        let name = src
            .file_name()
            .ok_or_else(|| err("missing filename"))?
            .as_bytes();
        for n in 0u64.. {
            let mut candidate = name.to_vec();
            if n > 0 {
                candidate.extend_from_slice(format!(".{n}").as_bytes());
            }
            let os = OsString::from_vec(candidate.clone());
            let file = self.root.join("files").join(&os);
            let mut info_name = candidate.clone();
            info_name.extend_from_slice(b".trashinfo");
            let info = self.root.join("info").join(OsString::from_vec(info_name));
            let mut f = match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&info)
            {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            };
            let result = (|| {
                write!(
                    f,
                    "[Trash Info]\nPath={}\nDeletionDate={}\n",
                    encode(src.as_os_str().as_bytes()),
                    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S")
                )?;
                f.sync_all()?;
                rename_noreplace(&src, &file)
            })();
            match result {
                Ok(()) => {
                    if meta.file_type().is_dir() {
                        let _ = self.update_directorysizes();
                    }
                    return Ok(format!("trash://{}", encode(&candidate)));
                }
                Err(e) => {
                    fs::remove_file(&info)?;
                    if e.kind() == io::ErrorKind::AlreadyExists {
                        continue;
                    }
                    return Err(e);
                }
            }
        }
        unreachable!()
    }
    fn name(uri: &str) -> io::Result<OsString> {
        let s = uri
            .strip_prefix("trash://")
            .ok_or_else(|| err("invalid trash URI"))?;
        let b = decode(s)?;
        if b.is_empty() || b == b"." || b == b".." || b.contains(&b'/') {
            return Err(err("invalid trash entry name"));
        }
        Ok(OsString::from_vec(b))
    }
    fn info(&self, name: &OsStr) -> PathBuf {
        let mut n = name.to_os_string();
        n.push(".trashinfo");
        self.root.join("info").join(n)
    }
    fn destination(&self, name: &OsStr) -> io::Result<(PathBuf, bool)> {
        let path = self.info(name);
        let m = fs::symlink_metadata(&path)?;
        if !m.file_type().is_file() {
            return Err(err("metadata is not a regular file"));
        }
        let mut opts = OpenOptions::new();
        opts.read(true).custom_flags(libc::O_NOFOLLOW);
        let mut f = opts.open(path)?;
        use std::io::Read;
        let mut s = String::new();
        f.read_to_string(&mut s)?;
        let mut lines = s.lines();
        if lines.next() != Some("[Trash Info]") {
            return Err(err("invalid metadata header"));
        }
        let mut p = None;
        let mut date = None;
        for line in lines {
            if let Some(v) = line.strip_prefix("Path=") {
                p.get_or_insert(v);
            } else if let Some(v) = line.strip_prefix("DeletionDate=") {
                date.get_or_insert(v);
            }
        }
        let raw = decode(p.ok_or_else(|| err("missing Path"))?)?;
        let mut dest = PathBuf::from(OsString::from_vec(raw));
        if dest.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
            || dest.file_name().is_none()
        {
            return Err(err("unsafe destination path"));
        }
        let relative = dest.is_relative();
        if relative {
            dest = self.root.parent().ok_or_else(|| err("invalid trash root"))?.join(dest);
        }
        chrono::NaiveDateTime::parse_from_str(
            date.ok_or_else(|| err("missing DeletionDate"))?,
            "%Y-%m-%dT%H:%M:%S",
        )
        .map_err(|_| err("invalid DeletionDate"))?;
        if dest.starts_with(&self.root) || self.root.starts_with(&dest) {
            return Err(err("destination overlaps trash"));
        }
        Ok((dest, relative))
    }
    pub fn list(&self) -> io::Result<ListResult> {
        let mut out = ListResult { entries: Vec::new(), errors: Vec::new() };
        for item in fs::read_dir(self.root.join("files"))? {
            let item = item?;
            let name = item.file_name();
            let uri = format!("trash://{}", encode(name.as_bytes()));
            match self.destination(&name) {
                Ok((dest, _)) => out.entries.push((uri, dest)),
                Err(error) => out.errors.push((uri, error)),
            }
        }
        out.entries.sort_by(|a, b| a.0.cmp(&b.0));
        out.errors.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }
    pub fn restore(&self, uri: &str) -> io::Result<()> {
        let name = Self::name(uri)?;
        let (dest, relative) = self.destination(&name)?;
        let parent = dest.parent().ok_or_else(|| err("invalid destination"))?;
        // Check existing ancestors before creating directories, so a symlink cannot
        // redirect a relative destination outside the trash's parent directory.
        let base = self.root.parent().ok_or_else(|| err("invalid trash root"))?;
        if relative {
            let relative = parent.strip_prefix(base).map_err(|_| err("unsafe destination path"))?;
            let mut current = base.to_path_buf();
            for component in relative.components() {
                current.push(component);
                match fs::canonicalize(&current) {
                    Ok(actual) if !actual.starts_with(base) => {
                        return Err(err("destination escapes trash parent"));
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => break,
                    Err(e) => return Err(e),
                }
            }
        }
        fs::create_dir_all(parent)?;
        let actual_parent = fs::canonicalize(parent)?;
        if relative && !actual_parent.starts_with(base) {
            return Err(err("destination escapes trash parent"));
        }
        let actual_dest =
            actual_parent.join(dest.file_name().ok_or_else(|| err("invalid destination"))?);
        if actual_dest.starts_with(&self.root) || self.root.starts_with(&actual_dest) {
            return Err(err("destination overlaps trash"));
        }
        rename_noreplace(&self.root.join("files").join(&name), &actual_dest)?;
        let result = fs::remove_file(self.info(&name));
        let _ = self.update_directorysizes();
        result
    }
    pub fn empty(&self) -> io::Result<()> {
        let result = (|| {
            for dir in ["files", "info"] {
                for e in fs::read_dir(self.root.join(dir))? {
                    remove_entry(&e?.path())?;
                }
            }
            Ok(())
        })();
        let _ = self.update_directorysizes();
        result
    }
}
