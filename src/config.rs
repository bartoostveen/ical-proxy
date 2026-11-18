use anyhow::{Context, Result};
use figment::Figment;
use figment::providers::{Format, Toml};
use serde::Deserialize;
use std::env;
use std::path::PathBuf;

pub const DEFAULT_CONFIG_PATH: &str = "config.toml";
pub const CONFIG_PATH_ENV: &str = "ICAL_PROXY_CONFIG";
const DEFAULT_ROOT_URL: &str = "https://omeduostuurcentenneef.nl";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub debug: bool,
    pub port: u16,
    pub host: Option<String>,
    pub root_url: String,
    #[serde(alias = "redis_endpoint")]
    pub redis_url: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            debug: false,
            port: 8080,
            host: None,
            root_url: DEFAULT_ROOT_URL.to_owned(),
            redis_url: None,
        }
    }
}

impl Config {
    /// Loads the TOML configuration file. A missing default file is fine;
    /// explicitly configured paths must exist.
    pub fn load() -> Result<(Self, Option<PathBuf>)> {
        let configured_path = env::var_os(CONFIG_PATH_ENV).map(PathBuf::from);
        let required = configured_path.is_some();
        let path = configured_path.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));

        let exists = match path.try_exists() {
            Ok(true) if path.is_file() => true,
            Ok(true) => anyhow::bail!("configuration path {} is not a file", path.display()),
            Ok(false) if required => {
                anyhow::bail!("configuration file {} does not exist", path.display())
            }
            Ok(false) => false,
            Err(error) => {
                return Err(error).with_context(|| format!("failed to inspect {}", path.display()));
            }
        };

        let figment = if exists {
            Figment::new().merge(Toml::file(&path))
        } else {
            Figment::new()
        };

        let config = figment
            .extract()
            .with_context(|| format!("failed to load configuration from {}", path.display()))?;

        Ok((config, exists.then_some(path)))
    }

    /// The address to bind to. When unset, debug mode listens on all
    /// interfaces and otherwise only on localhost.
    pub fn host(&self) -> String {
        self.host
            .as_deref()
            .map(str::trim)
            .filter(|host| !host.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                if self.debug {
                    "0.0.0.0".to_owned()
                } else {
                    "127.0.0.1".to_owned()
                }
            })
    }

    /// The optional Redis URL. Omitting it, or leaving it blank, disables
    /// caching.
    pub fn redis_url(&self) -> Option<&str> {
        self.redis_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Config {
        Figment::new().merge(Toml::string(input)).extract().unwrap()
    }

    #[test]
    fn defaults_do_not_require_a_config_file_or_redis() {
        let config: Config = Figment::new().extract().unwrap();

        assert!(!config.debug);
        assert_eq!(config.port, 8080);
        assert_eq!(config.host(), "127.0.0.1");
        assert_eq!(config.root_url, DEFAULT_ROOT_URL);
        assert_eq!(config.redis_url(), None);
    }

    #[test]
    fn reads_toml_settings() {
        let config = parse(
            r#"
            debug = true
            port = 9000
            root_url = "https://calendar.example"
            redis_url = "redis://localhost:6379"
            "#,
        );

        assert!(config.debug);
        assert_eq!(config.port, 9000);
        assert_eq!(config.host(), "0.0.0.0");
        assert_eq!(config.root_url, "https://calendar.example");
        assert_eq!(config.redis_url(), Some("redis://localhost:6379"));
    }

    #[test]
    fn explicit_host_wins_and_blank_redis_disables_cache() {
        let config = parse(
            r#"
            debug = true
            host = "localhost"
            redis_url = "   "
            "#,
        );

        assert_eq!(config.host(), "localhost");
        assert_eq!(config.redis_url(), None);
    }

    #[test]
    fn accepts_legacy_redis_endpoint_key() {
        let config = parse(r#"redis_endpoint = "127.0.0.1:6379""#);
        assert_eq!(config.redis_url(), Some("127.0.0.1:6379"));
    }
}
