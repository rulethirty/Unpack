# AppImage and Flatpak packages

Both are **unsigned development packages**, version 0.1.0, for **x86_64**. They retain the feature and security limitations described in README.md and SECURITY.md. SHA-256 checksums establish file integrity, not publisher authenticity.

## Generated artifacts

Under `/home/user/Documents/Unpack/dist`:

- `Unpack-0.1.0-x86_64.AppImage` — approximately 45 MiB, with GTK, libadwaita, libarchive and supporting libraries bundled.
- `Unpack-0.1.0-x86_64.flatpak` — approximately 0.5 MiB application bundle; **GNOME 49 runtime is separate** and may require a substantial initial download.
- `SHA256SUMS` — verify from the distribution directory with `sha256sum --check SHA256SUMS`.
- `appimage-build.json` — dependency count, minimum glibc, tool URLs and pinned tool digests.

## AppImage: run without installation

```sh
chmod +x /home/user/Documents/Unpack/dist/Unpack-0.1.0-x86_64.AppImage
/home/user/Documents/Unpack/dist/Unpack-0.1.0-x86_64.AppImage
```

If FUSE mounting is unavailable:

```sh
APPIMAGE_EXTRACT_AND_RUN=1 /home/user/Documents/Unpack/dist/Unpack-0.1.0-x86_64.AppImage
```

### Compatibility

- Linux x86_64 with **glibc 2.39 or newer**. musl/Alpine and older glibc are not supported by this build.
- A functioning Wayland or X11 desktop with fonts, session D-Bus, and desktop file-dialog services.
- Libraries and icons are bundled; glibc and its loader are deliberately **not** bundled.
- Cairo rendering is the default to avoid coupling bundled GTK to host GPU drivers. You can experiment with `GSK_RENDERER=gl`, but GPU backends have not been certified across drivers.
- AppImage runs **without a sandbox**. It supports the native portable application installation workflow.
- Bundled `LD_LIBRARY_PATH`, GIO modules and schema settings are restored to the original host values for explicitly launched applications, so the AppImage's GTK libraries do not leak into those processes.
- No file associations are changed and the AppImage is not automatically added to the desktop menu.

The glibc floor is measured from versioned symbols across the binary and its 108 bundled libraries. Broader distribution testing is still needed; this is not a claim that every system with glibc 2.39 will work.

## Flatpak: install and run

On a host with Flatpak and Flathub configured:

```sh
flatpak install --user /home/user/Documents/Unpack/dist/Unpack-0.1.0-x86_64.flatpak
flatpak run io.github.unpack.Unpack
```

The bundle contains the Flathub runtime-repository URL. If GNOME 49 is not available, install the runtime explicitly:

```sh
flatpak install --user flathub org.gnome.Platform//49
```

Remove the program with:

```sh
flatpak uninstall --user io.github.unpack.Unpack
```

Application state is retained by default under `~/.var/app/io.github.unpack.Unpack`. Add `--delete-data` only if you explicitly want to remove its private settings and data too. Extracted files in Downloads or user-selected folders are not its private state.

### Deliberate sandbox limitations

This Flatpak provides **archive browsing, inspection, creation and extraction**. It does **not** install portable applications onto the host, create host application launchers, launch arbitrary host binaries, or change MIME defaults. Those workflows require the native/AppImage edition. Both the UI and installation core enforce this distinction rather than claiming in-sandbox launchers work on the host.

Permissions:

- Wayland and fallback X11 display access.
- IPC and DRI graphics access.
- Read/write access to the standard Downloads directory.
- Other files and extraction folders through user-mediated file chooser/document portal grants.
- No network permission, unrestricted home/host filesystem access, host-spawn D-Bus permission, full session bus, or system bus access.

Flatpak exposes a default sandbox seccomp filter. Linux `renameat2(RENAME_NOREPLACE)` support inside the runtime must be preserved because extraction never overwrites an existing destination.

The package's `.desktop` actions still accept `--install` for compatibility, but attempting installation explains the restriction. External archive-manager launching uses the built-in browser in this sandbox.

This is not a Flathub submission or signed update repository. Installing the local bundle does not configure automatic Unpack updates.

## Rebuild from this workspace

Build the executable in GNOME SDK 49 so its ABI matches the Flatpak runtime:

```sh
sh /home/user/Documents/Unpack/scripts/sdk.sh cargo build --release --locked
sh /home/user/Documents/Unpack/scripts/sdk.sh sh scripts/package-appimage.sh
sh /home/user/Documents/Unpack/scripts/package-flatpak.sh
```

The Flatpak script uses standard `flatpak build-init`, `build-finish`, `build-export` and `build-bundle`; it does not need root or flatpak-builder. The alternative prebuilt-binary builder manifest is at `/home/user/Documents/Unpack/packaging/flatpak/io.github.unpack.Unpack.json`; build the release executable first. It is not a source-only Flathub manifest.

The AppImage builder downloads upstream appimagetool and type-2 runtime assets only after checking pinned SHA-256 hashes in `packaging/appimage/tools.json`. Upstream's `continuous` URLs can change; if the hash changes the build stops. Review a new upstream release before updating pins. This is integrity pinning, not an independent authenticity/signature check.

SDK library license texts are included in the AppImage, with a bundled-library inventory. The dependency versions inherit the SDK's maintenance status; rebuild packages to receive library security fixes. No bit-for-bit reproducibility or GPG signing is claimed.

## Package validation

```sh
sh /home/user/Documents/Unpack/scripts/test-appimage.sh
sh /home/user/Documents/Unpack/scripts/test-flatpak.sh
```

Run the AppImage test on the host (from a sandboxed VS Code terminal, prefix it with `flatpak-spawn --host`). It exercises extract-and-run, real archive inspection, narrow responsive layouts and themes. The Flatpak test installs into a temporary isolated user installation, uses the host's existing GNOME runtime, prints the actual permissions, runs an in-sandbox GUI smoke test, and removes the test deployment. Neither test changes default archive associations.