//! Deliberately malformed TAR fixtures are generated in memory, not checked in
//! as executable or potentially dangerous files.
use std::{io::Write, path::Path};
use unpack_core::{archive, security::Limits, Cancellation};

type TarMember<'a> = (&'a str, u8, &'a str, &'a [u8], Option<u64>);
fn tar(entries: &[TarMember<'_>]) -> Vec<u8> {
    let mut result = Vec::new();
    for (name, kind, link, contents, declared) in entries {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..108].copy_from_slice(b"0000755\0");
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        let size = format!("{:011o}\0", declared.unwrap_or(contents.len() as u64));
        header[124..136].copy_from_slice(size.as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[148..156].fill(b' ');
        header[156] = *kind;
        header[157..157 + link.len()].copy_from_slice(link.as_bytes());
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        let checksum: u64 = header.iter().map(|b| *b as u64).sum();
        let checksum = format!("{checksum:06o}\0 ");
        header[148..156].copy_from_slice(checksum.as_bytes());
        result.extend_from_slice(&header);
        result.extend_from_slice(contents);
        result.resize(result.len().div_ceil(512) * 512, 0);
    }
    result.resize(result.len() + 1024, 0);
    result
}
fn inspect_bytes(bytes: &[u8], limits: &Limits) -> unpack_core::Result<archive::Inspection> {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    archive::inspect(file.path(), limits, &Cancellation::default(), |_| {})
}

#[test]
fn traversal_absolute_and_malformed_names_are_rejected() {
    for path in [
        "../outside",
        "/etc/passwd",
        "app/../../outside",
        "C:\\outside",
        "app\\evil",
        "app\n.desktop",
    ] {
        assert!(
            inspect_bytes(&tar(&[(path, b'0', "", b"bad", None)]), &Limits::default()).is_err(),
            "{path}"
        );
    }
}
#[test]
fn escaping_cyclic_and_parent_symlinks_are_rejected() {
    for entries in [
        vec![("link", b'2', "../outside", &b""[..], None)],
        vec![
            ("a", b'2', "b", &b""[..], None),
            ("b", b'2', "a", &b""[..], None),
        ],
        vec![
            ("a", b'2', "b", &b""[..], None),
            ("a/pwned", b'0', "", &b"bad"[..], None),
        ],
    ] {
        assert!(inspect_bytes(&tar(&entries), &Limits::default()).is_err());
    }
}
#[test]
fn devices_fifos_duplicate_paths_and_size_bombs_are_rejected() {
    for kind in *b"346" {
        assert!(inspect_bytes(
            &tar(&[("special", kind, "", b"", None)]),
            &Limits::default()
        )
        .is_err());
    }
    assert!(inspect_bytes(
        &tar(&[("x", b'0', "", b"one", None), ("x", b'0', "", b"two", None)]),
        &Limits::default()
    )
    .is_err());
    assert!(inspect_bytes(
        &tar(&[("bomb", b'0', "", b"", Some(5 << 30))]),
        &Limits::default()
    )
    .is_err());
    let limits = Limits {
        max_entries: 1,
        ..Limits::default()
    };
    assert!(inspect_bytes(
        &tar(&[("a", b'0', "", b"a", None), ("b", b'0', "", b"b", None)]),
        &limits
    )
    .is_err());
    assert!(inspect_bytes(b"not an archive", &Limits::default()).is_err());
}
#[test]
fn hardlinks_are_materialized_and_counted_against_limits() {
    let bytes = tar(&[
        ("run", b'0', "", b"hello", None),
        ("other", b'1', "run", b"", None),
    ]);
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(&bytes).unwrap();
    let token = Cancellation::default();
    let inspected = archive::inspect(file.path(), &Limits::default(), &token, |_| {}).unwrap();
    assert_eq!(inspected.total_size, 10);
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("out");
    archive::extract(&inspected, &out, &Limits::default(), &token, |_| {}).unwrap();
    assert_eq!(std::fs::read(out.join("other")).unwrap(), b"hello");
    use std::os::unix::fs::MetadataExt;
    assert_ne!(
        std::fs::metadata(out.join("run")).unwrap().ino(),
        std::fs::metadata(out.join("other")).unwrap().ino()
    );
    let limits = Limits {
        max_total_bytes: 8,
        ..Limits::default()
    };
    assert!(inspect_bytes(&bytes, &limits).is_err());
    assert!(inspect_bytes(
        &tar(&[("bad", b'1', "../outside", b"", None)]),
        &Limits::default()
    )
    .is_err());
}
#[test]
fn archive_changes_and_cancellation_leave_no_output() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(&tar(&[("safe", b'0', "", b"safe", None)]))
        .unwrap();
    let token = Cancellation::default();
    let inspected = archive::inspect(file.path(), &Limits::default(), &token, |_| {}).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("out");
    std::fs::write(file.path(), tar(&[("evil", b'0', "", b"evil", None)])).unwrap();
    assert!(archive::extract(&inspected, &out, &Limits::default(), &token, |_| {}).is_err());
    assert!(!out.exists());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    token.cancel();
    assert!(archive::inspect(file.path(), &Limits::default(), &token, |_| {}).is_err());
}
#[test]
fn nested_archives_are_not_recursively_extracted() {
    let nested = tar(&[("../escape", b'0', "", b"bad", None)]);
    let bytes = tar(&[("inner.tar", b'0', "", &nested, None)]);
    let inspection = inspect_bytes(&bytes, &Limits::default()).unwrap();
    assert_eq!(inspection.entries.len(), 1);
    assert_eq!(inspection.entries[0].path, Path::new("inner.tar"));
}

#[test]
fn safe_internal_symlink_extracts_and_root_headers_are_counted() {
    let bytes = tar(&[
        ("app/run", b'0', "", b"safe", None),
        ("app/link", b'2', "run", b"", None),
    ]);
    let mut archive_file = tempfile::NamedTempFile::new().unwrap();
    archive_file.write_all(&bytes).unwrap();
    let token = Cancellation::default();
    let inspection =
        archive::inspect(archive_file.path(), &Limits::default(), &token, |_| {}).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("out");
    archive::extract(&inspection, &out, &Limits::default(), &token, |_| {}).unwrap();
    assert_eq!(
        std::fs::read_link(out.join("app/link")).unwrap(),
        Path::new("run")
    );
    assert_eq!(
        std::fs::read_to_string(out.join("app/link")).unwrap(),
        "safe"
    );
    let roots = tar(&[
        ("./", b'5', "", b"", None),
        ("./", b'5', "", b"", None),
        ("file", b'0', "", b"data", None),
    ]);
    assert!(inspect_bytes(
        &roots,
        &Limits {
            max_entries: 2,
            ..Limits::default()
        }
    )
    .is_err());
}

#[test]
fn cancellation_during_extraction_cleans_staging() {
    let payload = vec![b'x'; 256 * 1024];
    let bytes = tar(&[("large", b'0', "", &payload, None)]);
    let mut archive_file = tempfile::NamedTempFile::new().unwrap();
    archive_file.write_all(&bytes).unwrap();
    let token = Cancellation::default();
    let inspection =
        archive::inspect(archive_file.path(), &Limits::default(), &token, |_| {}).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("out");
    let trigger = token.clone();
    assert!(
        archive::extract(&inspection, &out, &Limits::default(), &token, |_| trigger
            .cancel())
        .is_err()
    );
    assert!(!out.exists());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}
