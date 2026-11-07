use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// AppImage runtime libraries belong to Unpack, not applications it launches.
/// Return the original environment so the GUI can apply it to Command or GIO.
pub fn child_environment() -> Vec<(&'static str, Option<std::ffi::OsString>)> {
    restored_environment(|key| std::env::var_os(key))
}

fn restored_environment(
    get: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Vec<(&'static str, Option<std::ffi::OsString>)> {
    if get("UNPACK_APPIMAGE").is_none() {
        return vec![];
    }
    [
        "LD_LIBRARY_PATH",
        "XDG_DATA_DIRS",
        "GSETTINGS_SCHEMA_DIR",
        "GIO_MODULE_DIR",
        "GSK_RENDERER",
    ]
    .into_iter()
    .map(|key| {
        let value = get(&format!("UNPACK_ORIGINAL_{key}")).filter(|v| !v.is_empty());
        (key, value)
    })
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopEntry {
    pub name: String,
    pub generic_name: String,
    pub comment: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub icon: String,
    pub terminal: bool,
    pub categories: Vec<String>,
    pub startup_notify: bool,
    pub startup_wm_class: Option<String>,
}
fn value(s: &str) -> Result<String> {
    if s.chars().any(|c| c.is_control()) {
        return Err(Error::Invalid(
            "Desktop fields cannot contain control characters".into(),
        ));
    }
    Ok(s.replace('\\', "\\\\"))
}

/// Two layers: desktop string-value escaping, then Exec argument quoting.
/// Literal '%' is deliberately rejected: field codes within quoted arguments
/// have undefined behavior in the specification.
pub fn exec_argument(argument: &str) -> Result<String> {
    if argument.contains('%') || argument.chars().any(|c| c.is_control()) {
        return Err(Error::Invalid(
            "Launcher paths and arguments cannot contain '%' or control characters".into(),
        ));
    }
    let mut quoted = String::from("\"");
    for c in argument.chars() {
        if matches!(c, '\\' | '"' | '`' | '$') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    Ok(quoted.replace('\\', "\\\\"))
}
impl DesktopEntry {
    pub fn render(&self) -> Result<String> {
        if self.name.trim().is_empty()
            || !Path::new(&self.executable).is_absolute()
            || self.executable.contains('=')
        {
            return Err(Error::Invalid(
                "A launcher needs a name and an absolute executable path without '='".into(),
            ));
        }
        if self
            .categories
            .iter()
            .any(|s| s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        {
            return Err(Error::Invalid("Invalid application category".into()));
        }
        let mut command = vec![exec_argument(&self.executable)?];
        for arg in &self.arguments {
            command.push(exec_argument(arg)?);
        }
        let mut result = format!("[Desktop Entry]\nType=Application\nVersion=1.0\nName={}\nGenericName={}\nComment={}\nExec={}\nIcon={}\nTerminal={}\nCategories={};\nStartupNotify={}\nX-Unpack-Managed=true\n", value(&self.name)?, value(&self.generic_name)?, value(&self.comment)?, command.join(" "), value(&self.icon)?, self.terminal, self.categories.join(";"), self.startup_notify);
        if let Some(class) = &self.startup_wm_class {
            result.push_str(&format!("StartupWMClass={}\n", value(class)?));
        }
        Ok(result)
    }
}

/// Import only descriptive values. Exec, TryExec, Path, actions, environment,
/// D-Bus activation and launch arguments from the archive are never imported.
pub fn descriptive_fields(text: &str) -> Vec<(String, String)> {
    let mut in_entry = false;
    text.lines()
        .filter_map(|line| {
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
                return None;
            }
            if !in_entry {
                return None;
            }
            let (key, val) = line.split_once('=')?;
            if matches!(key, "Name" | "GenericName" | "Comment" | "StartupWMClass")
                && !val.chars().any(|c| c.is_control())
            {
                Some((key.into(), val.into()))
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn appimage_children_restore_host_libraries() {
        assert!(restored_environment(|_| None).is_empty());
        let adjustments = restored_environment(|key| match key {
            "UNPACK_APPIMAGE" => Some("1".into()),
            "UNPACK_ORIGINAL_XDG_DATA_DIRS" => Some("/usr/share".into()),
            _ => None,
        });
        assert_eq!(adjustments[0], ("LD_LIBRARY_PATH", None));
        assert_eq!(adjustments[1], ("XDG_DATA_DIRS", Some("/usr/share".into())));
    }
    #[test]
    fn quoting_and_injection() {
        assert_eq!(
            exec_argument("/home/user/My App").unwrap(),
            "\"/home/user/My App\""
        );
        assert!(exec_argument("/tmp/%f").is_err());
        assert!(exec_argument("bad\nExec=sh").is_err());
        assert_eq!(exec_argument("$x").unwrap(), "\"\\\\$x\"");
        let fields = descriptive_fields(
            "[Desktop Entry]\nName=Fine\nExec=sh -c evil\n[Desktop Action evil]\nName=Bad",
        );
        assert_eq!(fields, vec![("Name".into(), "Fine".into())]);
    }
}
