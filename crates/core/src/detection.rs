use crate::{
    archive::{EntryKind, Inspection},
    compatibility::{self, ElfInfo},
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplicationKind {
    Portable,
    AppDir,
    AppImageDistribution,
    PrecompiledBinaries,
    SourceCode,
    SystemFilesystem,
    InstallerBased,
    Generic,
    Ambiguous,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub path: PathBuf,
    pub script: bool,
    pub elf: Option<ElfInfo>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub kind: ApplicationKind,
    pub confidence: String,
    pub name: String,
    pub name_source: Option<PathBuf>,
    pub recommended_icon: Option<PathBuf>,
    pub suggested_id: String,
    pub evidence: Vec<String>,
    pub warnings: Vec<String>,
    pub candidates: Vec<Candidate>,
    pub recommended: Option<PathBuf>,
    pub readmes: Vec<PathBuf>,
    pub icons: Vec<PathBuf>,
    pub desktop_entries: Vec<PathBuf>,
}
impl ApplicationKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Portable => "Portable application",
            Self::AppDir => "AppDir application",
            Self::AppImageDistribution => "AppImage distribution",
            Self::PrecompiledBinaries => "Precompiled binaries",
            Self::SourceCode => "Source code",
            Self::SystemFilesystem => "System filesystem archive",
            Self::InstallerBased => "Installer-based distribution",
            Self::Generic => "General archive",
            Self::Ambiguous => "Ambiguous application structure",
        }
    }
}
pub fn application_id(name: &str) -> String {
    let id: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let id = id
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let id: String = id.chars().take(80).collect();
    if id.is_empty() {
        "application".into()
    } else {
        id
    }
}

pub fn analyze(archive: &Inspection) -> Detection {
    let top: Vec<_> = archive
        .entries
        .iter()
        .filter_map(|e| e.path.components().next().map(|c| c.as_os_str().to_owned()))
        .collect();
    let common_root = top.first().filter(|first| top.iter().all(|p| p == *first));
    let name = common_root
        .and_then(|n| n.to_str())
        .map(String::from)
        .unwrap_or_else(|| {
            archive
                .source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .split('.')
                .next()
                .unwrap_or("Application")
                .to_string()
        });
    let identity = crate::identity::detect(archive);
    let name = identity.name.unwrap_or(name);
    let mut d = Detection { kind: ApplicationKind::Generic, confidence: "Low".into(), suggested_id: application_id(&name), name, name_source: identity.name_source, recommended_icon: identity.icon,
        evidence: vec![], warnings: vec!["Structural inspection is not malware detection. Only install software from sources you trust.".into()], candidates: vec![], recommended: None, readmes: vec![], icons: vec![], desktop_entries: vec![] };
    let mut app_run = None;
    let mut libraries = false;
    let mut resources = false;
    let mut source = false;
    let mut installer = false;
    let mut appimage = false;
    let mut system = false;
    for entry in &archive.entries {
        let file_name = entry.path.file_name().unwrap_or_default().to_string_lossy();
        let lower = file_name.to_ascii_lowercase();
        let components: Vec<_> = entry.path.iter().filter_map(|s| s.to_str()).collect();
        libraries |=
            components.iter().any(|c| matches!(*c, "lib" | "lib64")) || lower.contains(".so");
        resources |= components
            .iter()
            .any(|c| matches!(*c, "resources" | "share" | "assets" | "locales"));
        source |= matches!(
            lower.as_str(),
            "cargo.toml" | "cmakelists.txt" | "configure.ac" | "meson.build" | "makefile.am"
        );
        system |= matches!(
            components.first(),
            Some(&"etc") | Some(&"boot") | Some(&"var")
        );
        if entry.kind != EntryKind::File {
            continue;
        }
        if lower.starts_with("readme") || lower.starts_with("install.") || lower == "install" {
            d.readmes.push(entry.path.clone());
        }
        if lower.ends_with(".desktop") {
            d.desktop_entries.push(entry.path.clone());
        }
        if matches!(
            entry.path.extension().and_then(|e| e.to_str()),
            Some("png" | "svg" | "xpm")
        ) {
            d.icons.push(entry.path.clone());
        }
        if lower.ends_with(".appimage")
            || entry
                .preview
                .get(8..11)
                .is_some_and(|m| m == b"AI\x01" || m == b"AI\x02")
        {
            appimage = true;
            continue;
        }
        let script = entry.preview.starts_with(b"#!");
        let elf = compatibility::inspect(&entry.preview);
        let is_installer = matches!(
            lower.as_str(),
            "install.sh" | "setup.sh" | "installer.sh" | "uninstall.sh"
        );
        installer |= is_installer;
        if is_installer {
            d.warnings.push(format!(
                "{} is an installation script. Unpack will not execute it.",
                entry.path.display()
            ));
            continue;
        }
        let shared = lower.contains(".so") || lower.starts_with("ld-linux");
        if (elf.is_some() || entry.executable || file_name == "AppRun") && !shared {
            if file_name == "AppRun" {
                app_run = Some(entry.path.clone());
            }
            d.candidates.push(Candidate {
                path: entry.path.clone(),
                script,
                elf,
            });
        }
    }
    if !d.candidates.is_empty() {
        d.evidence.push(format!(
            "{} possible application entry point(s)",
            d.candidates.len()
        ));
    }
    if libraries {
        d.evidence.push("Bundled library structure detected".into());
    }
    if resources {
        d.evidence.push("Application resources detected".into());
    }
    if !d.icons.is_empty() {
        d.evidence.push("Application icon candidates found".into());
    }
    if !d.desktop_entries.is_empty() {
        d.evidence
            .push("Desktop metadata found (not trusted as launch commands)".into());
    }
    if !d.readmes.is_empty() {
        d.evidence
            .push("Installation instructions or README available for preview".into());
    }
    d.kind = if appimage {
        ApplicationKind::AppImageDistribution
    } else if system {
        ApplicationKind::SystemFilesystem
    } else if source {
        ApplicationKind::SourceCode
    } else if app_run.is_some() && (resources || !d.desktop_entries.is_empty()) {
        ApplicationKind::AppDir
    } else if !d.candidates.is_empty()
        && (libraries || resources)
        && (!d.icons.is_empty() || !d.desktop_entries.is_empty())
    {
        ApplicationKind::Portable
    } else if !d.candidates.is_empty() {
        ApplicationKind::PrecompiledBinaries
    } else if installer {
        ApplicationKind::InstallerBased
    } else {
        ApplicationKind::Generic
    };
    if matches!(d.kind, ApplicationKind::AppDir | ApplicationKind::Portable) {
        d.confidence = "High".into();
    } else if !d.candidates.is_empty() || source || installer || appimage {
        d.confidence = "Medium".into();
    }
    d.recommended = app_run.or_else(|| {
        if d.candidates.len() == 1 {
            Some(d.candidates[0].path.clone())
        } else {
            let matches: Vec<_> = d
                .candidates
                .iter()
                .filter(|c| {
                    c.path
                        .file_name()
                        .is_some_and(|n| application_id(&n.to_string_lossy()) == d.suggested_id)
                })
                .collect();
            if matches.len() == 1 {
                Some(matches[0].path.clone())
            } else {
                None
            }
        }
    });
    if d.candidates.is_empty() {
        d.warnings.push(
            "No application entry point identified. You can still browse and extract this archive."
                .into(),
        );
    } else if d.recommended.is_none() {
        d.warnings
            .push("Multiple executables found. Choose the main application entry point.".into());
    }
    if matches!(
        d.kind,
        ApplicationKind::AppImageDistribution
            | ApplicationKind::SystemFilesystem
            | ApplicationKind::SourceCode
            | ApplicationKind::InstallerBased
    ) {
        d.recommended = None;
    }
    d
}

pub fn preview_text(archive: &Inspection, path: &Path) -> Option<String> {
    let entry = archive.entries.iter().find(|e| e.path == path)?;
    if entry.preview.contains(&0) {
        return None;
    }
    let text = std::str::from_utf8(&entry.preview).ok()?;
    Some(
        text.chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t' | '\r'))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inspection(names: &[(&str, bool, &[u8])]) -> Inspection {
        Inspection {
            source: "example.tar.gz".into(),
            sha256: "test".into(),
            compressed_size: 100,
            total_size: 10,
            format: "tar".into(),
            entries: names
                .iter()
                .map(|(path, executable, bytes)| crate::archive::Entry {
                    path: (*path).into(),
                    kind: EntryKind::File,
                    size: bytes.len() as u64,
                    executable: *executable,
                    link: None,
                    sha256: None,
                    preview: bytes.to_vec(),
                })
                .collect(),
        }
    }
    #[test]
    fn appdir_requires_structure_and_generic_exec_is_not_portable() {
        let archive = inspection(&[
            ("Example.AppDir/AppRun", true, b"#!/bin/sh"),
            (
                "Example.AppDir/usr/share/applications/example.desktop",
                false,
                b"[Desktop Entry]",
            ),
            ("Example.AppDir/README.md", false, b"Read me"),
        ]);
        let detected = analyze(&archive);
        assert_eq!(detected.kind, ApplicationKind::AppDir);
        assert_eq!(detected.readmes.len(), 1);
        assert!(detected.recommended.is_some());
        let detected = analyze(&inspection(&[("tool", true, b"#!/bin/sh")]));
        assert_eq!(detected.kind, ApplicationKind::PrecompiledBinaries);
        let detected = analyze(&inspection(&[
            ("src/Cargo.toml", false, b"[package]"),
            ("src/install.sh", true, b"#!/bin/sh"),
        ]));
        assert_eq!(detected.kind, ApplicationKind::SourceCode);
        assert!(detected.recommended.is_none());
    }
    #[test]
    fn ids_are_bounded() {
        assert_eq!(application_id("My App / Test"), "my-app-test");
        assert_eq!(application_id(".."), "application");
        assert!(application_id(&"x".repeat(100)).len() <= 80);
    }
}
