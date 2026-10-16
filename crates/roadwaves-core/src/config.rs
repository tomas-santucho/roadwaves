use crate::{autotune::TuneMode, selection::GeoPoint};
use serde::{Deserialize, Serialize};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FavoriteSnapshot {
    pub channel_id: String,
    pub name: String,
    #[serde(default)]
    pub country: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub mode: TuneMode,
    #[serde(default)]
    pub favorites: Vec<FavoriteSnapshot>,
    #[serde(default)]
    pub hidden_channels: Vec<String>,
    #[serde(default)]
    pub manual_location: Option<GeoPoint>,
    #[serde(default = "default_audio_port")]
    pub audio_port: u16,
    #[serde(default = "default_telemetry_port")]
    pub telemetry_port: u16,
    #[serde(default)]
    pub start_with_login: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            mode: TuneMode::AutoTune,
            favorites: Vec::new(),
            hidden_channels: Vec::new(),
            manual_location: None,
            audio_port: default_audio_port(),
            telemetry_port: default_telemetry_port(),
            start_with_login: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigLoad {
    pub config: AppConfig,
    pub warning: Option<String>,
}

fn default_schema_version() -> u32 {
    CONFIG_SCHEMA_VERSION
}
fn default_audio_port() -> u16 {
    47926
}
fn default_telemetry_port() -> u16 {
    47927
}

pub fn user_config_dir() -> PathBuf {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("roadwaves");
    }
    #[cfg(windows)]
    if let Some(path) = env::var_os("APPDATA") {
        return PathBuf::from(path).join("RoadWaves");
    }
    if let Some(path) = env::var_os("HOME") {
        return PathBuf::from(path).join(".config").join("roadwaves");
    }
    PathBuf::from(".roadwaves")
}

pub fn user_config_path() -> PathBuf {
    user_config_dir().join("config.json")
}

pub fn load(path: &Path) -> io::Result<ConfigLoad> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ConfigLoad {
                config: AppConfig::default(),
                warning: None,
            });
        }
        Err(error) => return Err(error),
    };
    let parsed = serde_json::from_slice::<AppConfig>(&bytes);
    match parsed {
        Ok(config) if config.schema_version == CONFIG_SCHEMA_VERSION => Ok(ConfigLoad {
            config,
            warning: None,
        }),
        Ok(config) => preserve_bad_config(
            path,
            format!("unsupported config schema {}", config.schema_version),
        ),
        Err(error) => preserve_bad_config(path, format!("invalid config JSON: {error}")),
    }
}

fn preserve_bad_config(path: &Path, reason: String) -> io::Result<ConfigLoad> {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let backup = path.with_file_name(format!(
        "{}.corrupt-{suffix}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config.json")
    ));
    let warning = match fs::rename(path, &backup) {
        Ok(()) => format!("{reason}; preserved the old file as {}", backup.display()),
        Err(error) => format!("{reason}; could not preserve the old file: {error}"),
    };
    Ok(ConfigLoad {
        config: AppConfig::default(),
        warning: Some(warning),
    })
}

pub fn store(path: &Path, config: &AppConfig) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(config).map_err(io::Error::other)?,
    )?;
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(path)?;
            fs::rename(temporary, path)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn config_round_trip_and_corrupt_recovery() {
        let directory = tempfile_dir();
        let path = directory.join("config.json");
        let config = AppConfig {
            mode: TuneMode::Explore,
            favorites: vec![FavoriteSnapshot {
                channel_id: "channel".into(),
                name: "Example".into(),
                country: Some("DE".into()),
            }],
            ..AppConfig::default()
        };
        store(&path, &config).unwrap();
        assert_eq!(load(&path).unwrap().config, config);
        fs::write(&path, b"not json").unwrap();
        let recovered = load(&path).unwrap();
        assert_eq!(recovered.config, AppConfig::default());
        assert!(recovered.warning.is_some());
        assert!(directory.read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("corrupt-")));
        let _ = fs::remove_dir_all(directory);
    }

    fn tempfile_dir() -> PathBuf {
        let path = env::temp_dir().join(format!(
            "roadwaves-config-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
