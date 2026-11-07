# Security policy and threat model

## Status

This is a development release. It has not received an independent security audit and is **not claimed production-ready**. Do not use it as a malware scanner or as a security sandbox.

There is no public vulnerability-reporting endpoint configured yet. Do not publish sensitive exploit details in unrelated issue trackers; coordinate a private reporting channel with the project maintainer before disclosure.

## Untrusted inputs

Archive data, member names, sizes, permissions, symbolic links, hard links, icons, desktop files, README contents and binary metadata are untrusted.

Inspection/extraction/installation never execute archive members, run setup scripts, interpret README commands, invoke package managers, or call `ldd` on downloaded executables. No automatic privilege escalation exists.

**Explicit launch is different:** a user-launched installed application executes with the user's normal account permissions. Unpack does not constrain what that application subsequently does.

## Implemented defenses

- Reject absolute paths, `..`, backslash separators, drive-like names, control characters, overly long names and excessive nesting.
- Reject special files, conflicting duplicate paths and non-directory archive parents.
- Validate relative symlink targets, reject escaping/cyclic link graphs and ambiguous traversals through linked parents.
- Create symlinks after regular files; materialize hard links only from regular in-archive members.
- Use descriptor-relative no-follow traversal and exclusive file creation inside extraction roots.
- Strip setuid/setgid/sticky and ownership metadata. Use 0644/0755 instead of copying privileged modes.
- Enforce positive limits for input size, actual output bytes, individual file size, entry count, depth and compression ratio. Count materialized hard-link copies too.
- Bound preview bytes per file and globally; do not render HTML, follow documentation links automatically or fetch preview resources.
- Extract from a private source snapshot matching the inspected SHA-256; never merge into existing output.
- Generate desktop commands from a user-selected absolute executable and separately encoded arguments, never a shell command. Reject control characters and percent field-code ambiguity.
- Ignore downloaded Exec/TryExec/Path/actions/environment/D-Bus instructions. Descriptive-only import is separate from executable selection.
- Import only bounded raster icon files; SVG archive icons are not installed as launchers.
- Name/icon lookup is archive-only. Desktop Exec/actions are not imported; icon hints match reviewed archive members rather than host paths or URLs. PNG/XPM headers and dimensions are bounded; PNG preview decoding uses patched system libpng on a background worker and rejects decoder failures. XPM previews depend on available runtime loaders.
- Verify managed file hashes before update/removal and refuse user-modified installations.
- Lock filesystem-changing operations; journal publish moves; preserve backups.
- Never change MIME defaults as a side effect of install or application startup.

## Assumptions and limitations

- The user account, XDG state directories and parent installation directories are trusted. A malicious process running as the same user is **not** an isolated adversary; it can alter manifests, executable files or desktop integration after verification. No-follow extraction mitigates archive-triggered link attacks but does not create a same-user security boundary.
- Final publication, inventory enumeration, transaction recovery and desktop integration still involve pathname-based operations. Race hardening against actively malicious same-user filesystem mutation needs further work.
- libarchive is native code and remains part of the attack surface. Keep it patched. Parsing currently runs in a worker thread, not a dedicated seccomp/resource-limited helper process.
- Output limits do not bound all decompressor CPU time or libarchive internal allocation. Cancellation is cooperative between decoder reads and cannot interrupt a blocked native decoder call. Sparse/decompression-heavy or malformed archives may still cause resource pressure.
- Disk-space preflight and resource-isolated parsing are not complete. ENOSPC produces an error and staging cleanup; it does not mean sufficient free space was guaranteed.
- A checksum establishes source identity/integrity, **not publisher authenticity**. Signatures and provenance UI are not implemented. Neither establishes malware absence.
- ELF/library checks are heuristic. Standard-directory presence is not a guarantee that a library has the required ABI/version or will be selected at runtime.
- Crash recovery is journal-based, not a claim of atomicity across multiple filesystems. Abrupt process death can leave private staging directories; deterministic orphan reconciliation and exhaustive power-loss simulation remain outstanding.
- Backups consume disk space and can contain executable software. They are hidden recovery artifacts, not encrypted or immutable backups.
- Uninstall refuses changed installations rather than attempting to guess which changes are safe to delete.

## Packaged editions

The AppImage runs with normal user permissions, not in a sandbox. Its bundled GTK/GIO library environment is restored to the original host values when explicitly launching managed applications or external archive managers. It bundles third-party native libraries: rebuild from an updated SDK to receive their fixes. Its build tools are pinned by SHA-256, but the packages are unsigned and have no publisher-authenticity guarantee.

The Flatpak edition has a deliberately narrower scope: archive inspection, creation and extraction, without host portable installation, host launcher creation or arbitrary host process launch. It receives Downloads access and user-selected portal grants, not unrestricted home access or host-spawn D-Bus permissions. See PACKAGING.md for exact permissions and limitations.

## Security tests

The integration suite constructs malicious TARs in memory: traversal, absolute paths, backslashes, control characters, symbolic-link escape/cycles/parents, special files, duplicate names, oversized declarations, excessive entries, hard links, changed sources and nested archives. It checks rejection, output boundaries, copy materialization, cancellation and temporary cleanup.

Run `cargo test -p unpack-core --locked`. Passing tests cover those cases; they are not proof of overall security. Fuzzing, native parser isolation, broader malicious corpus coverage and independent review are release blockers.