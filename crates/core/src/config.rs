use std::fs;
use std::io;

use serde::{Deserialize, Serialize};

use crate::util::get_running_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogRotation {
    Daily,
    Hourly,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Config {
    pub addr: String,
    pub port: u16,
    pub https_port: u16,
    pub log_path: Option<String>,
    pub log_error_path: Option<String>,
    pub log_level: String,
    pub log_rotation: LogRotation,
    pub title: Option<String>,
    pub db_path: String,
}

impl Default for Config {
    fn default() -> Self {
        let path = get_running_path();
        let log_path = path.join("logs");
        let log_error_path = path.join("logs/error");

        Self {
            addr: "0.0.0.0".to_string(),
            port: 8080,
            https_port: 8443,
            log_level: "info".to_string(),
            log_rotation: LogRotation::Daily,
            title: Some("Ferri".to_string()),
            db_path: path.join("ferri.db").to_string_lossy().to_string(),
            log_path: Some(log_path.to_string_lossy().to_string()),
            log_error_path: Some(log_error_path.to_string_lossy().to_string()),
        }
    }
}

impl Config {
    /// Create a config with defaults and ensure required directories exist.
    pub fn with_dirs() -> io::Result<Self> {
        let cfg = Self::default();
        cfg.ensure_dirs()?;
        Ok(cfg)
    }

    /// Ensure log and DB parent directories exist.
    pub fn ensure_dirs(&self) -> io::Result<()> {
        if let Some(ref p) = self.log_path {
            fs::create_dir_all(p)?;
        }
        if let Some(ref p) = self.log_error_path {
            fs::create_dir_all(p)?;
        }
        // Ensure DB parent directory exists (if any)
        if let Some(parent) = std::path::Path::new(&self.db_path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        Ok(())
    }

    /// Load config from a TOML file.
    pub fn load_from_file<P: AsRef<std::path::Path>>(path: P) -> io::Result<Self> {
        let content = fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&content).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("TOML parse error: {}", e),
            )
        })?;
        Ok(cfg)
    }

    /// Save config to a TOML file.
    pub fn save_to_file<P: AsRef<std::path::Path>>(&self, path: P) -> io::Result<()> {
        let content = toml::to_string_pretty(self).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("TOML serialize error: {}", e),
            )
        })?;
        fs::write(path, content)?;
        Ok(())
    }
}

pub fn load_config() -> io::Result<Config> {
    let path = get_running_path().join("config.toml");
    if path.exists() {
        Config::load_from_file(path)
    } else {
        let cfg = Config::with_dirs()?;
        cfg.save_to_file(get_running_path().join("config.toml"))?;
        Ok(cfg)
    }
}
