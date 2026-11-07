//! Conservative ELF inspection. Never runs ldd, interpreters, or downloaded code.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElfInfo {
    pub architecture: String,
    pub bits: u8,
    pub kind: String,
    pub interpreter: Option<String>,
    pub needed: Vec<String>,
    pub glibc_versions: Vec<String>,
    pub warnings: Vec<String>,
}

fn number(bytes: &[u8], offset: usize, width: usize, little: bool) -> Option<u64> {
    let value = bytes.get(offset..offset.checked_add(width)?)?;
    let mut out = 0u64;
    if little {
        for (i, b) in value.iter().enumerate() {
            out |= (*b as u64) << (i * 8);
        }
    } else {
        for b in value {
            out = (out << 8) | *b as u64;
        }
    }
    Some(out)
}
fn string(bytes: &[u8], offset: usize) -> Option<String> {
    let bytes = bytes.get(offset..)?;
    let end = bytes.iter().position(|b| *b == 0)?;
    std::str::from_utf8(&bytes[..end]).ok().map(String::from)
}

pub fn inspect(bytes: &[u8]) -> Option<ElfInfo> {
    if !bytes.starts_with(b"\x7fELF") {
        return None;
    }
    let bits = match bytes.get(4)? {
        1 => 32,
        2 => 64,
        _ => return None,
    };
    let little = match bytes.get(5)? {
        1 => true,
        2 => false,
        _ => return None,
    };
    let n = |o, w| number(bytes, o, w, little);
    let machine = n(18, 2)?;
    let architecture = match machine {
        62 => "x86_64",
        183 => "aarch64",
        3 => "x86",
        40 => "arm",
        243 => "riscv64",
        _ => "unknown",
    }
    .to_string();
    let kind = match n(16, 2)? {
        2 => "Executable",
        3 => "Shared object / position-independent executable",
        _ => "Other ELF object",
    }
    .to_string();
    let mut info = ElfInfo {
        architecture,
        bits,
        kind,
        interpreter: None,
        needed: vec![],
        glibc_versions: vec![],
        warnings: vec![],
    };
    let (phoff, entsize, count) = if bits == 64 {
        (n(32, 8)?, n(54, 2)?, n(56, 2)?)
    } else {
        (n(28, 4)?, n(42, 2)?, n(44, 2)?)
    };
    let mut segments = Vec::new();
    let mut dynamic = None;
    for i in 0..count.min(4096) {
        let off = usize::try_from(phoff.checked_add(i.checked_mul(entsize)?)?).ok()?;
        let Some(kind) = n(off, 4) else {
            info.warnings
                .push("ELF metadata exceeds the bounded inspection preview".into());
            break;
        };
        let fields = if bits == 64 {
            (n(off + 8, 8), n(off + 16, 8), n(off + 32, 8))
        } else {
            (n(off + 4, 4), n(off + 8, 4), n(off + 16, 4))
        };
        let (Some(fileoff), Some(address), Some(size)) = fields else {
            break;
        };
        if kind == 1 {
            segments.push((fileoff, address, size));
        }
        if kind == 2 {
            dynamic = Some((fileoff, size));
        }
        if kind == 3 {
            info.interpreter = usize::try_from(fileoff).ok().and_then(|o| string(bytes, o));
        }
    }
    if let Some((offset, size)) = dynamic {
        let width = if bits == 64 { 8 } else { 4 };
        let mut needed = vec![];
        let mut strtab = None;
        for i in 0..(size / (width * 2)).min(8192) {
            let Some(off) = offset
                .checked_add(i * width * 2)
                .and_then(|v| usize::try_from(v).ok())
            else {
                break;
            };
            let (Some(tag), Some(value)) = (
                n(off, width as usize),
                n(off + width as usize, width as usize),
            ) else {
                break;
            };
            if tag == 0 {
                break;
            }
            if tag == 1 {
                needed.push(value);
            }
            if tag == 5 {
                strtab = Some(value);
            }
        }
        if let Some(address) = strtab {
            let offset = segments.iter().find_map(|(offset, start, size)| {
                if address >= *start && address - start < *size {
                    offset.checked_add(address - start)
                } else {
                    None
                }
            });
            if let Some(offset) = offset {
                for index in needed {
                    if let Some(name) = offset
                        .checked_add(index)
                        .and_then(|o| usize::try_from(o).ok())
                        .and_then(|o| string(bytes, o))
                    {
                        info.needed.push(name);
                    }
                }
            }
        }
    }
    for part in bytes.split(|b| !b.is_ascii_alphanumeric() && *b != b'_' && *b != b'.') {
        if part.starts_with(b"GLIBC_") {
            if let Ok(s) = std::str::from_utf8(part) {
                info.glibc_versions.push(s.into());
            }
        }
    }
    info.glibc_versions.sort();
    info.glibc_versions.dedup();
    if info.architecture != std::env::consts::ARCH && info.architecture != "unknown" {
        info.warnings.push(format!(
            "Architecture mismatch: application is {}, this machine is {}",
            info.architecture,
            std::env::consts::ARCH
        ));
    }
    if let Some(interpreter) = &info.interpreter {
        if !std::path::Path::new(interpreter).exists() {
            info.warnings
                .push(format!("Dynamic linker not found: {interpreter}"));
        }
    }
    Some(info)
}

pub fn dependency_report(info: &ElfInfo, bundled: &[String]) -> Vec<String> {
    let arch = std::env::consts::ARCH;
    let directories = [
        "/lib".to_string(),
        "/lib64".into(),
        "/usr/lib".into(),
        "/usr/lib64".into(),
        format!("/usr/lib/{arch}-linux-gnu"),
        format!("/lib/{arch}-linux-gnu"),
    ];
    info.needed.iter().map(|library| {
        if bundled.contains(library) { format!("{library}: bundled (runtime resolution not verified)") }
        else if !library.contains('/') && directories.iter().any(|d| std::path::Path::new(d).join(library).is_file()) { format!("{library}: found in a standard library directory") }
        else { format!("{library}: not found in checked directories; loader cache, RPATH and other search paths may differ") }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspects_host_elf_without_execution() {
        let bytes = std::fs::read("/usr/bin/true").unwrap();
        let info = inspect(&bytes).unwrap();
        assert!(!info.architecture.is_empty());
        assert!(!info.needed.is_empty());
    }
    #[test]
    fn malformed_elf_is_bounded() {
        assert!(inspect(b"\x7fELF").is_none());
        assert!(inspect(b"hello").is_none());
    }
}
