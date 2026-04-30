use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MovementKeys {
    pub forward: char,
    pub backward: char,
    pub left: char,
    pub right: char,
}

impl Default for MovementKeys {
    fn default() -> Self {
        Self {
            forward: 'W',
            backward: 'S',
            left: 'A',
            right: 'D',
        }
    }
}

impl MovementKeys {
    pub fn vertical_pair(self) -> [char; 2] {
        [self.forward, self.backward]
    }

    pub fn horizontal_pair(self) -> [char; 2] {
        [self.left, self.right]
    }

    pub fn contains(self, key: char) -> bool {
        let key = key.to_ascii_uppercase();
        [self.forward, self.backward, self.left, self.right].contains(&key)
    }

    fn from_raw(raw: Option<RawMovementConfig>) -> Result<Self, ConfigKeyError> {
        let defaults = Self::default();
        let Some(raw) = raw else {
            return Ok(defaults);
        };

        let keys = Self {
            forward: parse_key(raw.forward.as_deref(), defaults.forward)?,
            backward: parse_key(raw.backward.as_deref(), defaults.backward)?,
            left: parse_key(raw.left.as_deref(), defaults.left)?,
            right: parse_key(raw.right.as_deref(), defaults.right)?,
        };

        if unique_key_count(keys) != 4 {
            return Err(ConfigKeyError);
        }

        Ok(keys)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppConfig {
    pub debug: bool,
    pub movement: MovementKeys,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedAppConfig {
    pub config: AppConfig,
    pub warning: Option<String>,
}

impl AppConfig {
    pub fn load_from_working_dir_with_diagnostics() -> LoadedAppConfig {
        Self::load_from_path_with_diagnostics(Path::new("dstrafe.toml"))
    }

    #[cfg(test)]
    pub fn load_from_path(path: &Path) -> Self {
        let loaded = Self::load_from_path_with_diagnostics(path);

        if let Some(warning) = loaded.warning.as_ref() {
            log::warn!("{warning}");
        }

        loaded.config
    }

    pub fn load_from_path_with_diagnostics(path: &Path) -> LoadedAppConfig {
        let Ok(contents) = fs::read_to_string(path) else {
            return LoadedAppConfig {
                config: Self::default(),
                warning: None,
            };
        };

        match parse_config(&contents) {
            Ok(config) => LoadedAppConfig {
                config,
                warning: None,
            },
            Err(error) => LoadedAppConfig {
                config: Self::default(),
                warning: Some(format!("Ignoring {path:?}: {error}")),
            },
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            debug: false,
            movement: MovementKeys::default(),
        }
    }
}

#[derive(Debug)]
struct ConfigKeyError;

impl std::fmt::Display for ConfigKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("movement keys must be unique single ASCII alphanumeric characters")
    }
}

impl std::error::Error for ConfigKeyError {}

#[derive(Debug, Deserialize)]
struct RawConfig {
    debug: Option<bool>,
    movement: Option<RawMovementConfig>,
}

#[derive(Debug, Deserialize)]
struct RawMovementConfig {
    forward: Option<String>,
    backward: Option<String>,
    left: Option<String>,
    right: Option<String>,
}

fn parse_config(contents: &str) -> Result<AppConfig, Box<dyn std::error::Error>> {
    let raw = toml::from_str::<RawConfig>(contents)?;
    let debug = raw.debug.unwrap_or(false);
    let movement = MovementKeys::from_raw(raw.movement)?;

    Ok(AppConfig { debug, movement })
}

fn parse_key(value: Option<&str>, default: char) -> Result<char, ConfigKeyError> {
    let Some(value) = value else {
        return Ok(default);
    };

    let trimmed = value.trim();
    let mut chars = trimmed.chars();
    let Some(key) = chars.next() else {
        return Err(ConfigKeyError);
    };

    if chars.next().is_some() || !key.is_ascii_alphanumeric() {
        return Err(ConfigKeyError);
    }

    Ok(key.to_ascii_uppercase())
}

fn unique_key_count(keys: MovementKeys) -> usize {
    [keys.forward, keys.backward, keys.left, keys.right]
        .into_iter()
        .collect::<HashSet<_>>()
        .len()
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, MovementKeys, parse_config};

    #[test]
    fn missing_debug_defaults_to_false() {
        let config = parse_config("").expect("empty config");

        assert!(!config.debug);
    }

    #[test]
    fn parses_debug_true() {
        let config = parse_config("debug = true").expect("valid config");

        assert!(config.debug);
    }

    #[test]
    fn parses_valid_movement_keys() {
        let config = parse_config(
            r#"
            [movement]
            forward = "E"
            backward = "D"
            left = "S"
            right = "F"
            "#,
        )
        .expect("valid config");

        assert_eq!(
            config.movement,
            MovementKeys {
                forward: 'E',
                backward: 'D',
                left: 'S',
                right: 'F',
            }
        );
    }

    #[test]
    fn duplicate_configured_keys_are_rejected() {
        let error = parse_config(
            r#"
            [movement]
            forward = "W"
            backward = "W"
            left = "A"
            right = "D"
            "#,
        )
        .expect_err("duplicate keys should fail");

        assert!(error.to_string().contains("movement keys"));
    }

    #[test]
    fn unsupported_configured_keys_are_rejected() {
        let error = parse_config(
            r#"
            [movement]
            forward = "Up"
            backward = "S"
            left = "A"
            right = "D"
            "#,
        )
        .expect_err("special keys should fail");

        assert!(error.to_string().contains("movement keys"));
    }

    #[test]
    fn load_falls_back_to_defaults_when_file_is_missing() {
        let config = AppConfig::load_from_path(std::path::Path::new(
            "this-file-should-not-exist-dstrafe.toml",
        ));

        assert_eq!(config.movement, MovementKeys::default());
        assert!(!config.debug);
    }

    #[test]
    fn load_with_diagnostics_warns_and_falls_back_to_defaults_for_invalid_toml() {
        let path = std::env::temp_dir().join(format!(
            "dstrafe-invalid-config-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after Unix epoch")
                .as_nanos()
        ));
        std::fs::write(&path, "debug = ").expect("write invalid config");

        let loaded = AppConfig::load_from_path_with_diagnostics(&path);

        std::fs::remove_file(&path).expect("remove invalid config");
        assert_eq!(loaded.config, AppConfig::default());
        let warning = loaded
            .warning
            .expect("invalid TOML should return a warning");
        assert!(warning.starts_with("Ignoring "));
    }
}
