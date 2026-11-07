# Contributing

Read BUILDING.md, ARCHITECTURE.md and SECURITY.md first.

## Workflow

1. Keep business logic in `unpack-core`; GTK code belongs in `unpack-desktop`.
2. Prefer explicit domain types and errors. Do not suppress errors or introduce shell extraction/launch commands.
3. Add tests for the behavior and failure paths. Security fixtures should be generated, not executable payloads checked into the repository.
4. Run formatting, Clippy, all tests and desktop metadata validation.
5. Document supported behavior and limits. Never present an unfinished capability as implemented.

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Use `scripts/sdk.sh` to prefix commands when developing from the provided isolated SDK.

## Required review for filesystem changes

- What happens on cancellation, ENOSPC and process interruption?
- Can any path, symlink or hard link escape the intended scope?
- Can unrelated existing files be overwritten or removed?
- Does the installation preview match the actual plan?
- Is the manifest sufficient to reverse owned changes?
- Are user-generated changes preserved?
- Does a long operation run outside the GTK main context?

## GUI verification checklist

Test at 600, 700, 900 and 1200px widths; large text; light/dark/system themes; keyboard-only navigation; screen reader labels; multiple archives; cancellation; malformed archive errors; duplicate/update conflicts; changed-file uninstall conflicts; close while busy.

Test actual GNOME, KDE and at least one Wayland and one X11 tiling compositor before claiming cross-desktop certification. A synthetic resize smoke test does not replace those checks.

## Dependency policy

Use established codec libraries; do not implement compression. Keep the core display-independent. New dependencies must have a documented need, compatible license, and maintenance/security review. Commit Cargo.lock for reproducible application builds.