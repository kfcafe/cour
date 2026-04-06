use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use clap::Args;

use serde::Deserialize;

use crate::error::{AppError, AppResult};

const APP_NAME: &str = "cour";
const CONFIG_FILE_NAME: &str = "config.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPaths {
    pub config_file: PathBuf,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl ProjectPaths {
    pub fn detect() -> AppResult<Self> {
        fn get_var(key: &str) -> Option<OsString> {
            env::var_os(key)
        }

        Self::from_env(get_var)
    }

    fn from_env<F>(mut get_var: F) -> AppResult<Self>
    where
        F: FnMut(&str) -> Option<OsString>,
    {
        let home = get_var("HOME").map(PathBuf::from);

        let config_root = Self::resolve_base_dir(
            "XDG_CONFIG_HOME",
            home.as_deref(),
            Path::new(".config"),
            &mut get_var,
        )?;
        let state_root = Self::resolve_base_dir(
            "XDG_STATE_HOME",
            home.as_deref(),
            Path::new(".local/state"),
            &mut get_var,
        )?;
        let cache_root = Self::resolve_base_dir(
            "XDG_CACHE_HOME",
            home.as_deref(),
            Path::new(".cache"),
            &mut get_var,
        )?;

        Ok(Self {
            config_file: config_root.join(APP_NAME).join(CONFIG_FILE_NAME),
            state_dir: state_root.join(APP_NAME),
            cache_dir: cache_root.join(APP_NAME),
        })
    }

    fn resolve_base_dir<F>(
        xdg_key: &str,
        home: Option<&Path>,
        default_relative_to_home: &Path,
        get_var: &mut F,
    ) -> AppResult<PathBuf>
    where
        F: FnMut(&str) -> Option<OsString>,
    {
        if let Some(value) = get_var(xdg_key) {
            let path = PathBuf::from(value);
            if !path.as_os_str().is_empty() {
                return Ok(path);
            }
        }

        let home = home.ok_or_else(|| {
            AppError::Config(format!(
                "{xdg_key} is not set and HOME is unavailable for default path resolution"
            ))
        })?;

        Ok(home.join(default_relative_to_home))
    }
}

#[derive(Debug, Clone, Args, Default, PartialEq, Eq)]
pub struct ConfigArgs {
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub accounts: Vec<AccountConfig>,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub smtp: Vec<SmtpIdentityConfig>,
}

impl AppConfig {
    pub fn load(path: Option<&Path>) -> AppResult<Self> {
        let resolved_path = match path {
            Some(path) => path.to_path_buf(),
            None => ProjectPaths::detect()?.config_file,
        };

        let raw = fs::read_to_string(&resolved_path).map_err(|err| {
            AppError::Config(format!(
                "failed to read config from {}: {err}",
                resolved_path.display()
            ))
        })?;

        let config = toml::from_str(&raw).map_err(|err| {
            AppError::Config(format!(
                "failed to parse config from {}: {err}",
                resolved_path.display()
            ))
        })?;

        Ok(config)
    }

    pub fn default_account(&self) -> AppResult<&AccountConfig> {
        match self.accounts.as_slice() {
            [] => Err(AppError::Config(
                "config must define at least one account".to_string(),
            )),
            [only] => Ok(only),
            many => many
                .iter()
                .find(|account| account.default.unwrap_or(false))
                .ok_or_else(|| {
                    AppError::Config(
                        "multiple accounts configured but none marked default = true".to_string(),
                    )
                }),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AccountConfig {
    pub name: String,
    pub email_address: String,
    pub maildir_root: PathBuf,
    pub sync_command: Option<String>,
    pub default: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct AiConfig {
    pub embedding: Option<ProviderConfig>,
    pub extraction: Option<ProviderConfig>,
    pub drafting: Option<ProviderConfig>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub provider: String,
    pub model: String,
    pub api_url: Option<String>,
    pub api_key_env: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SmtpIdentityConfig {
    pub name: String,
    pub email_address: String,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password_env: Option<String>,
    pub tls_mode: Option<String>,
    pub default: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, ProjectPaths};
    use crate::error::AppError;
    use crate::test_support::TestEnvGuard;
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn detect_from_pairs(pairs: &[(&str, &str)]) -> Result<ProjectPaths, AppError> {
        let env_map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(key, value)| (String::from(*key), OsString::from(*value)))
            .collect();

        ProjectPaths::from_env(|key| env_map.get(key).cloned())
    }

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("cour-{label}-{unique}"));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    fn write_config(path: &Path) {
        let raw = r#"
accounts = [
  { name = "personal", email_address = "ash@example.com", maildir_root = "/tmp/mail/personal", sync_command = "mbsync personal", default = true }
]

[ai.embedding]
provider = "ollama"
model = "nomic-embed-text"
api_url = "http://localhost:11434"
enabled = true

[ai.extraction]
provider = "openai-compatible"
model = "gpt-4o-mini"
api_url = "https://example.invalid/v1"
api_key_env = "MAILFOR_API_KEY"
enabled = true

[[smtp]]
name = "default"
email_address = "ash@example.com"
host = "smtp.example.com"
port = 465
username = "ash@example.com"
password_env = "MAILFOR_SMTP_PASSWORD"
tls_mode = "tls"
default = true
"#;
        fs::write(path, raw).expect("write config file");
    }

    #[test]
    fn project_paths_use_home_defaults() {
        let paths = detect_from_pairs(&[("HOME", "/home/alice")]).expect("paths should resolve");

        assert_eq!(
            paths.config_file,
            PathBuf::from("/home/alice/.config/cour/config.toml")
        );
        assert_eq!(
            paths.state_dir,
            PathBuf::from("/home/alice/.local/state/cour")
        );
        assert_eq!(paths.cache_dir, PathBuf::from("/home/alice/.cache/cour"));
    }

    #[test]
    fn project_paths_use_xdg_overrides() {
        let paths = detect_from_pairs(&[
            ("HOME", "/home/alice"),
            ("XDG_CONFIG_HOME", "/tmp/xdg-config"),
            ("XDG_STATE_HOME", "/tmp/xdg-state"),
            ("XDG_CACHE_HOME", "/tmp/xdg-cache"),
        ])
        .expect("paths should resolve");

        assert_eq!(
            paths.config_file,
            PathBuf::from("/tmp/xdg-config/cour/config.toml")
        );
        assert_eq!(paths.state_dir, PathBuf::from("/tmp/xdg-state/cour"));
        assert_eq!(paths.cache_dir, PathBuf::from("/tmp/xdg-cache/cour"));
    }

    #[test]
    fn loads_config_from_explicit_path() {
        let dir = temp_dir("explicit-config");
        let path = dir.join("config.toml");
        write_config(&path);

        let config = AppConfig::load(Some(&path)).expect("load config from explicit path");

        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.accounts[0].name, "personal");
        assert_eq!(
            config.accounts[0].maildir_root,
            PathBuf::from("/tmp/mail/personal")
        );
        assert_eq!(config.smtp.len(), 1);
        assert_eq!(config.smtp[0].host, "smtp.example.com");
        assert_eq!(
            config.ai.embedding.as_ref().unwrap().model,
            "nomic-embed-text"
        );
    }

    #[test]
    fn loads_config_from_default_path() {
        let dir = temp_dir("default-config");
        let config_home = dir.join("xdg-config");
        let app_dir = config_home.join("cour");
        fs::create_dir_all(&app_dir).expect("create app config dir");
        let path = app_dir.join("config.toml");
        write_config(&path);

        let mut env = TestEnvGuard::acquire();
        env.set_var("HOME", &dir);
        env.set_var("XDG_CONFIG_HOME", &config_home);
        env.remove_var("XDG_STATE_HOME");
        env.remove_var("XDG_CACHE_HOME");

        let config = AppConfig::load(None).expect("load config from default path");
        assert_eq!(config.accounts[0].email_address, "ash@example.com");
    }
}
