//! Streaming libarchive reader. We deliberately do not use archive_write_disk:
//! extraction policy and all filesystem writes belong to our own security layer.
use crate::{
    security::{self, Limits, SafeRoot},
    Cancellation, Error, Progress, Result,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    ffi::{CStr, CString},
    fs::File,
    io::{Read, Write},
    os::{
        raw::{c_char, c_int, c_void},
        unix::ffi::OsStrExt,
    },
    path::{Path, PathBuf},
    ptr,
};

#[allow(non_camel_case_types)]
type archive = c_void;
#[allow(non_camel_case_types)]
type archive_entry = c_void;
#[link(name = "archive")]
unsafe extern "C" {
    fn archive_read_new() -> *mut archive;
    fn archive_read_support_filter_all(a: *mut archive) -> c_int;
    fn archive_read_support_format_all(a: *mut archive) -> c_int;
    fn archive_read_open_fd(a: *mut archive, fd: c_int, block: usize) -> c_int;
    fn archive_read_next_header(a: *mut archive, e: *mut *mut archive_entry) -> c_int;
    fn archive_read_data(a: *mut archive, data: *mut c_void, size: usize) -> isize;
    fn archive_read_free(a: *mut archive) -> c_int;
    fn archive_error_string(a: *mut archive) -> *const c_char;
    fn archive_entry_pathname_utf8(e: *mut archive_entry) -> *const c_char;
    fn archive_entry_filetype(e: *mut archive_entry) -> u32;
    fn archive_entry_perm(e: *mut archive_entry) -> u32;
    fn archive_entry_size(e: *mut archive_entry) -> i64;
    fn archive_entry_symlink_utf8(e: *mut archive_entry) -> *const c_char;
    fn archive_entry_hardlink_utf8(e: *mut archive_entry) -> *const c_char;
    fn archive_format_name(a: *mut archive) -> *const c_char;
    fn archive_entry_is_encrypted(e: *mut archive_entry) -> c_int;
    fn archive_write_new() -> *mut archive;
    fn archive_write_set_format_pax_restricted(a: *mut archive) -> c_int;
    fn archive_write_set_format_zip(a: *mut archive) -> c_int;
    fn archive_write_set_format_7zip(a: *mut archive) -> c_int;
    fn archive_write_add_filter_gzip(a: *mut archive) -> c_int;
    fn archive_write_add_filter_xz(a: *mut archive) -> c_int;
    fn archive_write_add_filter_bzip2(a: *mut archive) -> c_int;
    fn archive_write_add_filter_zstd(a: *mut archive) -> c_int;
    fn archive_write_open_fd(a: *mut archive, fd: c_int) -> c_int;
    fn archive_write_header(a: *mut archive, e: *mut archive_entry) -> c_int;
    fn archive_write_data(a: *mut archive, data: *const c_void, size: usize) -> isize;
    fn archive_write_close(a: *mut archive) -> c_int;
    fn archive_write_free(a: *mut archive) -> c_int;
    fn archive_entry_new() -> *mut archive_entry;
    fn archive_entry_free(e: *mut archive_entry);
    fn archive_entry_set_pathname(e: *mut archive_entry, name: *const c_char);
    fn archive_entry_set_filetype(e: *mut archive_entry, kind: u32);
    fn archive_entry_set_perm(e: *mut archive_entry, perm: u32);
    fn archive_entry_set_size(e: *mut archive_entry, size: i64);
}

fn text(p: *const c_char) -> Result<String> {
    if p.is_null() {
        return Err(Error::Archive(
            "Missing or non-UTF-8 archive filename".into(),
        ));
    }
    // SAFETY: libarchive owns the NUL-terminated string until the next API call.
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map(String::from)
        .map_err(|_| Error::Archive("Non-UTF-8 archive metadata is not supported".into()))
}
fn failure(a: *mut archive) -> Error {
    let p = unsafe { archive_error_string(a) };
    Error::Archive(if p.is_null() {
        "libarchive operation failed".into()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    })
}
struct Reader {
    raw: *mut archive,
    _file: File,
}
impl Drop for Reader {
    fn drop(&mut self) {
        unsafe {
            archive_read_free(self.raw);
        }
    }
}
impl Reader {
    fn new(path: &Path) -> Result<Self> {
        use std::os::fd::AsRawFd;
        let file = File::open(path)?;
        let raw = unsafe { archive_read_new() };
        if raw.is_null() {
            return Err(Error::Archive("Cannot allocate archive reader".into()));
        }
        let reader = Self { raw, _file: file };
        for status in [
            unsafe { archive_read_support_filter_all(raw) },
            unsafe { archive_read_support_format_all(raw) },
            unsafe { archive_read_open_fd(raw, reader._file.as_raw_fd(), 65536) },
        ] {
            if status < 0 {
                return Err(failure(raw));
            }
        }
        Ok(reader)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    Tar,
    TarGz,
    TarXz,
    TarBz2,
    TarZst,
    Zip,
    SevenZip,
}
impl Format {
    pub fn from_path(path: &Path) -> Option<Self> {
        let n = path.file_name()?.to_str()?.to_ascii_lowercase();
        for (suffix, format) in [
            (".tar.gz", Self::TarGz),
            (".tgz", Self::TarGz),
            (".tar.xz", Self::TarXz),
            (".txz", Self::TarXz),
            (".tar.bz2", Self::TarBz2),
            (".tbz2", Self::TarBz2),
            (".tar.zst", Self::TarZst),
            (".tar", Self::Tar),
            (".zip", Self::Zip),
            (".7z", Self::SevenZip),
        ] {
            if n.ends_with(suffix) {
                return Some(format);
            }
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Hardlink,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub path: PathBuf,
    pub kind: EntryKind,
    pub size: u64,
    pub executable: bool,
    pub link: Option<String>,
    pub sha256: Option<String>,
    #[serde(skip)]
    pub preview: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inspection {
    pub source: PathBuf,
    pub sha256: String,
    pub compressed_size: u64,
    pub total_size: u64,
    pub format: String,
    pub entries: Vec<Entry>,
}

fn check_tree(entries: &[Entry], limits: &Limits) -> Result<()> {
    let by_path: BTreeMap<_, _> = entries.iter().map(|e| (e.path.clone(), e)).collect();
    for entry in entries {
        for parent in entry
            .path
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            if let Some(e) = by_path.get(parent) {
                if e.kind != EntryKind::Directory {
                    return Err(Error::Security(format!(
                        "Non-directory used as parent: {}",
                        parent.display()
                    )));
                }
            }
        }
        if entry.kind == EntryKind::Symlink {
            let mut target = security::link_target(
                &entry.path,
                entry.link.as_deref().unwrap_or(""),
                limits.max_depth,
            )?;
            let mut visited = HashSet::from([entry.path.clone()]);
            // Links through links are rejected instead of attempting ambiguous resolution.
            loop {
                if !visited.insert(target.clone()) {
                    return Err(Error::Security("Cyclic symbolic links".into()));
                }
                if target
                    .ancestors()
                    .skip(1)
                    .any(|p| by_path.get(p).is_some_and(|e| e.kind == EntryKind::Symlink))
                {
                    return Err(Error::Security(
                        "Symbolic link target traverses another link".into(),
                    ));
                }
                match by_path.get(&target) {
                    Some(e) if e.kind == EntryKind::Symlink => {
                        target = security::link_target(
                            &e.path,
                            e.link.as_deref().unwrap_or(""),
                            limits.max_depth,
                        )?
                    }
                    _ => break,
                }
            }
        }
        if entry.kind == EntryKind::Hardlink {
            let target =
                security::archive_path(entry.link.as_deref().unwrap_or(""), limits.max_depth)?;
            if !by_path
                .get(&target)
                .is_some_and(|e| e.kind == EntryKind::File)
            {
                return Err(Error::Security(
                    "Hard link must reference a regular archive member".into(),
                ));
            }
        }
    }
    Ok(())
}

fn stream(
    path: &Path,
    limits: &Limits,
    cancel: &Cancellation,
    root: Option<&SafeRoot>,
    progress: &mut impl FnMut(Progress),
) -> Result<Inspection> {
    limits.validate()?;
    let compressed_size = std::fs::metadata(path)?.len();
    if compressed_size > limits.max_archive_bytes {
        return Err(Error::Security(
            "Archive exceeds configured input-size limit".into(),
        ));
    }
    let reader = Reader::new(path)?;
    let mut entries = Vec::new();
    let mut header_count = 0usize;
    let mut names = HashSet::new();
    let mut total = 0u64;
    let mut preview_budget = 32usize << 20;
    let mut format = String::new();
    let mut buf = [0u8; 65536];
    loop {
        cancel.check()?;
        let mut raw_entry = ptr::null_mut();
        let status = unsafe { archive_read_next_header(reader.raw, &mut raw_entry) };
        if status == 1 {
            break;
        }
        if status < 0 {
            return Err(failure(reader.raw));
        }
        if raw_entry.is_null() {
            return Err(Error::Archive("Archive returned an empty header".into()));
        }
        header_count = header_count
            .checked_add(1)
            .ok_or_else(|| Error::Security("Archive header-count overflow".into()))?;
        if header_count > limits.max_entries {
            return Err(Error::Security("Archive entry-count limit exceeded".into()));
        }
        if unsafe { archive_entry_is_encrypted(raw_entry) } > 0 {
            return Err(Error::Unsupported(
                "Encrypted archive members require a password and are not currently supported"
                    .into(),
            ));
        }
        let name = text(unsafe { archive_entry_pathname_utf8(raw_entry) })?;
        let path = security::archive_path(&name, limits.max_depth)?;
        let filetype = unsafe { archive_entry_filetype(raw_entry) };
        if path.as_os_str().is_empty() && filetype == 0o040000 {
            continue;
        }
        if path.as_os_str().is_empty() || !names.insert(path.clone()) {
            return Err(Error::Security(format!(
                "Empty or duplicate archive path: {name}"
            )));
        }
        let hard = unsafe { archive_entry_hardlink_utf8(raw_entry) };
        let kind = if !hard.is_null() {
            EntryKind::Hardlink
        } else {
            match filetype {
                0o100000 => EntryKind::File,
                0o040000 => EntryKind::Directory,
                0o120000 => EntryKind::Symlink,
                _ => {
                    return Err(Error::Security(format!(
                        "Unsupported special file in archive: {name}"
                    )))
                }
            }
        };
        let declared = unsafe { archive_entry_size(raw_entry) };
        if declared < 0 || declared as u64 > limits.max_file_bytes {
            return Err(Error::Security(format!(
                "Invalid or excessive file size: {name}"
            )));
        }
        let mut link = match kind {
            EntryKind::Symlink => Some(text(unsafe { archive_entry_symlink_utf8(raw_entry) })?),
            EntryKind::Hardlink => Some(text(hard)?),
            _ => None,
        };
        if let Some(target) = &link {
            if kind == EntryKind::Symlink {
                security::link_target(&path, target, limits.max_depth)?;
            } else {
                link = Some(
                    security::archive_path(target, limits.max_depth)?
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        let executable = unsafe { archive_entry_perm(raw_entry) } & 0o111 != 0;
        let mut output = match (&kind, root) {
            (EntryKind::Directory, Some(root)) => {
                root.mkdir(&path)?;
                None
            }
            (EntryKind::File, Some(root)) => Some(root.create_file(&path, executable)?),
            _ => None,
        };
        let mut size = 0u64;
        let mut hash = Sha256::new();
        let mut preview = Vec::new();
        loop {
            cancel.check()?;
            let count =
                unsafe { archive_read_data(reader.raw, buf.as_mut_ptr().cast(), buf.len()) };
            if count < 0 {
                return Err(failure(reader.raw));
            }
            if count == 0 {
                break;
            }
            let bytes = &buf[..count as usize];
            size = size
                .checked_add(count as u64)
                .ok_or_else(|| Error::Security("Size overflow".into()))?;
            total = total
                .checked_add(count as u64)
                .ok_or_else(|| Error::Security("Size overflow".into()))?;
            if size > limits.max_file_bytes
                || total > limits.max_total_bytes
                || total / compressed_size.max(1) > limits.max_ratio
            {
                return Err(Error::Security(
                    "Archive exceeds extracted-size or compression-ratio limit".into(),
                ));
            }
            if kind != EntryKind::File {
                return Err(Error::Security(
                    "Non-regular member contains unexpected data".into(),
                ));
            }
            hash.update(bytes);
            if preview.len() < 131072 && preview_budget > 0 {
                let n = bytes.len().min(131072 - preview.len()).min(preview_budget);
                preview.extend_from_slice(&bytes[..n]);
                preview_budget -= n;
            }
            if let Some(file) = &mut output {
                file.write_all(bytes)?;
            }
            progress(Progress {
                phase: if root.is_some() {
                    "Extracting"
                } else {
                    "Inspecting"
                }
                .into(),
                path: path.clone(),
                bytes: total,
                total: None,
            });
        }
        if kind == EntryKind::File && size != declared as u64 {
            return Err(Error::Archive(format!(
                "Truncated or inconsistent archive member: {name}"
            )));
        }
        if let Some(file) = output {
            file.sync_all()?;
        }
        format = text(unsafe { archive_format_name(reader.raw) })?;
        entries.push(Entry {
            path,
            kind: kind.clone(),
            size,
            executable,
            link,
            sha256: if kind == EntryKind::File {
                Some(format!("{:x}", hash.finalize()))
            } else {
                None
            },
            preview,
        });
    }
    check_tree(&entries, limits)?;
    for entry in &entries {
        if entry.kind == EntryKind::Hardlink {
            let target = Path::new(entry.link.as_deref().unwrap_or(""));
            let size = entries
                .iter()
                .find(|e| e.path == target)
                .map(|e| e.size)
                .ok_or_else(|| Error::Security("Missing hard link target".into()))?;
            total = total
                .checked_add(size)
                .ok_or_else(|| Error::Security("Hard link size overflow".into()))?;
            if total > limits.max_total_bytes || total / compressed_size.max(1) > limits.max_ratio {
                return Err(Error::Security(
                    "Materialized hard links exceed extraction limits".into(),
                ));
            }
        }
    }
    if entries.is_empty() {
        return Err(Error::Archive("The archive is empty".into()));
    }
    if let Some(root) = root {
        for entry in &entries {
            cancel.check()?;
            if entry.kind == EntryKind::Hardlink {
                let target = Path::new(entry.link.as_deref().unwrap_or(""));
                let target_entry = entries
                    .iter()
                    .find(|e| e.path == target)
                    .ok_or_else(|| Error::Security("Hard link target disappeared".into()))?;
                // Materialize a separate file: never create cross-scope hard links.
                let mut input = root.open_file(target)?;
                let mut output = root.create_file(&entry.path, target_entry.executable)?;
                std::io::copy(&mut input, &mut output)?;
                output.sync_all()?;
            }
        }
        for entry in &entries {
            if entry.kind == EntryKind::Symlink {
                root.symlink(&entry.path, entry.link.as_deref().unwrap_or(""))?;
            }
        }
    }
    Ok(Inspection {
        source: path.to_path_buf(),
        sha256: String::new(),
        compressed_size,
        total_size: total,
        format,
        entries,
    })
}

pub fn inspect(
    path: &Path,
    limits: &Limits,
    cancel: &Cancellation,
    mut progress: impl FnMut(Progress),
) -> Result<Inspection> {
    cancel.check()?;
    limits.validate()?;
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > limits.max_archive_bytes {
        return Err(Error::Security(
            "Archive must be a regular file within the configured input-size limit".into(),
        ));
    }
    let initial_hash = hash_cancellable(path, cancel)?;
    let mut inspection = stream(path, limits, cancel, None, &mut progress)?;
    if initial_hash != hash_cancellable(path, cancel)? {
        return Err(Error::Security(
            "Archive changed while being inspected".into(),
        ));
    }
    inspection.sha256 = initial_hash;
    Ok(inspection)
}

pub fn hash_cancellable(path: &Path, cancel: &Cancellation) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        cancel.check()?;
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Read one reviewed member for an icon preview; never writes or executes it.
pub fn read_member(
    inspection: &Inspection,
    path: &Path,
    maximum: u64,
    cancel: &Cancellation,
) -> Result<Vec<u8>> {
    let expected = inspection
        .entries
        .iter()
        .find(|e| e.path == path && e.kind == EntryKind::File)
        .ok_or_else(|| Error::Invalid("Selected file is not a regular archive member".into()))?;
    if expected.size > maximum {
        return Err(Error::Security(
            "Archive preview exceeds its size limit".into(),
        ));
    }
    if expected.preview.len() as u64 == expected.size {
        let hash = format!("{:x}", Sha256::digest(&expected.preview));
        if expected.sha256.as_deref() != Some(&hash) {
            return Err(Error::Security(
                "Cached archive member failed its integrity check".into(),
            ));
        }
        return Ok(expected.preview.clone());
    }
    let reader = Reader::new(&inspection.source)?;
    let mut count = 0usize;
    loop {
        cancel.check()?;
        let mut entry = ptr::null_mut();
        let status = unsafe { archive_read_next_header(reader.raw, &mut entry) };
        if status == 1 {
            return Err(Error::Archive(
                "Selected archive member was not found".into(),
            ));
        }
        if status < 0 {
            return Err(failure(reader.raw));
        }
        count += 1;
        if count > inspection.entries.len().saturating_add(1024) {
            return Err(Error::Security("Archive changed since inspection".into()));
        }
        if entry.is_null() {
            return Err(Error::Archive("Empty archive header".into()));
        }
        let name = text(unsafe { archive_entry_pathname_utf8(entry) })?;
        if security::archive_path(&name, 256)? != path {
            continue;
        }
        if unsafe { archive_entry_filetype(entry) } != 0o100000 {
            return Err(Error::Security(
                "Preview member is no longer a regular file".into(),
            ));
        }
        let mut bytes = Vec::new();
        let mut buf = [0u8; 65536];
        loop {
            cancel.check()?;
            let n = unsafe { archive_read_data(reader.raw, buf.as_mut_ptr().cast(), buf.len()) };
            if n < 0 {
                return Err(failure(reader.raw));
            }
            if n == 0 {
                break;
            }
            if bytes.len() as u64 + n as u64 > maximum {
                return Err(Error::Security(
                    "Archive preview exceeded its size limit".into(),
                ));
            }
            bytes.extend_from_slice(&buf[..n as usize]);
        }
        let hash = format!("{:x}", Sha256::digest(&bytes));
        if bytes.len() as u64 != expected.size || expected.sha256.as_deref() != Some(&hash) {
            return Err(Error::Security(
                "Selected file changed since archive inspection".into(),
            ));
        }
        return Ok(bytes);
    }
}

/// Extract to a new directory only. Nothing is merged into an existing directory.
pub fn extract(
    inspection: &Inspection,
    destination: &Path,
    limits: &Limits,
    cancel: &Cancellation,
    mut progress: impl FnMut(Progress),
) -> Result<()> {
    if destination.try_exists()? {
        return Err(Error::Conflict(destination.display().to_string()));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| Error::Invalid("Extraction destination needs a parent".into()))?;
    if !parent.is_dir() {
        return Err(Error::Invalid("Choose an existing parent directory".into()));
    }
    let staging = tempfile::Builder::new()
        .prefix(".unpack-extract-")
        .tempdir_in(parent)?;
    // Work from a private snapshot: downloaded files can change between review and execution.
    let mut snapshot = tempfile::NamedTempFile::new()?;
    copy_limited(
        &inspection.source,
        snapshot.as_file_mut(),
        limits.max_archive_bytes,
        cancel,
    )?;
    if hash_cancellable(snapshot.path(), cancel)? != inspection.sha256 {
        return Err(Error::Security(
            "Archive changed since inspection; open it again".into(),
        ));
    }
    let root = SafeRoot::open(staging.path())?;
    let actual = stream(snapshot.path(), limits, cancel, Some(&root), &mut progress)?;
    if actual.entries.len() != inspection.entries.len() {
        return Err(Error::Security("Archive inventory changed".into()));
    }
    cancel.check()?;
    rename_noreplace(staging.path(), destination)?;
    Ok(())
}

pub fn copy_cancellable(
    source: &Path,
    destination: &mut File,
    cancel: &Cancellation,
) -> Result<()> {
    copy_limited(source, destination, u64::MAX, cancel)
}

fn copy_limited(
    source: &Path,
    destination: &mut File,
    maximum: u64,
    cancel: &Cancellation,
) -> Result<()> {
    let mut source = File::open(source)?;
    if !source.metadata()?.is_file() {
        return Err(Error::Security(
            "Archive input is not a regular file".into(),
        ));
    }
    let mut buf = [0u8; 65536];
    let mut total = 0u64;
    loop {
        cancel.check()?;
        let n = source.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or_else(|| Error::Security("Input size overflow".into()))?;
        if total > maximum {
            return Err(Error::Security(
                "Archive input-size limit exceeded during snapshot".into(),
            ));
        }
        destination.write_all(&buf[..n])?;
    }
    destination.sync_all()?;
    Ok(())
}

pub fn rename_noreplace(from: &Path, to: &Path) -> Result<()> {
    let from = CString::new(from.as_os_str().as_bytes())
        .map_err(|_| Error::Invalid("NUL in path".into()))?;
    let to = CString::new(to.as_os_str().as_bytes())
        .map_err(|_| Error::Invalid("NUL in path".into()))?;
    let rc = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if rc < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

struct Writer(*mut archive);
impl Drop for Writer {
    fn drop(&mut self) {
        unsafe {
            archive_write_free(self.0);
        }
    }
}
fn write_status(raw: *mut archive, status: c_int) -> Result<()> {
    if status < 0 {
        Err(failure(raw))
    } else {
        Ok(())
    }
}

pub fn create(
    sources: &[PathBuf],
    destination: &Path,
    format: Format,
    cancel: &Cancellation,
    mut progress: impl FnMut(Progress),
) -> Result<()> {
    use std::os::{fd::AsRawFd, unix::fs::PermissionsExt};
    if sources.is_empty() {
        return Err(Error::Invalid("Select at least one file".into()));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| Error::Invalid("Missing destination parent".into()))?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    let raw = unsafe { archive_write_new() };
    if raw.is_null() {
        return Err(Error::Archive("Cannot allocate archive writer".into()));
    }
    let _writer = Writer(raw);
    let status = unsafe {
        match format {
            Format::Zip => archive_write_set_format_zip(raw),
            Format::SevenZip => archive_write_set_format_7zip(raw),
            _ => archive_write_set_format_pax_restricted(raw),
        }
    };
    write_status(raw, status)?;
    let status = unsafe {
        match format {
            Format::TarGz => archive_write_add_filter_gzip(raw),
            Format::TarXz => archive_write_add_filter_xz(raw),
            Format::TarBz2 => archive_write_add_filter_bzip2(raw),
            Format::TarZst => archive_write_add_filter_zstd(raw),
            _ => 0,
        }
    };
    write_status(raw, status)?;
    write_status(raw, unsafe {
        archive_write_open_fd(raw, temporary.as_file().as_raw_fd())
    })?;
    let mut queue = Vec::new();
    for source in sources {
        let source = std::fs::canonicalize(source)?;
        if source.is_dir() && destination.starts_with(&source) {
            return Err(Error::Invalid(
                "Archive output cannot be inside a source directory".into(),
            ));
        }
        let name = source
            .file_name()
            .ok_or_else(|| Error::Invalid("Cannot archive the filesystem root".into()))?;
        queue.push((source.clone(), PathBuf::from(name)));
    }
    let mut names = HashSet::new();
    let mut bytes = 0;
    while let Some((source, name)) = queue.pop() {
        cancel.check()?;
        let name_text = name
            .to_str()
            .ok_or_else(|| Error::Invalid("Non-UTF-8 source name".into()))?;
        security::archive_path(name_text, 256)?;
        if !names.insert(name.clone()) {
            return Err(Error::Conflict(format!(
                "Duplicate source name: {}",
                name.display()
            )));
        }
        let metadata = std::fs::symlink_metadata(&source)?;
        if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            return Err(Error::Unsupported(format!(
                "Archive creation accepts regular files and directories, not {}",
                source.display()
            )));
        }
        let c_name =
            CString::new(name_text).map_err(|_| Error::Invalid("NUL in source name".into()))?;
        let e = unsafe { archive_entry_new() };
        if e.is_null() {
            return Err(Error::Archive("Cannot allocate archive entry".into()));
        }
        unsafe {
            archive_entry_set_pathname(e, c_name.as_ptr());
            archive_entry_set_filetype(
                e,
                if metadata.is_dir() {
                    0o040000
                } else {
                    0o100000
                },
            );
            archive_entry_set_perm(
                e,
                if metadata.is_dir() || metadata.permissions().mode() & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                },
            );
            archive_entry_set_size(
                e,
                if metadata.is_file() {
                    metadata.len() as i64
                } else {
                    0
                },
            );
        }
        let status = unsafe { archive_write_header(raw, e) };
        unsafe {
            archive_entry_free(e);
        }
        write_status(raw, status)?;
        if metadata.is_dir() {
            for child in std::fs::read_dir(source)? {
                let child = child?;
                queue.push((child.path(), name.join(child.file_name())));
            }
        } else {
            let mut file = File::open(source)?;
            let mut buf = [0u8; 65536];
            loop {
                cancel.check()?;
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                let written = unsafe { archive_write_data(raw, buf.as_ptr().cast(), n) };
                if written != n as isize {
                    return Err(failure(raw));
                }
                bytes += n as u64;
                progress(Progress {
                    phase: "Creating archive".into(),
                    path: name.clone(),
                    bytes,
                    total: None,
                });
            }
        }
    }
    write_status(raw, unsafe { archive_write_close(raw) })?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(destination)
        .map_err(|e| Error::Io(e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewed_member_read_is_bounded_and_detects_changed_payload() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("icon.png");
        let bytes = vec![b'x'; 200_000];
        std::fs::write(&source, &bytes).unwrap();
        let path = dir.path().join("icons.tar");
        let cancel = Cancellation::default();
        create(
            std::slice::from_ref(&source),
            &path,
            Format::Tar,
            &cancel,
            |_| {},
        )
        .unwrap();
        let inspection = inspect(&path, &Limits::default(), &cancel, |_| {}).unwrap();
        assert_eq!(
            read_member(&inspection, Path::new("icon.png"), 300_000, &cancel).unwrap(),
            bytes
        );
        assert!(read_member(&inspection, Path::new("icon.png"), 100, &cancel).is_err());
        std::fs::write(&source, vec![b'y'; 200_000]).unwrap();
        std::fs::remove_file(&path).unwrap();
        create(&[source], &path, Format::Tar, &cancel, |_| {}).unwrap();
        assert!(read_member(&inspection, Path::new("icon.png"), 300_000, &cancel).is_err());
    }
    #[test]
    fn formats() {
        for name in [
            "a.tar.gz",
            "a.tgz",
            "a.tar.xz",
            "a.tar.bz2",
            "a.tar.zst",
            "a.tar",
            "a.ZIP",
            "a.7z",
        ] {
            assert!(Format::from_path(Path::new(name)).is_some());
        }
        assert!(Format::from_path(Path::new("a.zip.exe")).is_none());
    }
    #[test]
    fn create_inspect_extract_roundtrip() {
        for format in [
            Format::Tar,
            Format::TarGz,
            Format::TarXz,
            Format::TarBz2,
            Format::TarZst,
            Format::Zip,
            Format::SevenZip,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let source = dir.path().join("hello.txt");
            std::fs::write(&source, "Hello archive").unwrap();
            let archive = dir.path().join("archive");
            let token = Cancellation::default();
            create(&[source], &archive, format, &token, |_| {}).unwrap();
            let inspection = inspect(&archive, &Limits::default(), &token, |_| {}).unwrap();
            assert_eq!(inspection.total_size, 13);
            let destination = dir.path().join("out");
            extract(
                &inspection,
                &destination,
                &Limits::default(),
                &token,
                |_| {},
            )
            .unwrap();
            assert_eq!(
                std::fs::read_to_string(destination.join("hello.txt")).unwrap(),
                "Hello archive"
            );
            assert!(extract(
                &inspection,
                &destination,
                &Limits::default(),
                &token,
                |_| {}
            )
            .is_err());
        }
    }
}
