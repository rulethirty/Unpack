//! Descriptive archive metadata only. Never imports Exec, actions, environment,
//! remote icon URLs or absolute paths from downloaded desktop entries.
use crate::{
    archive::{Entry, EntryKind, Inspection},
    Error, Result,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_ICON_BYTES: u64 = 8 << 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveIdentity {
    pub name: Option<String>,
    pub name_source: Option<PathBuf>,
    pub icon: Option<PathBuf>,
}

fn desktop_values(text: &str) -> (Option<String>, Option<String>) {
    let mut main = false;
    let mut name = None;
    let mut icon = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            main = line == "[Desktop Entry]";
            continue;
        }
        if !main || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() || value.chars().any(|c| c.is_control()) {
            continue;
        }
        match key.trim() {
            "Name" if name.is_none() && value.chars().count() <= 200 => {
                // Decode only the ordinary Desktop Entry string escape set.
                let mut decoded = String::new();
                let mut chars = value.chars();
                let mut valid = true;
                while let Some(c) = chars.next() {
                    if c != '\\' {
                        decoded.push(c);
                        continue;
                    }
                    match chars.next() {
                        Some('s') => decoded.push(' '),
                        Some('\\') => decoded.push('\\'),
                        _ => {
                            valid = false;
                            break;
                        }
                    }
                }
                if valid && !decoded.trim().is_empty() {
                    name = Some(decoded);
                }
            }
            "Icon" if icon.is_none() && value.len() <= 4096 => icon = Some(value.to_owned()),
            _ => {}
        }
    }
    (name, icon)
}

pub fn supported_icon(entry: &Entry) -> bool {
    entry.kind == EntryKind::File
        && entry.size > 0
        && entry.size <= MAX_ICON_BYTES
        && entry
            .path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "png" | "xpm"))
}

pub fn detect(archive: &Inspection) -> ArchiveIdentity {
    let mut desktop_files: Vec<_> = archive
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::File && e.path.extension().is_some_and(|s| s == "desktop"))
        .collect();
    // Prefer canonical application metadata, then shallow metadata. Sorting makes
    // the result independent of archive member order.
    desktop_files.sort_by_key(|e| {
        (
            !e.path.to_string_lossy().contains("share/applications/"),
            e.path.components().count(),
            e.path.clone(),
        )
    });
    let mut identity = ArchiveIdentity {
        name: None,
        name_source: None,
        icon: None,
    };
    let mut icon_hint = None;
    let mut desktop_stem = None;
    for entry in desktop_files {
        let Ok(text) = std::str::from_utf8(&entry.preview) else {
            continue;
        };
        let (name, icon) = desktop_values(text);
        if name.is_some() {
            identity.name = name;
            identity.name_source = Some(entry.path.clone());
            desktop_stem = entry
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().to_lowercase());
            icon_hint = icon;
            break;
        }
    }
    let mut icons: Vec<_> = archive
        .entries
        .iter()
        .filter(|e| supported_icon(e))
        .collect();
    let hint = icon_hint
        .as_deref()
        .and_then(|s| Path::new(s).file_stem())
        .map(|s| s.to_string_lossy().to_lowercase());
    let app_name = identity
        .name
        .as_deref()
        .unwrap_or("")
        .to_lowercase()
        .replace(' ', "-");
    icons.sort_by_key(|e| {
        let stem = e
            .path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let path = e.path.to_string_lossy().to_lowercase();
        let exact = hint.as_ref().is_some_and(|h| *h == stem);
        let app = desktop_stem.as_ref().is_some_and(|h| *h == stem)
            || (!app_name.is_empty() && stem == app_name);
        let generic = matches!(stem.as_str(), "icon" | "logo" | "app" | "application");
        let themed = path.contains("/apps/");
        let resolution = png_dimensions(&e.preview)
            .map(|(w, h)| w.min(h).min(512))
            .unwrap_or(0);
        (
            !exact,
            !app,
            !generic,
            !themed,
            std::cmp::Reverse(resolution),
            e.path.clone(),
        )
    });
    identity.icon = icons.first().map(|e| e.path.clone());
    identity
}

pub fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

pub fn validate_icon(path: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_ICON_BYTES {
        return Err(Error::Invalid(
            "Choose an archive icon smaller than 8 MiB".into(),
        ));
    }
    match path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => {
            let (w, h) = png_dimensions(bytes).ok_or_else(|| Error::Invalid("The selected PNG has an invalid header".into()))?;
            if w > 4096 || h > 4096 || u64::from(w) * u64::from(h) > 16_777_216 { return Err(Error::Invalid("The selected icon exceeds 4096 × 4096 pixels".into())); }
        }
        "xpm" if bytes.starts_with(b"/* XPM */") => {
            let text = std::str::from_utf8(bytes).map_err(|_| Error::Invalid("XPM icon is not valid text".into()))?;
            let header = text.lines().find_map(|line| line.trim().strip_prefix('"')).ok_or_else(|| Error::Invalid("XPM icon has no dimensions".into()))?;
            let dims: Vec<u32> = header.trim_end_matches([',', '"']).split_whitespace().take(4).map(str::parse).collect::<std::result::Result<_, _>>().map_err(|_| Error::Invalid("Invalid XPM dimensions".into()))?;
            if dims.len() != 4 || dims[0] == 0 || dims[1] == 0 || dims[0] > 4096 || dims[1] > 4096 || dims[2] > 65536 || dims[3] == 0 || dims[3] > 8 {
                return Err(Error::Invalid("XPM icon exceeds supported dimension or color limits".into()));
            }
        }
        _ => return Err(Error::Unsupported("Choose a PNG or XPM from the archive. SVG can be inspected but is not imported because it can reference external resources.".into())),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(path: &str, text: &str) -> Entry {
        Entry {
            path: path.into(),
            kind: EntryKind::File,
            size: text.len() as u64,
            executable: false,
            link: None,
            sha256: None,
            preview: text.as_bytes().to_vec(),
        }
    }
    #[test]
    fn desktop_name_and_matching_icon_not_first_image() {
        let archive = Inspection { source: "download.tar".into(), sha256: String::new(), compressed_size: 1, total_size: 1, format: "tar".into(), entries: vec![
            entry("App/resources/banner.png", "other"), entry("App/share/icons/hicolor/256x256/apps/editor.png", "icon"),
            entry("App/share/applications/editor.desktop", "[Desktop Entry]\nName=Friendly\\sEditor\nIcon=/untrusted/path/editor.png\nExec=sh -c evil\n[Desktop Action other]\nName=Wrong"),
        ] };
        let identity = detect(&archive);
        assert_eq!(identity.name.as_deref(), Some("Friendly Editor"));
        assert_eq!(
            identity.icon.unwrap(),
            Path::new("App/share/icons/hicolor/256x256/apps/editor.png")
        );
        assert_eq!(
            identity.name_source.unwrap(),
            Path::new("App/share/applications/editor.desktop")
        );
    }
    #[test]
    fn invalid_desktop_fields_and_external_icons_are_not_imported() {
        let (name, _) =
            desktop_values("[Desktop Entry]\nName=Bad\\nExec=evil\nIcon=https://example/icon.png");
        assert!(name.is_none());
        assert!(validate_icon(Path::new("x.svg"), b"<svg/>").is_err());
        assert!(validate_icon(Path::new("x.png"), b"not png").is_err());
    }
}
