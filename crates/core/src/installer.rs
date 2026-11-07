use crate::{
    archive::{self, EntryKind, Inspection},
    config::Paths,
    desktop::DesktopEntry,
    detection::{self, ApplicationKind, Detection},
    security::{self, SafeRoot},
    Cancellation, Error, Progress, Result,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::Write,
    os::{fd::AsRawFd, unix::fs::PermissionsExt},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedFile {
    pub path: PathBuf,
    pub kind: EntryKind,
    pub sha256: Option<String>,
    pub link: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub path: PathBuf,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub installed_at: u64,
    pub destination: PathBuf,
    pub executable: PathBuf,
    pub files: Vec<ManagedFile>,
    pub artifacts: Vec<Artifact>,
    pub source_archive: PathBuf,
    pub source_sha256: String,
    pub profile: String,
    pub method: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub terminal: bool,
}
#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub id: String,
    pub name: String,
    pub destination_parent: PathBuf,
    pub entry_point: PathBuf,
    pub create_launcher: bool,
    pub install_icon: bool,
    pub icon_path: Option<PathBuf>,
    pub terminal: bool,
    pub arguments: Vec<String>,
    pub profile: String,
    pub update: bool,
}
#[derive(Debug, Clone)]
pub struct Plan {
    pub inspection: Inspection,
    pub options: InstallOptions,
    pub destination: PathBuf,
    pub executable: PathBuf,
    pub launcher: Option<(PathBuf, String)>,
    pub icon: Option<(PathBuf, PathBuf)>,
    pub operations: Vec<String>,
    pub warnings: Vec<String>,
}

fn valid_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 80 || id != detection::application_id(id) {
        return Err(Error::Invalid(
            "Application ID must contain lowercase ASCII letters, digits and single hyphens".into(),
        ));
    }
    Ok(())
}
fn manifest_path(paths: &Paths, id: &str) -> Result<PathBuf> {
    valid_id(id)?;
    Ok(paths.data.join("manifests").join(format!("{id}.json")))
}

pub fn registry(paths: &Paths) -> Result<Vec<Manifest>> {
    let directory = paths.data.join("manifests");
    if !directory.try_exists()? {
        return Ok(vec![]);
    }
    let mut manifests = Vec::new();
    for item in std::fs::read_dir(directory)? {
        let item = item?;
        if item.path().extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if !item.file_type()?.is_file() {
            return Err(Error::Security(
                "Manifest registry contains a non-regular file".into(),
            ));
        }
        let manifest: Manifest = serde_json::from_reader(File::open(item.path())?)?;
        validate_manifest(paths, &manifest)?;
        if item.path() != manifest_path(paths, &manifest.id)? {
            return Err(Error::Security(
                "Manifest filename and application identifier disagree".into(),
            ));
        }
        manifests.push(manifest);
    }
    manifests.sort_by_key(|m| m.name.to_lowercase());
    Ok(manifests)
}
fn validate_manifest(paths: &Paths, manifest: &Manifest) -> Result<()> {
    valid_id(&manifest.id)?;
    if manifest.schema_version != 1 {
        return Err(Error::Unsupported(
            "Unrecognized installation manifest schema".into(),
        ));
    }
    if !manifest.destination.is_absolute()
        || !manifest.destination.starts_with(&paths.home)
        || manifest
            .destination
            .components()
            .chain(manifest.executable.components())
            .any(|c| matches!(c, std::path::Component::ParentDir))
        || manifest.destination.file_name().and_then(|s| s.to_str()) != Some(&manifest.id)
        || manifest.destination == paths.home
        || !manifest.executable.starts_with(&manifest.destination)
    {
        return Err(Error::Security(
            "Invalid manifest installation scope".into(),
        ));
    }
    let mut names = HashSet::new();
    for file in &manifest.files {
        let name = file
            .path
            .to_str()
            .ok_or_else(|| Error::Security("Invalid manifest filename".into()))?;
        if security::archive_path(name, 256)? != file.path
            || file.path.as_os_str().is_empty()
            || !names.insert(&file.path)
        {
            return Err(Error::Security("Invalid or duplicate managed path".into()));
        }
    }
    for artifact in &manifest.artifacts {
        let name = artifact
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let expected_name = format!("io.github.unpack.{}", manifest.id);
        if name != expected_name
            || !(artifact.path.parent() == Some(paths.applications.as_path())
                || artifact.path.parent() == Some(paths.icons.join("scalable/apps").as_path())
                || artifact.path.parent() == Some(paths.icons.join("256x256/apps").as_path()))
        {
            return Err(Error::Security(
                "Manifest artifact is outside managed desktop integration scope".into(),
            ));
        }
    }
    Ok(())
}

pub fn duplicates<'a>(detection: &Detection, installed: &'a [Manifest]) -> Vec<&'a Manifest> {
    installed
        .iter()
        .filter(|m| {
            m.id == detection.suggested_id
                || detection::application_id(&m.name) == detection::application_id(&detection.name)
                || detection
                    .candidates
                    .iter()
                    .any(|c| c.path.file_name() == m.executable.file_name())
        })
        .collect()
}

pub fn plan(
    paths: &Paths,
    inspection: &Inspection,
    detection: &Detection,
    options: InstallOptions,
) -> Result<Plan> {
    if crate::config::sandboxed_package() {
        return Err(Error::Unsupported("The Flatpak edition cannot install applications or register launchers on the host. Use the AppImage or native edition. Archive inspection and extraction remain available.".into()));
    }
    valid_id(&options.id)?;
    if options.name.trim().is_empty() {
        return Err(Error::Invalid("Enter an application name".into()));
    }
    if matches!(
        detection.kind,
        ApplicationKind::AppImageDistribution
            | ApplicationKind::SystemFilesystem
            | ApplicationKind::SourceCode
            | ApplicationKind::InstallerBased
    ) {
        return Err(Error::Unsupported("This distribution requires a different installation method. Read its instructions or extract it; Unpack will not run its installer.".into()));
    }
    let entry = inspection
        .entries
        .iter()
        .find(|e| e.path == options.entry_point && e.kind == EntryKind::File)
        .ok_or_else(|| Error::Invalid("Choose a regular archive file as the entry point".into()))?;
    if entry
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| {
            matches!(
                n.to_ascii_lowercase().as_str(),
                "install.sh" | "setup.sh" | "installer.sh" | "uninstall.sh"
            )
        })
    {
        return Err(Error::Security(
            "Installation scripts cannot be used as application launchers".into(),
        ));
    }
    if !options.destination_parent.is_absolute()
        || options
            .destination_parent
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(Error::Invalid(
            "Use an absolute destination without '..' components".into(),
        ));
    }
    let mut ancestor = options.destination_parent.as_path();
    while !ancestor.try_exists()? {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| Error::Invalid("No existing destination ancestor".into()))?;
    }
    let parent = std::fs::canonicalize(ancestor)?.join(
        options
            .destination_parent
            .strip_prefix(ancestor)
            .map_err(|_| Error::Invalid("Invalid destination".into()))?,
    );
    if !parent.starts_with(&paths.home) || parent == paths.home {
        return Err(Error::Unsupported("Managed installations currently require a directory inside your home, such as ~/.local/opt or ~/Applications".into()));
    }
    let destination = parent.join(&options.id);
    let existing = registry(paths)?.into_iter().find(|m| m.id == options.id);
    if options.update {
        let old = existing.as_ref().ok_or_else(|| {
            Error::Invalid("No installed application with this identifier to update".into())
        })?;
        if old.destination != destination {
            return Err(Error::Conflict(
                "Updates must retain the existing installation destination".into(),
            ));
        }
        verify_owned_tree(old)?;
        verify_artifacts(old)?;
    } else if existing.is_some() || destination.try_exists()? {
        return Err(Error::Conflict(format!("{} is already installed or the destination exists. Explicitly choose Update or another application ID.", options.id)));
    }
    let executable = destination.join(&options.entry_point);
    let icon = if options.install_icon {
        let chosen = options
            .icon_path
            .as_ref()
            .or(detection.recommended_icon.as_ref());
        if let Some(path) = chosen {
            if !inspection
                .entries
                .iter()
                .any(|e| &e.path == path && crate::identity::supported_icon(e))
            {
                return Err(Error::Invalid("The selected icon must be a regular PNG or XPM inside this archive, smaller than 8 MiB".into()));
            }
        }
        chosen.map(|p| {
            (
                p.clone(),
                paths.icons.join("256x256/apps").join(format!(
                    "io.github.unpack.{}.{}",
                    options.id,
                    p.extension()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_ascii_lowercase()
                )),
            )
        })
    } else {
        None
    };
    let launcher = if options.create_launcher {
        let entry = DesktopEntry {
            name: options.name.clone(),
            generic_name: String::new(),
            comment: "Portable application installed with Unpack".into(),
            executable: executable.to_string_lossy().into_owned(),
            arguments: options.arguments.clone(),
            icon: if icon.is_some() {
                format!("io.github.unpack.{}", options.id)
            } else {
                "application-x-executable".into()
            },
            terminal: options.terminal,
            categories: vec!["Utility".into()],
            startup_notify: true,
            startup_wm_class: None,
        };
        Some((
            paths
                .applications
                .join(format!("io.github.unpack.{}.desktop", options.id)),
            entry.render()?,
        ))
    } else {
        None
    };
    for path in launcher
        .iter()
        .map(|(p, _)| p)
        .chain(icon.iter().map(|(_, p)| p))
    {
        if path.try_exists()?
            && !existing
                .as_ref()
                .is_some_and(|m| m.artifacts.iter().any(|a| &a.path == path))
        {
            return Err(Error::Conflict(path.display().to_string()));
        }
    }
    let mut operations = vec![
        format!("CREATE parent directories if missing: {}", parent.display()),
        format!(
            "{} {}",
            if options.update {
                "REPLACE (recoverable backup)"
            } else {
                "CREATE"
            },
            destination.display()
        ),
    ];
    operations.push(format!("COPY all {} archive entries ({} bytes of regular file data), preserving application structure", inspection.entries.len(), inspection.total_size));
    if !entry.executable {
        operations.push(format!(
            "SET executable permission: {} (0755)",
            executable.display()
        ));
    }
    if let Some((path, _)) = &launcher {
        operations.push(format!("CREATE launcher: {}", path.display()));
    }
    if let Some((_, path)) = &icon {
        operations.push(format!("CREATE icon: {}", path.display()));
    }
    if let Some((source, _)) = &icon {
        operations.push(format!("USE archive icon: {}", source.display()));
    }
    operations.push(format!(
        "WRITE manifest: {}",
        manifest_path(paths, &options.id)?.display()
    ));
    operations
        .push("PRESERVE user configuration, documents and data outside the installation".into());
    let mut warnings = detection.warnings.clone();
    if entry.preview.starts_with(b"#!") {
        warnings.push("The selected application is a script. It will not run during installation; launching it later executes its contents.".into());
    }
    Ok(Plan {
        inspection: inspection.clone(),
        options,
        destination,
        executable,
        launcher,
        icon,
        operations,
        warnings,
    })
}

struct Lock(File);
impl Lock {
    fn acquire(paths: &Paths) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        paths.initialize()?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(paths.state.join("operations.lock"))?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } < 0 {
            return Err(Error::Conflict(
                "Another installation operation is running".into(),
            ));
        }
        Ok(Self(file))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Move {
    from: PathBuf,
    to: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Journal {
    schema_version: u32,
    id: String,
    committed: bool,
    moves: Vec<Move>,
}
struct Transaction {
    path: PathBuf,
    journal: Journal,
}
impl Transaction {
    fn new(paths: &Paths, id: &str) -> Result<Self> {
        let path = paths.state.join("transactions").join(format!("{id}.json"));
        if path.try_exists()? {
            let previous: Journal = serde_json::from_reader(File::open(&path)?)?;
            if !previous.committed {
                return Err(Error::Conflict(
                    "An interrupted transaction needs recovery first".into(),
                ));
            }
            let records = paths.state.join("transaction-records");
            std::fs::create_dir_all(&records)?;
            let target = backup_path(&records.join("record"))?;
            archive::rename_noreplace(&path, &target)?;
        }
        let tx = Self {
            path,
            journal: Journal {
                schema_version: 1,
                id: id.into(),
                committed: false,
                moves: vec![],
            },
        };
        security::atomic_json(&tx.path, &tx.journal)?;
        Ok(tx)
    }
    fn move_file(&mut self, from: &Path, to: &Path) -> Result<()> {
        if to.try_exists()? || std::fs::symlink_metadata(to).is_ok() {
            return Err(Error::Conflict(to.display().to_string()));
        }
        self.journal.moves.push(Move {
            from: from.into(),
            to: to.into(),
        });
        security::atomic_json(&self.path, &self.journal)?;
        archive::rename_noreplace(from, to)?;
        if let Some(parent) = from.parent() {
            File::open(parent)?.sync_all()?;
        }
        if let Some(parent) = to.parent() {
            File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
    fn rollback(&self) -> Result<()> {
        rollback_moves(&self.journal)?;
        std::fs::remove_file(&self.path)?;
        Ok(())
    }
    fn commit(&mut self) -> Result<()> {
        self.journal.committed = true;
        security::atomic_json(&self.path, &self.journal)?;
        // The committed journal is retained as an operation record. Backups are
        // not automatically deleted; they are the user's recovery point.
        Ok(())
    }
}
fn rollback_moves(journal: &Journal) -> Result<()> {
    for operation in journal.moves.iter().rev() {
        let from = std::fs::symlink_metadata(&operation.from).is_ok();
        let to = std::fs::symlink_metadata(&operation.to).is_ok();
        if !from && to {
            if let Some(parent) = operation.from.parent() {
                if parent
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(".unpack-stage-"))
                {
                    std::fs::create_dir_all(parent)?;
                }
            }
            archive::rename_noreplace(&operation.to, &operation.from)?;
        } else if from && to {
            return Err(Error::Conflict(format!(
                "Recovery would overwrite {}",
                operation.from.display()
            )));
        }
    }
    Ok(())
}

pub fn recover(paths: &Paths) -> Result<Vec<String>> {
    let _lock = Lock::acquire(paths)?;
    let mut recovered = vec![];
    for item in std::fs::read_dir(paths.state.join("transactions"))? {
        let item = item?;
        if item.path().extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let journal: Journal = serde_json::from_reader(File::open(item.path())?)?;
        if journal.schema_version != 1 {
            return Err(Error::Unsupported(
                "Unknown transaction journal version".into(),
            ));
        }
        valid_id(&journal.id)?;
        // Journal paths originate from this process, but still restrict recovery
        // to the user home and application-specific XDG directories.
        for m in &journal.moves {
            for p in [&m.from, &m.to] {
                if !p.is_absolute()
                    || p.components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
                    || ![
                        &paths.home,
                        &paths.data,
                        &paths.state,
                        &paths.applications,
                        &paths.icons,
                    ]
                    .iter()
                    .any(|root| p.starts_with(root))
                {
                    return Err(Error::Security(
                        "Transaction path is outside permitted user scopes".into(),
                    ));
                }
            }
        }
        if journal.committed {
            continue;
        }
        rollback_moves(&journal)?;
        recovered.push(format!(
            "Recovered interrupted operation for {}",
            journal.id
        ));
        let history = paths.state.join("transaction-records");
        std::fs::create_dir_all(&history)?;
        let target = history.join(format!(
            "{}-{}-{}.json",
            journal.id,
            crate::timestamp(),
            recovered.len()
        ));
        archive::rename_noreplace(&item.path(), &target)?;
    }
    Ok(recovered)
}

fn temporary_artifact(path: &Path, contents: &[u8]) -> Result<tempfile::NamedTempFile> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("Missing artifact parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(contents)?;
    temp.as_file().sync_all()?;
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o644))?;
    Ok(temp)
}
fn backup_path(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("Backup requires a parent".into()))?;
    let placeholder = tempfile::Builder::new()
        .prefix(".unpack-backup-")
        .tempfile_in(parent)?;
    let path = placeholder.path().to_path_buf();
    placeholder.close()?;
    Ok(path)
}

pub fn execute(
    paths: &Paths,
    plan: &Plan,
    limits: &security::Limits,
    cancel: &Cancellation,
    mut progress: impl FnMut(Progress),
) -> Result<Manifest> {
    if crate::config::sandboxed_package() {
        return Err(Error::Unsupported(
            "Host application installation is unavailable in the Flatpak edition".into(),
        ));
    }
    let _lock = Lock::acquire(paths)?;
    let old = registry(paths)?
        .into_iter()
        .find(|m| m.id == plan.options.id);
    if plan.options.update {
        let old = old
            .as_ref()
            .ok_or_else(|| Error::Conflict("Application disappeared since review".into()))?;
        verify_owned_tree(old)?;
        verify_artifacts(old)?;
    } else if old.is_some() || plan.destination.try_exists()? {
        return Err(Error::Conflict(
            "Destination changed since installation review".into(),
        ));
    }
    let parent = plan
        .destination
        .parent()
        .ok_or_else(|| Error::Invalid("Invalid installation parent".into()))?;
    std::fs::create_dir_all(parent)?;
    if std::fs::canonicalize(parent)? != parent {
        return Err(Error::Security(
            "Installation parent changed since review".into(),
        ));
    }
    let staging = tempfile::Builder::new()
        .prefix(".unpack-stage-")
        .tempdir_in(parent)?;
    let payload = staging.path().join("payload");
    archive::extract(&plan.inspection, &payload, limits, cancel, &mut progress)?;
    let root = SafeRoot::open(&payload)?;
    let entry = root.open_file(&plan.options.entry_point)?;
    entry.set_permissions(std::fs::Permissions::from_mode(0o755))?;
    let files = inventory(&payload)?;
    let mut artifacts = Vec::new();
    let mut staged_artifacts = Vec::new();
    if let Some((source, destination)) = &plan.icon {
        let mut input = root.open_file(source)?;
        if input.metadata()?.len() > 8 << 20 {
            return Err(Error::Security(
                "Icon exceeds the 8 MiB integration limit".into(),
            ));
        }
        let mut bytes = Vec::new();
        use std::io::Read;
        input.read_to_end(&mut bytes)?;
        crate::identity::validate_icon(source, &bytes)?;
        let temporary = temporary_artifact(destination, &bytes)?;
        artifacts.push(Artifact {
            path: destination.clone(),
            sha256: security::digest(temporary.path())?,
        });
        staged_artifacts.push((temporary, destination.clone()));
    }
    if let Some((destination, contents)) = &plan.launcher {
        let temporary = temporary_artifact(destination, contents.as_bytes())?;
        artifacts.push(Artifact {
            path: destination.clone(),
            sha256: security::digest(temporary.path())?,
        });
        staged_artifacts.push((temporary, destination.clone()));
    }
    let manifest = Manifest {
        schema_version: 1,
        id: plan.options.id.clone(),
        name: plan.options.name.clone(),
        version: None,
        installed_at: crate::timestamp(),
        destination: plan.destination.clone(),
        executable: plan.executable.clone(),
        files,
        artifacts,
        source_archive: plan.inspection.source.clone(),
        source_sha256: plan.inspection.sha256.clone(),
        profile: plan.options.profile.clone(),
        method: "managed-archive".into(),
        arguments: plan.options.arguments.clone(),
        terminal: plan.options.terminal,
    };
    let manifest_target = manifest_path(paths, &manifest.id)?;
    let manifest_temp =
        temporary_artifact(&manifest_target, &serde_json::to_vec_pretty(&manifest)?)?;
    let mut transaction = Transaction::new(paths, &manifest.id)?;
    let result = (|| {
        cancel.check()?;
        if let Some(old) = &old {
            transaction.move_file(&old.destination, &backup_path(&old.destination)?)?;
            for artifact in &old.artifacts {
                transaction.move_file(&artifact.path, &backup_path(&artifact.path)?)?;
            }
            transaction.move_file(&manifest_target, &backup_path(&manifest_target)?)?;
        }
        transaction.move_file(&payload, &plan.destination)?;
        for (temporary, target) in &staged_artifacts {
            cancel.check()?;
            transaction.move_file(temporary.path(), target)?;
        }
        cancel.check()?;
        transaction.move_file(manifest_temp.path(), &manifest_target)?;
        transaction.commit()?;
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(rollback) = transaction.rollback() {
            return Err(Error::Conflict(format!("{error}. Automatic rollback also failed: {rollback}. The journal was retained for recovery.")));
        }
        return Err(error);
    }
    history_event(
        paths,
        if old.is_some() {
            "Updated"
        } else {
            "Installed"
        },
        &manifest,
        &plan.operations,
    ).map_err(|e| Error::Conflict(format!("Installation completed successfully, but its history record could not be saved: {e}. The manifest and recovery journal are intact; do not reinstall blindly.")))?;
    progress(Progress {
        phase: "Installation complete".into(),
        path: plan.destination.clone(),
        bytes: plan.inspection.total_size,
        total: Some(plan.inspection.total_size),
    });
    Ok(manifest)
}

fn inventory(root: &Path) -> Result<Vec<ManagedFile>> {
    let safe = SafeRoot::open(root)?;
    let mut queue = vec![PathBuf::new()];
    let mut files = vec![];
    while let Some(relative) = queue.pop() {
        for item in std::fs::read_dir(root.join(&relative))? {
            let item = item?;
            let path = relative.join(item.file_name());
            let metadata = std::fs::symlink_metadata(item.path())?;
            let (kind, sha256, link) = if metadata.file_type().is_symlink() {
                (
                    EntryKind::Symlink,
                    None,
                    Some(
                        std::fs::read_link(item.path())?
                            .to_string_lossy()
                            .into_owned(),
                    ),
                )
            } else if metadata.is_dir() {
                queue.push(path.clone());
                (EntryKind::Directory, None, None)
            } else if metadata.is_file() {
                (
                    EntryKind::File,
                    Some(security::digest_reader(safe.open_file(&path)?)?),
                    None,
                )
            } else {
                return Err(Error::Security(
                    "Unexpected file type in managed tree".into(),
                ));
            };
            files.push(ManagedFile {
                path,
                kind,
                sha256,
                link,
            });
        }
    }
    files.sort_by_key(|f| f.path.clone());
    Ok(files)
}

fn verify_owned_tree(manifest: &Manifest) -> Result<()> {
    let actual = inventory(&manifest.destination)?;
    if actual.len() != manifest.files.len() {
        return Err(Error::Conflict("Installation contains added or removed files. Preserve or move those changes before update or full removal.".into()));
    }
    for file in &actual {
        if !manifest.files.iter().any(|m| {
            m.path == file.path
                && m.kind == file.kind
                && m.sha256 == file.sha256
                && m.link == file.link
        }) {
            return Err(Error::Conflict(format!(
                "Managed file changed: {}. It will not be overwritten or removed.",
                file.path.display()
            )));
        }
    }
    Ok(())
}
fn verify_artifacts(manifest: &Manifest) -> Result<()> {
    for artifact in &manifest.artifacts {
        if std::fs::symlink_metadata(&artifact.path)?
            .file_type()
            .is_symlink()
            || security::digest(&artifact.path)? != artifact.sha256
        {
            return Err(Error::Conflict(format!(
                "Desktop integration file changed: {}",
                artifact.path.display()
            )));
        }
    }
    Ok(())
}

pub fn uninstall_preview(manifest: &Manifest) -> Vec<String> {
    let mut lines = vec![format!(
        "REMOVE {} recorded application entries from {}",
        manifest.files.len(),
        manifest.destination.display()
    )];
    lines.extend(
        manifest
            .artifacts
            .iter()
            .map(|a| format!("REMOVE {}", a.path.display())),
    );
    lines.push(
        "PRESERVE user configuration, application data and documents outside this installation"
            .into(),
    );
    lines.push(
        "Modified managed files will be preserved and reported; added files are never removed"
            .into(),
    );
    lines
}

pub fn uninstall(paths: &Paths, manifest: &Manifest, cancel: &Cancellation) -> Result<Vec<String>> {
    let _lock = Lock::acquire(paths)?;
    validate_manifest(paths, manifest)?;
    cancel.check()?;
    // Conservative policy: a changed tree is not partially removed. The user
    // gets an exact conflict rather than a damaged installation.
    verify_owned_tree(manifest)?;
    verify_artifacts(manifest)?;
    let mut transaction = Transaction::new(paths, &manifest.id)?;
    let result = (|| {
        transaction.move_file(&manifest.destination, &backup_path(&manifest.destination)?)?;
        for artifact in &manifest.artifacts {
            cancel.check()?;
            transaction.move_file(&artifact.path, &backup_path(&artifact.path)?)?;
        }
        let path = manifest_path(paths, &manifest.id)?;
        transaction.move_file(&path, &backup_path(&path)?)?;
        transaction.commit()?;
        Ok(())
    })();
    if let Err(error) = result {
        transaction.rollback()?;
        return Err(error);
    }
    let lines = uninstall_preview(manifest);
    history_event(
        paths,
        "Removed (recoverable backup retained)",
        manifest,
        &lines,
    ).map_err(|e| Error::Conflict(format!("Application removal completed with recoverable backups, but history could not be saved: {e}")))?;
    Ok(lines)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub timestamp: u64,
    pub action: String,
    pub application: String,
    pub id: String,
    pub details: Vec<String>,
}
fn history_event(
    paths: &Paths,
    action: &str,
    manifest: &Manifest,
    details: &[String],
) -> Result<()> {
    let mut events = history(paths)?;
    events.push(HistoryEvent {
        timestamp: crate::timestamp(),
        action: action.into(),
        application: manifest.name.clone(),
        id: manifest.id.clone(),
        details: details.into(),
    });
    security::atomic_json(&paths.state.join("history.json"), &events)
}
/// Change only owned descriptive integration artifacts, never application files.
/// The selected icon must be a reviewed member of the supplied archive.
pub fn refresh_identity(
    paths: &Paths,
    current: &Manifest,
    inspection: &Inspection,
    name: &str,
    icon_path: Option<&Path>,
    cancel: &Cancellation,
) -> Result<Manifest> {
    if crate::config::sandboxed_package() {
        return Err(Error::Unsupported(
            "Host application metadata cannot be changed from the Flatpak edition".into(),
        ));
    }
    let _lock = Lock::acquire(paths)?;
    validate_manifest(paths, current)?;
    cancel.check()?;
    let recorded = registry(paths)?
        .into_iter()
        .find(|m| m.id == current.id)
        .ok_or_else(|| Error::Conflict("The installed application no longer exists".into()))?;
    if serde_json::to_value(&recorded)? != serde_json::to_value(current)? {
        return Err(Error::Conflict(
            "Application metadata changed since review; reopen its details".into(),
        ));
    }
    verify_artifacts(current)?;
    if name.trim().is_empty() || name.chars().count() > 200 || name.chars().any(|c| c.is_control())
    {
        return Err(Error::Invalid(
            "Choose an application name without control characters, up to 200 characters".into(),
        ));
    }
    let mut updated = current.clone();
    updated.name = name.to_owned();
    let mut staged = Vec::new();
    let mut removed = Vec::new();
    if let Some(icon) = icon_path {
        let bytes =
            archive::read_member(inspection, icon, crate::identity::MAX_ICON_BYTES, cancel)?;
        crate::identity::validate_icon(icon, &bytes)?;
        let extension = icon
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        let target = paths
            .icons
            .join("256x256/apps")
            .join(format!("io.github.unpack.{}.{extension}", current.id));
        if target.try_exists()? && !current.artifacts.iter().any(|a| a.path == target) {
            return Err(Error::Conflict(target.display().to_string()));
        }
        for artifact in &current.artifacts {
            if artifact.path.starts_with(&paths.icons) {
                removed.push(artifact.path.clone());
            }
        }
        updated
            .artifacts
            .retain(|a| !a.path.starts_with(&paths.icons));
        let temp = temporary_artifact(&target, &bytes)?;
        updated.artifacts.push(Artifact {
            path: target.clone(),
            sha256: security::digest(temp.path())?,
        });
        staged.push((temp, target));
    }
    for launcher in current
        .artifacts
        .iter()
        .filter(|a| a.path.extension().is_some_and(|s| s == "desktop"))
    {
        let icon = if updated
            .artifacts
            .iter()
            .any(|a| a.path.starts_with(&paths.icons))
        {
            format!("io.github.unpack.{}", current.id)
        } else {
            "application-x-executable".into()
        };
        let desktop = DesktopEntry {
            name: updated.name.clone(),
            generic_name: String::new(),
            comment: "Portable application installed with Unpack".into(),
            executable: current.executable.to_string_lossy().into_owned(),
            arguments: current.arguments.clone(),
            icon,
            terminal: current.terminal,
            categories: vec!["Utility".into()],
            startup_notify: true,
            startup_wm_class: None,
        }
        .render()?;
        let temp = temporary_artifact(&launcher.path, desktop.as_bytes())?;
        updated.artifacts.retain(|a| a.path != launcher.path);
        updated.artifacts.push(Artifact {
            path: launcher.path.clone(),
            sha256: security::digest(temp.path())?,
        });
        staged.push((temp, launcher.path.clone()));
        removed.push(launcher.path.clone());
    }
    let manifest_target = manifest_path(paths, &current.id)?;
    let manifest_temp =
        temporary_artifact(&manifest_target, &serde_json::to_vec_pretty(&updated)?)?;
    let mut tx = Transaction::new(paths, &current.id)?;
    let result = (|| {
        for path in &removed {
            cancel.check()?;
            tx.move_file(path, &backup_path(path)?)?;
        }
        tx.move_file(&manifest_target, &backup_path(&manifest_target)?)?;
        for (temp, path) in &staged {
            cancel.check()?;
            tx.move_file(temp.path(), path)?;
        }
        cancel.check()?;
        tx.move_file(manifest_temp.path(), &manifest_target)?;
        tx.commit()
    })();
    if let Err(error) = result {
        if let Err(rollback) = tx.rollback() {
            return Err(Error::Conflict(format!(
                "{error}; rollback needs recovery: {rollback}"
            )));
        }
        return Err(error);
    }
    let changes = vec![
        format!("Name: {} → {}", current.name, updated.name),
        format!(
            "Icon source: {}",
            icon_path
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "Existing icon preserved".into())
        ),
        format!("Metadata archive: {}", inspection.source.display()),
        "Application payload, executable, user configuration and documents were not changed".into(),
    ];
    history_event(paths, "Changed name and icon for", &updated, &changes).map_err(|e| {
        Error::Conflict(format!(
            "Metadata was updated, but history could not be saved: {e}"
        ))
    })?;
    Ok(updated)
}

pub fn history(paths: &Paths) -> Result<Vec<HistoryEvent>> {
    let path = paths.state.join("history.json");
    if !path.try_exists()? {
        return Ok(vec![]);
    }
    Ok(serde_json::from_reader(File::open(path)?)?)
}

/// Undo the most recent committed operation for one application. All moves are
/// reversible because updates/removals retain backups. No overwrite is allowed.
pub fn undo(paths: &Paths, id: &str) -> Result<()> {
    let _lock = Lock::acquire(paths)?;
    valid_id(id)?;
    let path = paths.state.join("transactions").join(format!("{id}.json"));
    let mut journal: Journal = serde_json::from_reader(File::open(&path)?)?;
    if !journal.committed {
        return Err(Error::Invalid("Operation needs recovery, not undo".into()));
    }
    let current = registry(paths)?.into_iter().find(|m| m.id == id);
    if let Some(m) = &current {
        verify_owned_tree(m)?;
        verify_artifacts(m)?;
    }
    // Original staging files may have been cleaned up. Give their reverse move
    // a fresh sibling recovery path instead; old backups retain exact targets.
    for movement in &mut journal.moves {
        if movement.to.try_exists()?
            && !movement
                .to
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .starts_with(".unpack-backup-")
        {
            movement.from = backup_path(&movement.to)?;
        }
    }
    journal.committed = false;
    security::atomic_json(&path, &journal)?;
    rollback_moves(&journal)?;
    std::fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_move_recovers_without_overwriting() {
        let temp = tempfile::tempdir().unwrap();
        let paths = Paths::isolated(temp.path());
        paths.initialize().unwrap();
        let original = temp.path().join("original");
        let published = temp.path().join("published");
        std::fs::write(&original, "preserved").unwrap();
        let mut tx = Transaction::new(&paths, "recovery-test").unwrap();
        tx.move_file(&original, &published).unwrap();
        assert!(!original.exists());
        drop(tx);
        assert_eq!(recover(&paths).unwrap().len(), 1);
        assert_eq!(std::fs::read_to_string(original).unwrap(), "preserved");
        assert!(!published.exists());
    }
    #[test]
    fn install_update_uninstall_undo() {
        let temp = tempfile::tempdir().unwrap();
        let paths = Paths::isolated(temp.path());
        paths.initialize().unwrap();
        let source = temp.path().join("sample");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("run"), "#!/bin/sh\necho sample\n").unwrap();
        std::fs::set_permissions(source.join("run"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        let archive = temp.path().join("sample.tar.gz");
        let cancel = Cancellation::default();
        archive::create(&[source], &archive, archive::Format::TarGz, &cancel, |_| {}).unwrap();
        let inspection =
            archive::inspect(&archive, &security::Limits::default(), &cancel, |_| {}).unwrap();
        let detected = detection::analyze(&inspection);
        let options = InstallOptions {
            id: "sample".into(),
            name: "Sample".into(),
            destination_parent: paths.default_install.clone(),
            entry_point: "sample/run".into(),
            create_launcher: true,
            install_icon: false,
            icon_path: None,
            terminal: false,
            arguments: vec![],
            profile: "Personal".into(),
            update: false,
        };
        let plan = plan(&paths, &inspection, &detected, options).unwrap();
        let manifest =
            execute(&paths, &plan, &security::Limits::default(), &cancel, |_| {}).unwrap();
        assert!(manifest.executable.exists());
        assert_eq!(registry(&paths).unwrap().len(), 1);
        assert_eq!(duplicates(&detected, &registry(&paths).unwrap()).len(), 1);
        let encoded = serde_json::to_string(&manifest).unwrap();
        let decoded: Manifest = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.source_sha256, manifest.source_sha256);
        let launcher_path = manifest
            .artifacts
            .iter()
            .find(|a| a.path.extension().and_then(|s| s.to_str()) == Some("desktop"))
            .unwrap();
        let validated = std::process::Command::new("desktop-file-validate")
            .arg(&launcher_path.path)
            .output();
        if let Ok(output) = validated {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
        assert!(super::plan(&paths, &inspection, &detected, plan.options.clone()).is_err());
        // Refresh only descriptive integration state from archive metadata.
        let renamed = refresh_identity(
            &paths,
            &manifest,
            &inspection,
            "Friendly Sample",
            None,
            &cancel,
        )
        .unwrap();
        assert_eq!(renamed.name, "Friendly Sample");
        assert_eq!(renamed.files.len(), manifest.files.len());
        let desktop = std::fs::read_to_string(&launcher_path.path).unwrap();
        assert!(desktop.contains("Name=Friendly Sample"));
        undo(&paths, "sample").unwrap();
        assert_eq!(registry(&paths).unwrap()[0].name, "Sample");
        // Keep committed journal available for undo until a new operation starts.
        recover(&paths).unwrap();
        // The archive can change while preserving application identity. Undo
        // must restore the old manifest, not move it to an abandoned temp path.
        std::fs::write(temp.path().join("sample/run"), "#!/bin/sh\necho updated\n").unwrap();
        let replacement = temp.path().join("replacement.tar.gz");
        archive::create(
            &[temp.path().join("sample")],
            &replacement,
            archive::Format::TarGz,
            &cancel,
            |_| {},
        )
        .unwrap();
        let replacement =
            archive::inspect(&replacement, &security::Limits::default(), &cancel, |_| {}).unwrap();
        let mut options = plan.options.clone();
        options.update = true;
        let update = super::plan(&paths, &replacement, &detected, options).unwrap();
        execute(
            &paths,
            &update,
            &security::Limits::default(),
            &cancel,
            |_| {},
        )
        .unwrap();
        undo(&paths, "sample").unwrap();
        assert!(manifest.executable.exists());
        assert_eq!(
            registry(&paths).unwrap()[0].source_sha256,
            manifest.source_sha256
        );
        assert!(std::fs::read_to_string(&manifest.executable)
            .unwrap()
            .contains("echo sample"));
        uninstall(&paths, &manifest, &cancel).unwrap();
        assert!(registry(&paths).unwrap().is_empty());
        undo(&paths, "sample").unwrap();
        assert_eq!(registry(&paths).unwrap().len(), 1);
        std::fs::write(manifest.destination.join("user-document"), "preserve").unwrap();
        assert!(uninstall(&paths, &manifest, &cancel).is_err());
        assert!(manifest.destination.join("user-document").exists());
    }

    #[test]
    fn selected_icon_installs_refreshes_and_undo_preserves_payload() {
        let temp = tempfile::tempdir().unwrap();
        let paths = Paths::isolated(temp.path());
        paths.initialize().unwrap();
        let source = temp.path().join("example");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("run"), "#!/bin/sh\necho app\n").unwrap();
        let xpm = "/* XPM */\nstatic char *icon[] = {\n\"2 2 1 1\",\n\". c #3584e4\",\n\"..\",\n\"..\"};\n";
        std::fs::write(source.join("example.xpm"), xpm).unwrap();
        std::fs::write(source.join("other.xpm"), xpm.replace("3584e4", "33d17a")).unwrap();
        std::fs::write(
            source.join("example.desktop"),
            "[Desktop Entry]\nName=Friendly App\nIcon=example\nExec=sh -c malicious\n",
        )
        .unwrap();
        let archive_path = temp.path().join("example.tar.gz");
        let cancel = Cancellation::default();
        archive::create(
            &[source],
            &archive_path,
            archive::Format::TarGz,
            &cancel,
            |_| {},
        )
        .unwrap();
        let inspection =
            archive::inspect(&archive_path, &security::Limits::default(), &cancel, |_| {}).unwrap();
        let detection = detection::analyze(&inspection);
        assert_eq!(detection.name, "Friendly App");
        assert_eq!(
            detection.recommended_icon.as_deref(),
            Some(Path::new("example/example.xpm"))
        );
        let options = InstallOptions {
            id: "example".into(),
            name: detection.name.clone(),
            destination_parent: paths.default_install.clone(),
            entry_point: "example/run".into(),
            create_launcher: true,
            install_icon: true,
            icon_path: Some("example/other.xpm".into()),
            terminal: false,
            arguments: vec![],
            profile: "Personal".into(),
            update: false,
        };
        let plan = plan(&paths, &inspection, &detection, options.clone()).unwrap();
        assert_eq!(
            plan.icon.as_ref().unwrap().0,
            Path::new("example/other.xpm")
        );
        let manifest =
            execute(&paths, &plan, &security::Limits::default(), &cancel, |_| {}).unwrap();
        let icon = manifest
            .artifacts
            .iter()
            .find(|a| a.path.extension().is_some_and(|s| s == "xpm"))
            .unwrap();
        assert!(std::fs::read_to_string(&icon.path)
            .unwrap()
            .contains("33d17a"));
        let before = security::digest(&manifest.executable).unwrap();
        let refreshed = refresh_identity(
            &paths,
            &manifest,
            &inspection,
            "Refreshed App",
            Some(Path::new("example/example.xpm")),
            &cancel,
        )
        .unwrap();
        assert!(std::fs::read_to_string(&icon.path)
            .unwrap()
            .contains("3584e4"));
        assert_eq!(before, security::digest(&refreshed.executable).unwrap());
        undo(&paths, "example").unwrap();
        assert!(std::fs::read_to_string(&icon.path)
            .unwrap()
            .contains("33d17a"));
        assert_eq!(registry(&paths).unwrap()[0].name, "Friendly App");
        let mut bad = options;
        bad.id = "other".into();
        bad.icon_path = Some("../outside.png".into());
        assert!(super::plan(&paths, &inspection, &detection, bad).is_err());
    }
}
