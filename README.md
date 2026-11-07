<p align="center">
  <img src="data/icons/io.github.unpack.Unpack.svg" width="112" height="112" alt="Unpack application icon">
</p>

<h1 align="center">Unpack</h1>

<p align="center">
  <strong>Archives, made approachable.</strong><br>
  Explore downloads. Install portable apps. Keep everything together.
</p>

<p align="center">
  <a href="#download--install">Download & Install</a> ·
  <a href="#features">Features</a> ·
  <a href="BUILDING.md">Build from Source</a> ·
  <a href="CONTRIBUTING.md">Contribute</a>
</p>

---

Unpack is a native Linux archive manager and portable application installer built with **Rust, GTK4, and libadwaita**. Turn a downloaded archive into an application you can find, launch, update, and remove—without manually moving files or writing desktop shortcuts.

## Download & Install

**Use the Releases button on this repository’s GitHub page to download the latest published build.** Open the release, expand **Assets**, and choose the package for your system. You do not need to build Unpack yourself.

| Package | Best for | Requirements |
| --- | --- | --- |
| **AppImage — recommended** | Archive tools **and portable application installation** | Linux x86_64, glibc **2.39+** |
| **Flatpak** | Sandboxed archive browsing, inspection, creation, and extraction | Linux x86_64, Flatpak; GNOME 49 runtime |

> **Choose AppImage to install portable apps.** The Flatpak edition deliberately cannot install applications or register launchers on the host.

### AppImage

Download `Unpack-0.1.0-x86_64.AppImage` from the release assets. In your file manager, open **Properties → Permissions**, allow execution, then double-click it.

If FUSE is unavailable, run the downloaded AppImage with `APPIMAGE_EXTRACT_AND_RUN=1`. See [packaging instructions](PACKAGING.md) for commands and compatibility details.

### Flatpak

Download `Unpack-0.1.0-x86_64.flatpak` from the release assets and open it with your software installer. Alternatively, from the folder containing the download:

```sh
flatpak install --user ./Unpack-0.1.0-x86_64.flatpak
flatpak run io.github.unpack.Unpack
```

The GNOME runtime is a separate download. This is a local bundle, not a Flathub listing or an automatic update channel.

**Release status:** current packages are unsigned development builds, not production-certified releases. Download `SHA256SUMS` alongside the packages to verify integrity; checksums do not establish publisher authenticity.

## Features

### Explore before you install

- Browse archive folders, search files, and preview text and installation instructions.
- Inspect executable candidates, application structure, and compatibility findings without running downloaded code.
- Create and extract **TAR, TAR.GZ, TAR.XZ, TAR.BZ2, TAR.ZST, ZIP, and 7z** archives.

### A simpler installation experience

- A compact application card and a large **Install…** button at the top.
- Names and matching icons detected **from the archive**, never fetched from the internet.
- Expandable controls for name and icon, installation location, launcher settings, updates, and technical details.
- A clear confirmation showing what will change before installation begins.
- User-level installation by default—no administrator privileges required.

### Your applications, in one place

- Launch apps, open their folders, inspect manifests, and check dependencies.
- Update from a replacement archive with recoverable backups.
- Refresh an installed app’s name and icon without replacing its files.
- Remove unchanged managed installations and undo the most recent retained operation.
- Review installation history while preserving configuration and documents outside the managed installation.

### At home on Linux

Native file dialogs, keyboard shortcuts, system/light/dark themes, responsive navigation, cancellable background operations, and no automatic changes to your default archive handler.

## From download to application

1. **Open an archive** and choose **Inspect Application**.
2. **Review the app card.** Open **Name & Icon** or **Launch Settings** if you want to change the detected details.
3. **Click Install…** at the top, review the summary, and confirm.
4. **Find your app** in the application menu and Unpack’s **Installed Applications** section.

After installation, **Install…** becomes **Open** so you can launch the app immediately. The selected archive icon is installed with its launcher and referenced directly, so your system’s application menu does not have to wait for an icon-theme cache refresh. Keep **Show in the application menu** and **Use this icon for the installed application** enabled to create both. If an older installation has a generic icon, use **Manage Application → Name & Icon from Archive…** to refresh it.

Prefer to browse? Switch to **Files** to inspect or extract the archive. Advanced details stay available without getting in your way. **Next Archive** appears only when another archive is actually queued.

## Safety and transparency

Downloaded archives are untrusted. Unpack validates paths and links, enforces extraction limits, stages installations, records managed files, and refuses to overwrite unrelated or modified files. Installation scripts and commands in documentation are **never executed automatically**.

**Inspection is not malware detection.** Explicitly launched applications run with your account’s permissions; the native/AppImage installer does not sandbox them. Install software only from sources you trust.

Read the [security model and limitations](SECURITY.md) and [validation record](VALIDATION.md).

<details>
<summary><strong>Current limitations</strong></summary>

- No elevated/system-wide installation, AppImage installation workflow, or automatic execution of installers.
- No password-protected archive workflow, recursive nested extraction, or in-place archive add/remove/rename interface.
- Installation retains the complete archive payload; selected-subtree and keep-in-place portable registration are not implemented.
- PNG icons are previewed directly; XPM previews depend on runtime decoder support. SVG files are inspectable but are not imported as launcher icons.
- Markdown is previewed as text. Archive listings display up to 2,000 rows; search narrows larger archives.
- Compatibility checks are best-effort, not a guarantee that an app will run.
- No automatic update discovery, signature-verification UI, profile editor, advanced launcher editor, or one-click MIME-default registration.
- Backups consume disk space; backup lifecycle management remains unfinished. Undo requires retained state.
- Independent security review, exhaustive crash testing, accessibility certification, and broad desktop/compositor testing remain outstanding.

</details>

## Development

For dependencies and setup, see [BUILDING.md](BUILDING.md).

```sh
cargo build --release --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

| Documentation | Contents |
| --- | --- |
| [Building](BUILDING.md) | Development setup and native installation |
| [Packaging](PACKAGING.md) | AppImage/Flatpak usage, rebuilding, and restrictions |
| [Architecture](ARCHITECTURE.md) | Core modules, transactions, and interface design |
| [Security](SECURITY.md) | Threat model, protections, and known limitations |
| [Contributing](CONTRIBUTING.md) | Development workflow and review checklist |

## License

**GPL-3.0-or-later.** AppStream metadata is CC0-1.0.