use std::collections::{BTreeMap as Map, BTreeSet as Set};
use std::{fs, path::PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    buckal_warn,
    utils::{UnwrapOrExit, get_buck2_root},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(
        default = "default_buck2_binary",
        skip_serializing_if = "is_default_buck2_binary"
    )]
    pub buck2_binary: String,
    #[serde(default, skip_serializing_if = "RegistryDefault::if_skip")]
    pub registry: RegistryDefault,
    #[serde(default = "default_registries", skip_serializing_if = "Map::is_empty")]
    pub registries: Map<String, RegistryEntry>,
}

fn is_default_buck2_binary(value: &str) -> bool {
    value == "buck2"
}

fn default_buck2_binary() -> String {
    "buck2".to_string()
}

fn default_registries() -> Map<String, RegistryEntry> {
    let mut registries = Map::new();
    registries.insert(
        "buck2hub".to_string(),
        RegistryEntry {
            base: "https://hub.buck2hub.com".to_string(),
            api: "https://git.buck2hub.com".to_string(),
            token: None,
        },
    );
    registries
}

impl Default for Config {
    fn default() -> Self {
        Self {
            buck2_binary: default_buck2_binary(),
            registry: RegistryDefault::default(),
            registries: default_registries(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistryDefault {
    pub default: Option<String>,
}

impl RegistryDefault {
    pub fn if_skip(&self) -> bool {
        self.default.is_none()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub base: String,
    pub api: String,
    pub token: Option<String>,
}

impl Config {
    /// Load configuration from ~/.config/buckal/config.toml
    pub fn load() -> Self {
        let config_path = Self::config_path();

        if !config_path.exists() {
            return Self::default();
        }

        match fs::read_to_string(&config_path) {
            Ok(content) => match toml::from_str::<Config>(&content) {
                Ok(config) => config,
                Err(_) => {
                    eprintln!(
                        "Warning: Failed to parse config file at {}, using defaults",
                        config_path.display()
                    );
                    Self::default()
                }
            },
            Err(_) => {
                eprintln!(
                    "Warning: Failed to read config file at {}, using defaults",
                    config_path.display()
                );
                Self::default()
            }
        }
    }

    /// Save configuration to ~/.config/buckal/config.toml
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path();
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self).unwrap();
        fs::write(config_path, content)?;
        Ok(())
    }

    /// Get the configuration file path
    pub fn config_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".config")
            .join("buckal")
            .join("config.toml")
    }

    /// Get the default registry name, or "buck2hub" if not set
    pub fn default_registry(&self) -> &str {
        self.registry.default.as_deref().unwrap_or("buck2hub")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoConfig {
    pub inherit_workspace_deps: bool,
    pub align_cells: bool,
    pub ignore_tests: bool,
    pub patch_fields: Set<String>,
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            inherit_workspace_deps: false,
            align_cells: false,
            ignore_tests: true,
            patch_fields: Set::new(),
        }
    }
}

impl RepoConfig {
    pub fn load() -> Self {
        let repo_config_path = Self::repo_config_path();

        if !repo_config_path.exists() {
            return Self::default();
        }

        match fs::read_to_string(&repo_config_path) {
            Ok(content) => match toml::from_str::<RepoConfig>(&content) {
                Ok(config) => config,
                Err(_) => {
                    buckal_warn!(
                        "Failed to parse repo config file at {}, using defaults",
                        repo_config_path.display()
                    );
                    Self::default()
                }
            },
            Err(_) => {
                buckal_warn!(
                    "Failed to read repo config file at {}, using defaults",
                    repo_config_path.display()
                );
                Self::default()
            }
        }
    }

    pub fn repo_config_path() -> PathBuf {
        let buck2_root = get_buck2_root().unwrap_or_exit();
        buck2_root.join("buckal.toml").into()
    }
}
