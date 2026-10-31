//! Independent Linux trash storage.
use std::{
    ffi::{CString, OsStr, OsString},
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::OpenOptionsExt,
    },
    path::{Path, PathBuf},
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

pub struct Trash {
    root: PathBuf,
}

pub struct ListResult {
    pub entries: Vec<(String, PathBuf)>,
    pub errors: Vec<(String, io::Error)>,
}
impl Trash {
    pub fn open(path: &Path) -> io::Result<Self> {
        if path.as_os_str().is_empty() {
            return Err(err("empty trash directory"));
        }
        fs::create_dir_all(path)?;
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
                Ok(()) => return Ok(format!("trash://{}", encode(&candidate))),
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
        fs::remove_file(self.info(&name))
    }
    pub fn empty(&self) -> io::Result<()> {
        for dir in ["files", "info"] {
            for e in fs::read_dir(self.root.join(dir))? {
                remove_entry(&e?.path())?;
            }
        }
        Ok(())
    }
}
