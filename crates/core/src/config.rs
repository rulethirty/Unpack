use crate::{
    security::{atomic_json, Limits},
    Error, Result,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SCHEMA: u32 = 1;

/// Only the actual installed Flatpak is restricted; the GNOME development SDK
/// also has /.flatpak-info and must not be mistaken for the packaged application.
pub fn sandboxed_package() -> bool {
    std::env::var("FLATPAK_ID").is_ok_and(|id| id == "io.github.unpack.Unpack")
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub home: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
    pub applications: PathBuf,
    pub icons: PathBuf,
    pub default_install: PathBuf,
}
impl Paths {
    pub fn discover() -> Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or_else(|| Error::Invalid("HOME must be an absolute path".into()))?;
        let xdg = |name: &str, fallback: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(fallback))
        };
        let data = xdg("XDG_DATA_HOME", ".local/share");
        Ok(Self {
            config: xdg("XDG_CONFIG_HOME", ".config").join("unpack"),
            state: xdg("XDG_STATE_HOME", ".local/state").join("unpack"),
            applications: data.join("applications"),
            icons: data.join("icons/hicolor"),
            data: data.join("unpack"),
            default_install: home.join(".local/opt"),
            home,
        })
    }
    pub fn isolated(root: &Path) -> Self {
        Self {
            home: root.into(),
            config: root.join("config"),
            data: root.join("data"),
            state: root.join("state"),
            applications: root.join("applications"),
            icons: root.join("icons/hicolor"),
            default_install: root.join("opt"),
        }
    }
    pub fn initialize(&self) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        for p in [
            &self.config,
            &self.data,
            &self.state,
            &self.data.join("manifests"),
            &self.state.join("transactions"),
        ] {
            std::fs::create_dir_all(p)?;
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpenBehavior {
    #[default]
    Ask,
    Browser,
    Installer,
    External,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub open_behavior: OpenBehavior,
    pub theme: Theme,
    pub show_hidden: bool,
    pub create_launcher: bool,
    pub install_icons: bool,
    pub restore_window: bool,
    pub window_width: i32,
    pub window_height: i32,
    pub maximized: bool,
    pub onboarding_complete: bool,
    pub last_directory: Option<PathBuf>,
    pub installation_directory: Option<PathBuf>,
    pub limits: Limits,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA,
            open_behavior: OpenBehavior::Ask,
            theme: Theme::System,
            show_hidden: false,
            create_launcher: true,
            install_icons: true,
            restore_window: true,
            window_width: 1100,
            window_height: 760,
            maximized: false,
            onboarding_complete: false,
            last_directory: None,
            installation_directory: None,
            limits: Limits::default(),
        }
    }
}
impl Settings {
    pub fn load(paths: &Paths) -> Result<Self> {
        let path = paths.config.join("settings.json");
        if !path.try_exists()? {
            return Ok(Self::default());
        }
        let value: serde_json::Value = serde_json::from_reader(std::fs::File::open(path)?)?;
        let version = value
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if version > SCHEMA as u64 {
            return Err(Error::Unsupported(format!(
                "Settings schema {version} is newer than this application"
            )));
        }
        // Version zero had the same fields without an explicit schema marker.
        let mut settings: Self = serde_json::from_value(value)?;
        settings.schema_version = SCHEMA;
        settings.limits.validate()?;
        settings.window_width = settings.window_width.clamp(600, 4096);
        settings.window_height = settings.window_height.clamp(420, 4096);
        Ok(settings)
    }
    pub fn save(&self, paths: &Paths) -> Result<()> {
        self.limits.validate()?;
        atomic_json(&paths.config.join("settings.json"), self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_and_future_version() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::isolated(dir.path());
        paths.initialize().unwrap();
        std::fs::write(paths.config.join("settings.json"), r#"{"theme":"dark"}"#).unwrap();
        let settings = Settings::load(&paths).unwrap();
        assert_eq!(settings.schema_version, 1);
        assert_eq!(settings.theme, Theme::Dark);
        std::fs::write(
            paths.config.join("settings.json"),
            r#"{"schema_version":999}"#,
        )
        .unwrap();
        assert!(Settings::load(&paths).is_err());
    }
}
