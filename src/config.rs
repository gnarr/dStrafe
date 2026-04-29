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
    pub movement: MovementKeys,
}

impl AppConfig {
    pub fn load_from_working_dir() -> Self {
        Self::load_from_path(Path::new("dstrafe.toml"))
    }

    pub fn load_from_path(path: &Path) -> Self {
        let Ok(contents) = fs::read_to_string(path) else {
            return Self {
                movement: MovementKeys::default(),
            };
        };

        match parse_config(&contents) {
            Ok(config) => config,
            Err(error) => {
                log::warn!("Ignoring {path:?}: {error}");
                Self {
                    movement: MovementKeys::default(),
                }
            }
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
    let movement = MovementKeys::from_raw(raw.movement)?;

    Ok(AppConfig { movement })
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
    }
}
