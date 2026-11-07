# Unpack

**Archives, made approachable.** A native GTK4/libadwaita archive browser and user-level portable application manager, written in Rust.

> Development release, not yet production-certified. The implemented workflow is real; the remaining product scope and validation gaps are listed below. Structural inspection does not establish that downloaded software is trustworthy.

## What works

- Read TAR, gzip/XZ/bzip2/Zstandard-compressed TAR, ZIP and 7z through libarchive, with content-based decoding rather than trusting extensions.
- Browse archive directories, search, show hidden files, inspect sizes and executable indicators, and preview bounded UTF-8 text/Markdown source.
- Extract into a **new** directory with progress, cancellation, path validation, no-follow writes, extraction limits, and cleanup on failure.
- Create archives in those formats using the core API; choose multiple regular files in the GUI.
- Classify portable applications, AppDirs, binary distributions, source trees, installer distributions and AppImage-related archives, with evidence and uncertainty.
- Statically inspect ELF architecture, program type, interpreter, dependency names and visible GLIBC version references. Never run `ldd` on downloaded code.
- Select an entry point, name, ID and user-level destination; review the actual installation plan; explicitly confirm installation.
- Install a complete archive payload, generate a sanitized desktop launcher, import raster icons, and record every managed file.
- Detect application names from archive `.desktop` metadata, suggest matching archive icons, preview the selected PNG, and override either before installation. No internet lookup is used.
- Refresh an installed application's name and icon from an archive without replacing its payload; integration changes are journaled, recorded and undoable.
- List installed applications, launch explicitly, inspect manifests and dependencies, update from another archive, remove unchanged managed installations, and undo the most recent retained operation.
- Retain recoverable backups for updates/removals; recover journaled interrupted filesystem moves at startup.
- Persist preferences, window size, manifests and history in XDG directories.
- Use native file dialogs, keyboard shortcuts, optional onboarding, system/light/dark appearance, responsive navigation and background workers.
- Open multiple archives as an inspection queue; every installation still needs its own confirmation.
- Discover other MIME handlers without assuming File Roller or GNOME services are installed. Never change MIME defaults silently.

## Build and run

See [BUILDING.md](BUILDING.md) for native packages and the isolated GNOME SDK environment.

```sh
cargo build --release --locked
cargo run -p unpack-desktop -- /path/to/download.tar.gz
```

From this workspace's VS Code Flatpak terminal:

```sh
sh /home/user/Documents/Unpack/scripts/sdk.sh cargo run -p unpack-desktop
```

## Typical installation

1. Open a downloaded application archive.
2. Choose **Inspect Application**. The **Install** tab shows a compact app card and a large **Install…** button at the top. **Browse Archive** opens the **Files** tab instead.
3. Open **Name & Icon** to review the archive-detected identity or choose another icon. **Launch Settings** opens automatically when an entry point needs your attention.
4. Keep the default location, or expand **Install Location** to change it. **Advanced & Updates** contains the application ID and explicit update option. All evidence, compatibility findings, warnings and checksums remain in **Archive Details & Compatibility**.
5. Click the top **Install…** button and confirm the summary. **Show All Changes & Warnings** reveals the complete generated plan.
6. Find the application in your desktop menu or Unpack's **Installed Applications** section.

The full archive structure is retained. For `example/bin/example`, the entry point becomes `~/.local/opt/example-id/example/bin/example`. Unpack does not guess which bundled libraries can be discarded.

**Next Archive** appears only when another archive is actually queued; it is absent for a single archive. It does not install anything. Name/icon detection reads only archive files; downloaded Exec fields and icon paths never become launch commands or host file reads.

PNG icons have a built-in system-libpng preview path. XPM icons can be installed, but their preview depends on the runtime's decoder (some newer GNOME runtimes omit it). If a preview cannot load, Unpack explains the failure and uses a neutral icon. SVG files remain inspectable but are not imported.

For an installed application, open **Installed Applications → Details → Manage Application → Name & Icon from Archive…**. Select an archive, review the detected name/icon, and apply. The installed list and generated desktop launcher use the updated identity; application files and user data remain unchanged.

### Updating

Open the replacement archive, use the installed application's exact ID and parent directory, select **Update existing installation**, and review the new plan. Updates stop if managed files changed or user-added files are present. Backups are hidden `.unpack-backup-*` siblings; they consume disk space and are not silently discarded.

### Removing and undoing

Use **Installed Applications → Details → Manage Application → Uninstall**. Files outside the manifest scope are preserved. Changed installations are not partially destroyed. A recoverable backup remains; **Undo Last Operation** can restore the most recent retained operation. History includes removed applications and offers operation details.

## Security model

Read [SECURITY.md](SECURITY.md) before processing untrusted software. Inspection and installation never execute archive contents. Launching an installed application is an explicit user action and runs it with your account's normal permissions, **not in a sandbox**.

## Current limitations and remaining scope

These are not hidden behind nonfunctional buttons:

- No system-wide/elevated installation. `/opt`, `/usr/local` and installation scripts are explained but not supported as managed destinations.
- AppImage distributions are detected but not installed; mounting/extracting AppImages and FUSE-specific workflows require a separate implementation.
- No password-protected archive workflow or automatic nested archive extraction.
- No in-place archive add/remove/rename interface yet; creation and safe extraction are available. Symbolic links are not accepted by archive creation.
- No selected-subtree installation: installation deliberately retains the full application payload.
- README Markdown is previewed as text, without active HTML or external resources.
- The archive browser bounds displayed rows to 2,000; search narrows large inventories. A virtualized tree model is needed for larger interactive listings.
- Install and Files use explicit tabs at all window sizes, with collapsible details and a fixed top installation action. Further usability and compositor testing remains necessary.
- No keep-in-place portable launcher registration, profile editor, advanced launcher editor, arbitrary environment overrides, GPG verification UI, checksum-comparison UI, or one-click MIME-default registration yet.
- No automatic update discovery or package-manager operations.
- ELF inspection is best-effort. Archive previews are bounded; dynamic metadata outside that range can be incomplete. Dependency checks do not reproduce the dynamic linker's entire search algorithm.
- Undo is local recovery, not a durable backup system. Backup cleanup and an all-operations recovery browser remain to be implemented.
- GTK accessibility semantics are used, but screen-reader, multiple desktop environment, real tiling compositor, and full interactive workflow certification remain outstanding.
- AppImage and Flatpak development packages are available; see [PACKAGING.md](PACKAGING.md). The AppImage supports native installation and requires glibc 2.39+. The Flatpak is restricted to archive functions and does not install applications or launchers onto the host.

See [ARCHITECTURE.md](ARCHITECTURE.md), [CONTRIBUTING.md](CONTRIBUTING.md) and the tests for design details.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE). AppStream metadata is CC0-1.0.
