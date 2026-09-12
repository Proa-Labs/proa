// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use once_cell::sync::Lazy;
use serde::Deserialize;

use super::validators::{validator_by_name, ValidatorFn};

#[derive(Clone, Debug)]
pub struct Config {
    pub cache_size: usize,
    pub prefix: Option<String>,
    pub order_sensitive_modifiers: Vec<String>,
    pub theme: HashMap<String, Vec<ClassDefinition>>,
    pub class_groups: HashMap<String, Vec<ClassDefinition>>,
    pub conflicting_class_groups: HashMap<String, Vec<String>>,
    pub conflicting_class_group_modifiers: HashMap<String, Vec<String>>,
}

#[derive(Clone, Debug)]
pub enum ClassDefinition {
    Literal(String),
    Validator(ValidatorFn),
    Theme(String),
    Object(Vec<ClassDefinitionEntry>),
}

#[derive(Clone, Debug)]
pub struct ClassDefinitionEntry {
    pub key: String,
    pub definitions: Vec<ClassDefinition>,
}

#[derive(Debug)]
pub enum ConfigError {
    Json(serde_json::Error),
    UnknownValidator(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Json(err) => write!(f, "failed to parse config json: {err}"),
            ConfigError::UnknownValidator(name) => {
                write!(f, "unknown validator referenced in config: {name}")
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Json(err) => Some(err),
            ConfigError::UnknownValidator(_) => None,
        }
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(value: serde_json::Error) -> Self {
        ConfigError::Json(value)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConfig {
    cache_size: usize,
    prefix: Option<String>,
    order_sensitive_modifiers: Vec<String>,
    theme: HashMap<String, Vec<RawDefinition>>,
    class_groups: HashMap<String, Vec<RawDefinition>>,
    #[serde(default)]
    conflicting_class_groups: HashMap<String, Vec<String>>,
    #[serde(default)]
    conflicting_class_group_modifiers: HashMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct RawObjectEntry {
    key: String,
    definitions: Vec<RawDefinition>,
}

#[derive(Deserialize)]
#[serde(tag = "type", content = "value")]
enum RawDefinition {
    #[serde(rename = "literal")]
    Literal(String),
    #[serde(rename = "validator")]
    Validator(String),
    #[serde(rename = "theme")]
    Theme(String),
    #[serde(rename = "object")]
    Object(Vec<RawObjectEntry>),
}

impl Config {
    pub fn from_json(json: &str) -> Result<Self, ConfigError> {
        let raw: RawConfig = serde_json::from_str(json)?;
        Self::from_raw(raw)
    }

    pub fn clone_default() -> Self {
        DEFAULT_CONFIG.clone()
    }

    fn from_raw(raw: RawConfig) -> Result<Self, ConfigError> {
        Ok(Self {
            cache_size: raw.cache_size,
            prefix: raw.prefix,
            order_sensitive_modifiers: raw.order_sensitive_modifiers,
            theme: convert_map(raw.theme)?,
            class_groups: convert_map(raw.class_groups)?,
            conflicting_class_groups: raw.conflicting_class_groups,
            conflicting_class_group_modifiers: raw.conflicting_class_group_modifiers,
        })
    }
}

fn convert_map(
    input: HashMap<String, Vec<RawDefinition>>,
) -> Result<HashMap<String, Vec<ClassDefinition>>, ConfigError> {
    input
        .into_iter()
        .map(|(key, defs)| Ok((key, convert_definitions(defs)?)))
        .collect()
}

fn convert_definitions(
    definitions: Vec<RawDefinition>,
) -> Result<Vec<ClassDefinition>, ConfigError> {
    definitions
        .into_iter()
        .map(convert_definition)
        .collect::<Result<_, _>>()
}

fn convert_definition(def: RawDefinition) -> Result<ClassDefinition, ConfigError> {
    match def {
        RawDefinition::Literal(value) => Ok(ClassDefinition::Literal(value)),
        RawDefinition::Validator(name) => {
            if let Some(validator) = validator_by_name(&name) {
                Ok(ClassDefinition::Validator(validator))
            } else {
                Err(ConfigError::UnknownValidator(name))
            }
        }
        RawDefinition::Theme(key) => Ok(ClassDefinition::Theme(key)),
        RawDefinition::Object(entries) => Ok(ClassDefinition::Object(
            entries
                .into_iter()
                .map(|entry| {
                    Ok(ClassDefinitionEntry {
                        key: entry.key,
                        definitions: convert_definitions(entry.definitions)?,
                    })
                })
                .collect::<Result<_, ConfigError>>()?,
        )),
    }
}

pub static DEFAULT_CONFIG: Lazy<Config> = Lazy::new(|| {
    Config::from_json(include_str!("../../data/default_config.json"))
        .expect("default Tailwind config to be valid")
});
