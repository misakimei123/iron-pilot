use core::fmt;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use ironpilot_application::{
    ConfigValidationError, DeploymentEnvironment, EnvironmentFingerprint, RuntimeConfig,
    StartupIdentity, ValidatedRuntimeConfig,
};

use crate::deepseek::DEEPSEEK_API_KEY_ENV;

pub const CONFIG_PATH_ENV: &str = "IRONPILOT_CONFIG_PATH";
pub const ENVIRONMENT_NAME_ENV: &str = "IRONPILOT_ENVIRONMENT";
pub const ENVIRONMENT_FINGERPRINT_ENV: &str = "IRONPILOT_ENVIRONMENT_FINGERPRINT";
const ENVIRONMENT_PREFIX: &str = "IRONPILOT_";
const MAX_CONFIG_BYTES: u64 = 64 * 1_024;

pub fn load_startup_config() -> Result<ValidatedRuntimeConfig, LoadConfigError> {
    load_startup_config_from_vars(std::env::vars_os())
}

pub fn load_startup_config_from_vars<I, K, V>(
    variables: I,
) -> Result<ValidatedRuntimeConfig, LoadConfigError>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<OsString>,
    V: Into<OsString>,
{
    let variables = collect_ironpilot_variables(variables)?;
    let path = required_variable(&variables, CONFIG_PATH_ENV)?;
    let environment = required_variable(&variables, ENVIRONMENT_NAME_ENV)?;
    let fingerprint = required_variable(&variables, ENVIRONMENT_FINGERPRINT_ENV)?;

    let environment =
        DeploymentEnvironment::from_str(environment).map_err(LoadConfigError::Validation)?;
    let fingerprint =
        EnvironmentFingerprint::from_str(fingerprint).map_err(LoadConfigError::Validation)?;
    let identity = StartupIdentity::new(environment, fingerprint);

    load_from_path(Path::new(path), &identity)
}

pub fn parse_and_validate_yaml(
    yaml: &str,
    identity: &StartupIdentity,
) -> Result<ValidatedRuntimeConfig, LoadConfigError> {
    let config = parse_yaml_config(yaml)?;
    config
        .validate_for_startup(identity)
        .map_err(LoadConfigError::Validation)
}

pub fn parse_yaml_config(yaml: &str) -> Result<RuntimeConfig, LoadConfigError> {
    let actual_bytes = u64::try_from(yaml.len()).unwrap_or(u64::MAX);
    if actual_bytes > MAX_CONFIG_BYTES {
        return Err(LoadConfigError::ConfigTooLarge {
            bytes: actual_bytes,
            maximum: MAX_CONFIG_BYTES,
        });
    }

    let mut parser_config = noyalib::ParserConfig::strict();
    parser_config.max_document_length =
        usize::try_from(MAX_CONFIG_BYTES).expect("configuration byte limit fits usize");
    parser_config.max_total_scalar_bytes = parser_config.max_document_length;
    parser_config.max_documents = 1;
    parser_config.max_depth = 16;
    parser_config.max_alias_expansions = 0;
    parser_config.max_mapping_keys = 128;
    parser_config.max_sequence_length = 16;
    parser_config.max_events = 2_048;
    parser_config.max_nodes = 512;
    parser_config.max_merge_keys = 0;

    let mut documents =
        noyalib::document::load_all_with_config(yaml, &parser_config).map_err(yaml_error)?;
    if documents.len() != 1 {
        return Err(LoadConfigError::Yaml {
            message: "configuration must contain exactly one YAML document".into(),
        });
    }
    let document = documents
        .next()
        .ok_or_else(|| LoadConfigError::Yaml {
            message: "configuration document is unavailable".into(),
        })?
        .map_err(yaml_error)?;
    noyalib::from_value(&document).map_err(yaml_error)
}

fn load_from_path(
    path: &Path,
    identity: &StartupIdentity,
) -> Result<ValidatedRuntimeConfig, LoadConfigError> {
    let metadata = fs::metadata(path).map_err(|error| io_error(path, &error))?;
    if !metadata.is_file() {
        return Err(LoadConfigError::NotAFile {
            path: path.to_path_buf(),
        });
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err(LoadConfigError::ConfigTooLarge {
            bytes: metadata.len(),
            maximum: MAX_CONFIG_BYTES,
        });
    }

    let yaml = fs::read_to_string(path).map_err(|error| io_error(path, &error))?;
    let actual_bytes = u64::try_from(yaml.len()).unwrap_or(u64::MAX);
    if actual_bytes > MAX_CONFIG_BYTES {
        return Err(LoadConfigError::ConfigTooLarge {
            bytes: actual_bytes,
            maximum: MAX_CONFIG_BYTES,
        });
    }

    parse_and_validate_yaml(&yaml, identity)
}

fn collect_ironpilot_variables<I, K, V>(
    variables: I,
) -> Result<BTreeMap<OsString, OsString>, LoadConfigError>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<OsString>,
    V: Into<OsString>,
{
    let mut result = BTreeMap::new();
    for (key, value) in variables {
        let key = key.into();
        let Some(key_text) = key.to_str() else {
            continue;
        };
        if !key_text.starts_with(ENVIRONMENT_PREFIX) {
            continue;
        }
        if !matches!(
            key_text,
            CONFIG_PATH_ENV
                | ENVIRONMENT_NAME_ENV
                | ENVIRONMENT_FINGERPRINT_ENV
                | DEEPSEEK_API_KEY_ENV
        ) {
            return Err(LoadConfigError::UnknownEnvironmentVariable {
                name: key_text.into(),
            });
        }
        result.insert(key, value.into());
    }
    Ok(result)
}

fn required_variable<'a>(
    variables: &'a BTreeMap<OsString, OsString>,
    name: &'static str,
) -> Result<&'a str, LoadConfigError> {
    let value = variables
        .get(OsStr::new(name))
        .ok_or(LoadConfigError::MissingEnvironmentVariable { name })?;
    let value = value
        .to_str()
        .ok_or(LoadConfigError::NonUnicodeEnvironmentVariable { name })?;
    if value.is_empty() {
        return Err(LoadConfigError::EmptyEnvironmentVariable { name });
    }
    Ok(value)
}

fn io_error(path: &Path, error: &io::Error) -> LoadConfigError {
    LoadConfigError::Io {
        path: path.to_path_buf(),
        kind: error.kind(),
    }
}

fn yaml_error(error: noyalib::Error) -> LoadConfigError {
    LoadConfigError::Yaml {
        message: error.to_string().into(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoadConfigError {
    MissingEnvironmentVariable { name: &'static str },
    EmptyEnvironmentVariable { name: &'static str },
    NonUnicodeEnvironmentVariable { name: &'static str },
    UnknownEnvironmentVariable { name: Box<str> },
    Io { path: PathBuf, kind: io::ErrorKind },
    NotAFile { path: PathBuf },
    ConfigTooLarge { bytes: u64, maximum: u64 },
    Yaml { message: Box<str> },
    Validation(ConfigValidationError),
}

impl fmt::Display for LoadConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnvironmentVariable { name } => {
                write!(formatter, "required environment variable {name} is missing")
            }
            Self::EmptyEnvironmentVariable { name } => {
                write!(formatter, "required environment variable {name} is empty")
            }
            Self::NonUnicodeEnvironmentVariable { name } => {
                write!(
                    formatter,
                    "environment variable {name} is not valid Unicode"
                )
            }
            Self::UnknownEnvironmentVariable { name } => {
                write!(formatter, "unknown IronPilot environment variable {name}")
            }
            Self::Io { path, kind } => {
                write!(
                    formatter,
                    "cannot read configuration {}: {kind}",
                    path.display()
                )
            }
            Self::NotAFile { path } => {
                write!(
                    formatter,
                    "configuration path {} is not a file",
                    path.display()
                )
            }
            Self::ConfigTooLarge { bytes, maximum } => {
                write!(
                    formatter,
                    "configuration is {bytes} bytes and exceeds {maximum}"
                )
            }
            Self::Yaml { message } => write!(formatter, "YAML configuration rejected: {message}"),
            Self::Validation(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for LoadConfigError {}
