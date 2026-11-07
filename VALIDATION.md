# Validation record

Development validation performed on 2026-11-07, from the Fedora 44 host using a per-user GNOME 49 SDK:

- Rust 1.98.1
- GTK 4.20.4
- libadwaita 1.8.8
- libarchive 3.8.9
- Wayland / GNOME display

## Automated checks

- `cargo test --workspace --locked`: 27 tests pass (18 core unit tests, 8 archive-security integration tests, 1 desktop PNG-decoder test). Coverage includes archive name/icon selection, selected-icon installation, metadata-only refresh/undo, bounded member reads and corrupt-PNG rejection.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passes.
- `cargo fmt --all -- --check`: passes.
- Debug desktop application build: passes.
- Optimized release build: passes; stripped binary approximately 1.5 MiB (dynamically linked).
- Release-binary GUI smoke and temporary-prefix program install/remove checks pass. Repeat installation and modified-program removal are rejected.
- Desktop entry validation: passes, including Browse/Install desktop actions.
- AppStream validation: passes when the known missing-homepage tag is downgraded to informational. Strict validation reports that warning; no public project homepage has been invented.
- GUI smoke: real archive inspection, 640px adaptive main navigation, adaptive archive/installation pages, light/dark theme switching and orderly close pass with fatal GTK criticals enabled. Initial responsive failures were corrected and the test rerun.
- Generated application launcher is checked with `desktop-file-validate` when that utility is available in the test environment.
- Archive creation/read/extraction round trips cover TAR, TAR+gzip, TAR+XZ, TAR+bzip2, TAR+Zstandard, ZIP and 7z.
- Installation lifecycle test installs, rejects an unconfirmed duplicate, updates to changed payload bytes, undoes to the previous manifest/content, uninstalls, restores removal, and refuses to remove user-added data.
- Journal test simulates an interrupted publish move and restores the original without overwriting.
- Revised UI smoke checks the archive-derived friendly name and PNG preview, hidden queue navigation for a single archive, contextual queue visibility, collapsed options, Install/Files navigation, and the top Install action remaining onscreen at 640px with all details expanded.
- The same revised smoke fixture is used for AppImage and installed Flatpak package tests. PNG previews do not depend on launching a glycin image helper; libpng runs on a bounded worker input. XPM preview remains dependent on runtime loader availability.
- `scripts/smoke-install.sh` installs only its generated fixture into temporary user-level directories, verifies that the primary action becomes Open, keeps queued archives from hiding the finished app, retains Update, handles registry removal/reload, and verifies via GIO that the system desktop launcher resolves its icon to the recorded installed file. It does not execute the installed application.

## Distribution packages

AppImage and Flatpak x86_64 development bundles are generated under `dist/`; rebuild and install instructions are in PACKAGING.md. The AppImage bundles 108 SDK libraries and has a measured glibc floor of 2.39. Its host extract-and-run GUI smoke passes. The Flatpak uses GNOME 49, was installed into an isolated temporary user installation, and passed its in-sandbox GUI smoke with restricted permissions (Downloads access, display, IPC and DRI; no network, unrestricted home, or host-spawn access). Host portable installation is intentionally unavailable in Flatpak.

These are unsigned local bundles, not published/signed repositories or claims of broad distribution certification. Native FUSE mounting and older-glibc hosts are not covered by the host extract-and-run test.

Build logs are local ignored `.build-*.log` files; they are not a substitute for reproducing checks.

## Not certified

No claim is made of full manual end-to-end host desktop-menu certification, screen-reader testing, GNOME/KDE/XFCE cross-desktop testing, Hyprland/Sway/i3/River compatibility certification, exhaustive power-loss recovery, fuzzing, or independent security audit. SDK sandbox behavior does not establish host integration correctness.

The complete requested product scope is not finished. README.md lists the missing capabilities and SECURITY.md lists outstanding security hardening. This record describes a tested development release, not satisfaction of the production Definition of Done.