# Architecture

## Workspace

```text
crates/core/src/
  archive.rs        libarchive streaming reader/writer, inspection and extraction
  security.rs       path/link validation, no-follow file access, limits, hashes
  detection.rs      evidence-based application classification and text previews
  identity.rs       archive-only descriptive name/icon detection and validation
  compatibility.rs bounded static ELF metadata inspection
  desktop.rs        launcher generation and descriptive-only metadata import
  config.rs         XDG paths, versioned preferences and migration
  installer.rs      plans, manifests, locking, transaction journal, registry/history
crates/desktop/src/
  main.rs           GApplication lifecycle and file-open dispatch
  ui/mod.rs         window/navigation, job messages, file dialogs and queue
  ui/archive_view.rs archive browser and reviewed install/extraction workflow
  ui/installation_view.rs compact app card, fixed install action, expandable options
  ui/identity_editor.rs archive-only identity refresh for installed applications
  ui/icon_decode.rs bounded PNG decoding with system libpng on background workers
  ui/manager.rs     application list, details, dependency inspection and removal
  ui/settings.rs    preferences and optional onboarding
```

The core returns typed `Result` errors and serializable domain types. GTK objects never enter worker threads. Background jobs communicate through an async channel; only the main context updates widgets. Progress is throttled to avoid flooding the event loop. Long archive reads, extraction, hashing, planning of updates, installation, removal and dependency inspection use workers.

## Archive pipeline

1. Open with libarchive's content-detecting reader.
2. Validate every pathname, type, declared size and link target.
3. Stream data through actual-output counters and hashes, with cooperative cancellation.
4. Retain bounded previews, not entire archive contents. Preview memory has a global budget.
5. Validate tree conflicts, link chains/cycles and materialized hard-link sizes.
6. Return an inspection object and source SHA-256.

Extraction copies the input into a private snapshot, verifies that it matches the reviewed hash, and decodes that snapshot into a private sibling staging directory. Every file creation uses `openat` with `O_EXCL | O_NOFOLLOW`; each parent directory is opened with `O_DIRECTORY | O_NOFOLLOW`. Links are created only after regular files. Validated hard links are materialized as independent copies. A completed directory is published with Linux `renameat2(RENAME_NOREPLACE)`; existing directories are never merged or replaced by extraction.

The application never passes arbitrary archive filenames to a shell, invokes external extraction tools, or executes downloaded code during inspection/extraction.

## Detection

Detection is a heuristic, not a verdict. It combines entry-point candidates, resource/library structures, AppRun, source-build markers, desktop metadata and icons. Evidence and confidence are retained separately from classification. Installer scripts and AppImages cannot become automatic application entry points. The user can select regular files manually for archives lacking Unix executable permission metadata.

## Installation

Planning performs no installation. It checks IDs, entry points, destinations, conflicts and existing ownership records, then generates the precise launcher contents and operation list for confirmation.

Execution:

1. Acquire a per-user advisory operation lock.
2. Recheck conflicts and update ownership.
3. Extract into private staging and inventory actual files.
4. Stage icons, launcher and manifest on their target filesystems.
5. Journal each rename **before** performing it.
6. Move prior managed state to unique sibling backups for explicit updates.
7. Publish payload, desktop artifacts and manifest.
8. Mark the journal committed and append operation history.

There is no cross-filesystem atomic transaction. The durable journal makes rename operations reversible; startup recovery reverses incomplete operations. Committed records and backups permit limited undo. Recovery refuses to overwrite conflicting files.

## Ownership and removal

A versioned manifest records file type, relative path, regular-file SHA-256, link target, executable, destination and desktop artifacts. Registry loading validates manifest scope and identifiers.

Updates and removals inventory the actual tree. New, missing or modified files stop the operation. Uninstallation currently moves the unchanged installation and owned artifacts into recoverable backups rather than recursively deleting them. Unrelated user data is never selected for deletion.

This conservative behavior sacrifices convenience to prevent accidental data loss. A future partial-uninstall workflow needs explicit conflict UI and descriptor-relative removal of individual verified members.

## Persistence

- `$XDG_CONFIG_HOME/unpack/settings.json`
- `$XDG_DATA_HOME/unpack/manifests/<id>.json`
- `$XDG_STATE_HOME/unpack/history.json`
- `$XDG_STATE_HOME/unpack/transactions/<id>.json`
- `$XDG_STATE_HOME/unpack/transaction-records/`
- `$XDG_DATA_HOME/applications/io.github.unpack.<id>.desktop`
- `$XDG_DATA_HOME/icons/hicolor/256x256/apps/io.github.unpack.<id>.<ext>`

Missing XDG variables fall back to the specification's home-relative defaults. Non-absolute XDG variables are ignored. Configuration schema zero migrates to schema one; future schemas are rejected. JSON writes use synced same-directory temporary files and rename.

## Native UX

Adwaita controls supply theme integration, focus behavior and accessibility roles. An adaptive sidebar collapses below 850 scale-independent pixels. Archive content uses explicit Install/Files tabs at every width, avoiding width-dependent buttons that appear to do nothing. The app summary and large Install action stay outside the scrolling options. Name/icon, destination, launch settings, advanced/update controls and full inspection details are expandable; ambiguous entry-point selection opens automatically. Queue navigation is visible only when another archive is queued. A 600px minimum window width is intended to remain usable under tiling compositors.

Archive identity reads Name/Icon hints only from the main Desktop Entry group. Icon hints match regular in-archive raster members, never remote URLs or host files. PNG previews are byte/dimension bounded and decoded by libpng's simplified API off the main thread, then transferred as RGBA for a GTK memory texture. XPM integration remains available; previews depend on the runtime's XPM loader. SVG is not imported. Metadata-only refresh uses the same lock, artifact hashes, journal and backups as installation, but leaves application payload and user data unchanged.

No GNOME Shell service is required for core functions. GIO handles file dialogs, MIME handler discovery and directory opening. Launching a managed app is a separate explicit user action without a shell.

## Extension priorities

Before production release: stronger crash-injection coverage, descriptor-relative publication and recovery hardening, virtualized archive tree, archive editing, keep-portable registration, advanced launcher settings, bounded dependency reads, checksum/signature verification UI, backup lifecycle management, localization, assistive-technology testing and compositor/desktop matrix validation.