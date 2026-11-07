//! All archive names are validated before writing. Directory traversal uses openat
//! with O_NOFOLLOW for every component, including the final file.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    pub max_archive_bytes: u64,
    pub max_total_bytes: u64,
    pub max_file_bytes: u64,
    pub max_entries: usize,
    pub max_depth: usize,
    pub max_ratio: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 8 << 30,
            max_total_bytes: 16 << 30,
            max_file_bytes: 4 << 30,
            max_entries: 200_000,
            max_depth: 64,
            max_ratio: 1000,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        if self.max_archive_bytes == 0
            || self.max_total_bytes == 0
            || self.max_file_bytes == 0
            || self.max_entries == 0
            || self.max_depth == 0
            || self.max_depth > 256
            || self.max_ratio == 0
        {
            return Err(Error::Invalid(
                "Extraction limits must be positive; maximum nesting is 256".into(),
            ));
        }
        Ok(())
    }
}

pub fn archive_path(name: &str, max_depth: usize) -> Result<PathBuf> {
    if name.is_empty()
        || name.len() > 4096
        || name.chars().any(|c| c.is_control())
        || name.contains('\\')
        || name.starts_with('/')
        || name.contains(':')
    {
        return Err(Error::Security(format!("Unsafe archive path: {name:?}")));
    }
    let mut out = PathBuf::new();
    for component in Path::new(name).components() {
        match component {
            Component::Normal(part) => {
                if part.as_bytes().len() > 255 {
                    return Err(Error::Security("Filename exceeds 255 bytes".into()));
                }
                out.push(part);
            }
            Component::CurDir => {}
            _ => return Err(Error::Security(format!("Path traversal: {name:?}"))),
        }
    }
    if out.components().count() > max_depth {
        return Err(Error::Security("Archive nesting limit exceeded".into()));
    }
    Ok(out)
}

pub fn link_target(link: &Path, target: &str, max_depth: usize) -> Result<PathBuf> {
    if target.is_empty()
        || target.starts_with('/')
        || target.contains(['\\', ':'])
        || target.chars().any(|c| c.is_control())
    {
        return Err(Error::Security(format!(
            "Unsafe symbolic link target: {target:?}"
        )));
    }
    let mut result = link.parent().unwrap_or(Path::new("")).to_path_buf();
    for c in Path::new(target).components() {
        match c {
            Component::Normal(p) => result.push(p),
            Component::CurDir => {}
            Component::ParentDir if result.pop() => {}
            _ => {
                return Err(Error::Security(format!(
                    "Symbolic link escapes installation: {target}"
                )))
            }
        }
    }
    if result.components().count() > max_depth {
        return Err(Error::Security("Link nesting limit exceeded".into()));
    }
    Ok(result)
}

fn cstring(path: &std::ffi::OsStr) -> Result<CString> {
    CString::new(path.as_bytes()).map_err(|_| Error::Security("NUL in filesystem path".into()))
}

pub struct SafeRoot(File);
impl SafeRoot {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        Ok(Self(file))
    }
    fn parent(&self, path: &Path, create: bool) -> Result<(File, CString)> {
        let path = archive_path(
            path.to_str()
                .ok_or_else(|| Error::Security("Non-UTF-8 path".into()))?,
            256,
        )?;
        let mut dir = self.0.try_clone()?;
        let components: Vec<_> = path.iter().collect();
        let (last, parents) = components
            .split_last()
            .ok_or_else(|| Error::Security("Empty output path".into()))?;
        for component in parents {
            let name = cstring(component)?;
            if create {
                // SAFETY: valid directory descriptor and NUL-terminated component.
                let rc = unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), 0o755) };
                if rc < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                    return Err(std::io::Error::last_os_error().into());
                }
            }
            // SAFETY: component is not a path; O_NOFOLLOW prevents symlink traversal.
            let fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: openat returned a fresh owned descriptor.
            dir = unsafe { File::from_raw_fd(fd) };
        }
        Ok((dir, cstring(last)?))
    }
    pub fn create_file(&self, path: &Path, executable: bool) -> Result<File> {
        let (dir, name) = self.parent(path, true)?;
        // SAFETY: validated single component and owned directory descriptor.
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                if executable { 0o755 } else { 0o644 },
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    pub fn open_file(&self, path: &Path) -> Result<File> {
        let (dir, name) = self.parent(path, false)?;
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if !file.metadata()?.is_file() {
            return Err(Error::Security("Expected a regular file".into()));
        }
        Ok(file)
    }
    pub fn mkdir(&self, path: &Path) -> Result<()> {
        let (dir, name) = self.parent(path, true)?;
        let rc = unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), 0o755) };
        if rc < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
            return Err(std::io::Error::last_os_error().into());
        }
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        drop(unsafe { File::from_raw_fd(fd) });
        Ok(())
    }
    pub fn symlink(&self, path: &Path, target: &str) -> Result<()> {
        link_target(path, target, 256)?;
        let (dir, name) = self.parent(path, true)?;
        let target = CString::new(target).map_err(|_| Error::Security("NUL in link".into()))?;
        let rc = unsafe { libc::symlinkat(target.as_ptr(), dir.as_raw_fd(), name.as_ptr()) };
        if rc < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    pub fn unlink(&self, path: &Path, directory: bool) -> Result<()> {
        let (dir, name) = self.parent(path, false)?;
        let rc = unsafe {
            libc::unlinkat(
                dir.as_raw_fd(),
                name.as_ptr(),
                if directory { libc::AT_REMOVEDIR } else { 0 },
            )
        };
        if rc < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
}

pub fn digest_reader(mut reader: impl Read) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn digest(path: &Path) -> Result<String> {
    digest_reader(File::open(path)?)
}

pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("Missing parent directory".into()))?;
    std::fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut tmp, value)?;
    tmp.write_all(b"\n")?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| Error::Io(e.error))?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_bad_paths() {
        for path in [
            "../evil",
            "/etc/passwd",
            "a/../../b",
            "C:\\evil",
            "a\\b",
            "a\n.desktop",
        ] {
            assert!(archive_path(path, 64).is_err(), "{path}");
        }
        assert_eq!(
            archive_path("./app/bin/run", 64).unwrap(),
            Path::new("app/bin/run")
        );
        assert!(archive_path("a/b/c", 2).is_err());
    }
    #[test]
    fn links_stay_inside() {
        assert!(link_target(Path::new("app/link"), "../../etc", 64).is_err());
        assert_eq!(
            link_target(Path::new("app/lib/a"), "../bin/b", 64).unwrap(),
            Path::new("app/bin/b")
        );
    }
    #[test]
    fn no_symlink_parent_writes() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
        let safe = SafeRoot::open(root.path()).unwrap();
        assert!(safe.create_file(Path::new("escape/pwned"), false).is_err());
        assert!(!outside.path().join("pwned").exists());
    }
}
