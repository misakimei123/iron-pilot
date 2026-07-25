use core::fmt;
use core::str::FromStr;
use std::collections::BTreeSet;

use ironpilot_domain::{
    DomainDecimal, InstrumentId, InstrumentType, RISK_RULES_VERSION_V1,
    STRATEGY_SPACE_VERSION_V1_VS,
};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const CONFIG_SCHEMA_VERSION_V1: &str = "ironpilot-config-v1";
pub const MARKET_FEATURES_VERSION_V1: &str = "ironpilot-market-features-v1";

const MAX_TARGET_CPU_CORES: u16 = 2;
const MAX_TARGET_MEMORY_MB: u32 = 2_048;
const MAX_MEMORY_SOFT_LIMIT_MB: u32 = 1_400;
const MAX_ENABLED_INSTRUMENTS: u8 = 3;
const MAX_ACTIVE_TRADE_PLANS: u8 = 2;
const MAX_LLM_CONCURRENCY: u8 = 1;
const MAX_LLM_DAILY_CALLS: u32 = 40;
const MAX_LLM_DAILY_TOKENS: u32 = 200_000;
const MAX_CANDLE_WINDOW: u16 = 500;
const MAX_TIMEFRAMES_PER_INSTRUMENT: u8 = 2;
const MAX_SQLITE_CONNECTIONS: u8 = 4;
const MAX_SQLITE_WRITE_CONCURRENCY: u8 = 1;
const MAX_MARKET_EVENT_CAPACITY: u16 = 1_024;
const MAX_CRITICAL_EVENT_CAPACITY: u16 = 256;
const MIN_FINGERPRINT_LENGTH: usize = 8;
const MAX_FINGERPRINT_LENGTH: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentEnvironment {
    Development,
    Paper,
}

impl fmt::Display for DeploymentEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Development => formatter.write_str("development"),
            Self::Paper => formatter.write_str("paper"),
        }
    }
}

impl FromStr for DeploymentEnvironment {
    type Err = ConfigValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "development" => Ok(Self::Development),
            "paper" => Ok(Self::Paper),
            _ => Err(ConfigValidationError::UnknownEnvironment {
                value: value.into(),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    ObserveOnly,
    Paper,
    Testnet,
    Live,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EnvironmentFingerprint(Box<str>);

impl EnvironmentFingerprint {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for EnvironmentFingerprint {
    type Err = ConfigValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let length = value.len();
        if !(MIN_FINGERPRINT_LENGTH..=MAX_FINGERPRINT_LENGTH).contains(&length)
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.')
            })
        {
            return Err(ConfigValidationError::InvalidEnvironmentFingerprint);
        }

        Ok(Self(value.into()))
    }
}

impl Serialize for EnvironmentFingerprint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for EnvironmentFingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupIdentity {
    environment: DeploymentEnvironment,
    fingerprint: EnvironmentFingerprint,
}

impl StartupIdentity {
    #[must_use]
    pub const fn new(
        environment: DeploymentEnvironment,
        fingerprint: EnvironmentFingerprint,
    ) -> Self {
        Self {
            environment,
            fingerprint,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    schema_version: Box<str>,
    environment: EnvironmentConfig,
    permissions: PermissionConfig,
    versions: VersionConfig,
    instruments: Vec<InstrumentConfig>,
    runtime: RuntimeLimits,
    llm: LlmLimits,
    market: MarketLimits,
    storage: StorageLimits,
    queues: QueueLimits,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct EnvironmentConfig {
    name: DeploymentEnvironment,
    fingerprint: EnvironmentFingerprint,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PermissionConfig {
    execution_mode: ExecutionMode,
    ai_strategy_decisions: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VersionConfig {
    market_features: Box<str>,
    strategy_space: Box<str>,
    risk_rules: Box<str>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct InstrumentConfig {
    id: InstrumentId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeLimits {
    target_cpu_cores: u16,
    target_memory_mb: u32,
    memory_soft_limit_mb: u32,
    max_enabled_instruments: u8,
    max_active_trade_plans: u8,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LlmLimits {
    max_concurrency: u8,
    daily_call_limit: u32,
    daily_token_limit: u32,
    daily_cost_limit_usd: DomainDecimal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MarketLimits {
    candle_window_per_timeframe: u16,
    max_timeframes_per_instrument: u8,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StorageLimits {
    sqlite_max_connections: u8,
    sqlite_write_concurrency: u8,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct QueueLimits {
    market_event_capacity_per_instrument: u16,
    critical_event_capacity: u16,
}

impl RuntimeConfig {
    pub fn validate_for_startup(
        self,
        identity: &StartupIdentity,
    ) -> Result<ValidatedRuntimeConfig, ConfigValidationError> {
        self.validate_identity_and_versions(identity)?;
        self.validate_permissions()?;
        self.validate_instruments()?;
        self.validate_resource_limits()?;
        Ok(ValidatedRuntimeConfig(self))
    }

    fn validate_identity_and_versions(
        &self,
        identity: &StartupIdentity,
    ) -> Result<(), ConfigValidationError> {
        if self.schema_version.as_ref() != CONFIG_SCHEMA_VERSION_V1 {
            return Err(ConfigValidationError::UnsupportedVersion {
                field: "schema_version",
                value: self.schema_version.clone(),
            });
        }
        if self.environment.name != identity.environment {
            return Err(ConfigValidationError::EnvironmentMismatch {
                expected: identity.environment,
                actual: self.environment.name,
            });
        }
        if self.environment.fingerprint != identity.fingerprint {
            return Err(ConfigValidationError::EnvironmentFingerprintMismatch);
        }
        for (field, actual, expected) in [
            (
                "versions.market_features",
                self.versions.market_features.as_ref(),
                MARKET_FEATURES_VERSION_V1,
            ),
            (
                "versions.strategy_space",
                self.versions.strategy_space.as_ref(),
                STRATEGY_SPACE_VERSION_V1_VS,
            ),
            (
                "versions.risk_rules",
                self.versions.risk_rules.as_ref(),
                RISK_RULES_VERSION_V1,
            ),
        ] {
            if actual != expected {
                return Err(ConfigValidationError::UnsupportedVersion {
                    field,
                    value: actual.into(),
                });
            }
        }
        Ok(())
    }

    fn validate_permissions(&self) -> Result<(), ConfigValidationError> {
        if self.permissions.execution_mode > ExecutionMode::Paper {
            return Err(ConfigValidationError::ExecutionModeNotAuthorized {
                mode: self.permissions.execution_mode,
            });
        }
        Ok(())
    }

    fn validate_instruments(&self) -> Result<(), ConfigValidationError> {
        let count = self.instruments.len();
        if count == 0 || count > usize::from(self.runtime.max_enabled_instruments) {
            return Err(ConfigValidationError::InstrumentCountOutOfRange { count });
        }

        let mut unique = BTreeSet::new();
        for instrument in &self.instruments {
            if instrument.id.instrument_type() != InstrumentType::Spot {
                return Err(ConfigValidationError::NonSpotInstrument {
                    instrument_id: instrument.id.clone(),
                });
            }
            if !unique.insert(instrument.id.clone()) {
                return Err(ConfigValidationError::DuplicateInstrument {
                    instrument_id: instrument.id.clone(),
                });
            }
        }
        Ok(())
    }

    fn validate_resource_limits(&self) -> Result<(), ConfigValidationError> {
        ensure_limit(
            "runtime.target_cpu_cores",
            self.runtime.target_cpu_cores,
            MAX_TARGET_CPU_CORES,
        )?;
        ensure_limit(
            "runtime.target_memory_mb",
            self.runtime.target_memory_mb,
            MAX_TARGET_MEMORY_MB,
        )?;
        ensure_limit(
            "runtime.memory_soft_limit_mb",
            self.runtime.memory_soft_limit_mb,
            MAX_MEMORY_SOFT_LIMIT_MB,
        )?;
        if self.runtime.memory_soft_limit_mb > self.runtime.target_memory_mb {
            return Err(ConfigValidationError::MemorySoftLimitExceedsTarget);
        }
        ensure_limit(
            "runtime.max_enabled_instruments",
            self.runtime.max_enabled_instruments,
            MAX_ENABLED_INSTRUMENTS,
        )?;
        ensure_limit(
            "runtime.max_active_trade_plans",
            self.runtime.max_active_trade_plans,
            MAX_ACTIVE_TRADE_PLANS,
        )?;
        ensure_limit(
            "llm.max_concurrency",
            self.llm.max_concurrency,
            MAX_LLM_CONCURRENCY,
        )?;
        ensure_limit(
            "llm.daily_call_limit",
            self.llm.daily_call_limit,
            MAX_LLM_DAILY_CALLS,
        )?;
        ensure_limit(
            "llm.daily_token_limit",
            self.llm.daily_token_limit,
            MAX_LLM_DAILY_TOKENS,
        )?;
        let max_cost = DomainDecimal::from_mantissa_scale(200, 2)
            .expect("the fixed maximum cost must fit DomainDecimal");
        if self.llm.daily_cost_limit_usd < DomainDecimal::ZERO
            || self.llm.daily_cost_limit_usd > max_cost
        {
            return Err(ConfigValidationError::DecimalLimitOutOfRange {
                field: "llm.daily_cost_limit_usd",
                maximum: max_cost,
            });
        }
        ensure_limit(
            "market.candle_window_per_timeframe",
            self.market.candle_window_per_timeframe,
            MAX_CANDLE_WINDOW,
        )?;
        ensure_limit(
            "market.max_timeframes_per_instrument",
            self.market.max_timeframes_per_instrument,
            MAX_TIMEFRAMES_PER_INSTRUMENT,
        )?;
        ensure_limit(
            "storage.sqlite_max_connections",
            self.storage.sqlite_max_connections,
            MAX_SQLITE_CONNECTIONS,
        )?;
        ensure_limit(
            "storage.sqlite_write_concurrency",
            self.storage.sqlite_write_concurrency,
            MAX_SQLITE_WRITE_CONCURRENCY,
        )?;
        ensure_limit(
            "queues.market_event_capacity_per_instrument",
            self.queues.market_event_capacity_per_instrument,
            MAX_MARKET_EVENT_CAPACITY,
        )?;
        ensure_limit(
            "queues.critical_event_capacity",
            self.queues.critical_event_capacity,
            MAX_CRITICAL_EVENT_CAPACITY,
        )?;
        Ok(())
    }
}

fn ensure_limit<T>(field: &'static str, value: T, maximum: T) -> Result<(), ConfigValidationError>
where
    T: Copy + Into<u64> + Ord,
{
    let numeric_value = value.into();
    if numeric_value == 0 || value > maximum {
        return Err(ConfigValidationError::NumericLimitOutOfRange {
            field,
            value: numeric_value,
            maximum: maximum.into(),
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedRuntimeConfig(RuntimeConfig);

impl ValidatedRuntimeConfig {
    #[must_use]
    pub fn as_config(&self) -> &RuntimeConfig {
        &self.0
    }

    #[must_use]
    pub fn instrument_ids(&self) -> impl ExactSizeIterator<Item = &InstrumentId> {
        self.0.instruments.iter().map(|instrument| &instrument.id)
    }

    #[must_use]
    pub const fn environment(&self) -> DeploymentEnvironment {
        self.0.environment.name
    }

    #[must_use]
    pub fn environment_fingerprint(&self) -> &EnvironmentFingerprint {
        &self.0.environment.fingerprint
    }

    #[must_use]
    pub fn permissions(&self) -> &PermissionConfig {
        &self.0.permissions
    }

    #[must_use]
    pub fn versions(&self) -> &VersionConfig {
        &self.0.versions
    }

    #[must_use]
    pub fn runtime_limits(&self) -> &RuntimeLimits {
        &self.0.runtime
    }

    #[must_use]
    pub fn llm_limits(&self) -> &LlmLimits {
        &self.0.llm
    }

    #[must_use]
    pub fn market_limits(&self) -> &MarketLimits {
        &self.0.market
    }

    #[must_use]
    pub fn storage_limits(&self) -> &StorageLimits {
        &self.0.storage
    }

    #[must_use]
    pub fn queue_limits(&self) -> &QueueLimits {
        &self.0.queues
    }

    pub fn validate_reload(
        &self,
        candidate: RuntimeConfig,
        identity: &StartupIdentity,
    ) -> Result<Self, ConfigValidationError> {
        let candidate = candidate.validate_for_startup(identity)?;
        let current = &self.0;
        let next = &candidate.0;

        for (field, changed) in [
            (
                "schema_version",
                current.schema_version != next.schema_version,
            ),
            ("environment", current.environment != next.environment),
            ("versions", current.versions != next.versions),
        ] {
            if changed {
                return Err(ConfigValidationError::ImmutableFieldChanged { field });
            }
        }

        if next.permissions.execution_mode > current.permissions.execution_mode {
            return Err(ConfigValidationError::PermissionExpansion {
                field: "permissions.execution_mode",
            });
        }
        if next.permissions.ai_strategy_decisions && !current.permissions.ai_strategy_decisions {
            return Err(ConfigValidationError::PermissionExpansion {
                field: "permissions.ai_strategy_decisions",
            });
        }

        let current_instruments: BTreeSet<_> = self.instrument_ids().cloned().collect();
        let next_instruments: BTreeSet<_> = candidate.instrument_ids().cloned().collect();
        if !next_instruments.is_subset(&current_instruments) {
            return Err(ConfigValidationError::PermissionExpansion {
                field: "instruments",
            });
        }

        ensure_reload_not_increased(&current.runtime, &next.runtime)?;
        ensure_reload_not_increased(&current.llm, &next.llm)?;
        ensure_reload_not_increased(&current.market, &next.market)?;
        ensure_reload_not_increased(&current.storage, &next.storage)?;
        ensure_reload_not_increased(&current.queues, &next.queues)?;

        Ok(candidate)
    }
}

impl PermissionConfig {
    #[must_use]
    pub const fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    #[must_use]
    pub const fn ai_strategy_decisions(&self) -> bool {
        self.ai_strategy_decisions
    }
}

impl VersionConfig {
    #[must_use]
    pub fn market_features(&self) -> &str {
        &self.market_features
    }

    #[must_use]
    pub fn strategy_space(&self) -> &str {
        &self.strategy_space
    }

    #[must_use]
    pub fn risk_rules(&self) -> &str {
        &self.risk_rules
    }
}

impl RuntimeLimits {
    #[must_use]
    pub const fn target_cpu_cores(&self) -> u16 {
        self.target_cpu_cores
    }

    #[must_use]
    pub const fn target_memory_mb(&self) -> u32 {
        self.target_memory_mb
    }

    #[must_use]
    pub const fn memory_soft_limit_mb(&self) -> u32 {
        self.memory_soft_limit_mb
    }

    #[must_use]
    pub const fn max_enabled_instruments(&self) -> u8 {
        self.max_enabled_instruments
    }

    #[must_use]
    pub const fn max_active_trade_plans(&self) -> u8 {
        self.max_active_trade_plans
    }
}

impl LlmLimits {
    #[must_use]
    pub const fn max_concurrency(&self) -> u8 {
        self.max_concurrency
    }

    #[must_use]
    pub const fn daily_call_limit(&self) -> u32 {
        self.daily_call_limit
    }

    #[must_use]
    pub const fn daily_token_limit(&self) -> u32 {
        self.daily_token_limit
    }

    #[must_use]
    pub const fn daily_cost_limit_usd(&self) -> DomainDecimal {
        self.daily_cost_limit_usd
    }
}

impl MarketLimits {
    #[must_use]
    pub const fn candle_window_per_timeframe(&self) -> u16 {
        self.candle_window_per_timeframe
    }

    #[must_use]
    pub const fn max_timeframes_per_instrument(&self) -> u8 {
        self.max_timeframes_per_instrument
    }
}

impl StorageLimits {
    #[must_use]
    pub const fn sqlite_max_connections(&self) -> u8 {
        self.sqlite_max_connections
    }

    #[must_use]
    pub const fn sqlite_write_concurrency(&self) -> u8 {
        self.sqlite_write_concurrency
    }
}

impl QueueLimits {
    #[must_use]
    pub const fn market_event_capacity_per_instrument(&self) -> u16 {
        self.market_event_capacity_per_instrument
    }

    #[must_use]
    pub const fn critical_event_capacity(&self) -> u16 {
        self.critical_event_capacity
    }
}

trait ConservativeReload {
    fn first_increased_field(&self, candidate: &Self) -> Option<&'static str>;
}

fn ensure_reload_not_increased<T: ConservativeReload>(
    current: &T,
    candidate: &T,
) -> Result<(), ConfigValidationError> {
    if let Some(field) = current.first_increased_field(candidate) {
        return Err(ConfigValidationError::ResourceExpansion { field });
    }
    Ok(())
}

macro_rules! conservative_reload {
    ($type:ty, $( $field:ident ),+ $(,)?) => {
        impl ConservativeReload for $type {
            fn first_increased_field(&self, candidate: &Self) -> Option<&'static str> {
                $(
                    if candidate.$field > self.$field {
                        return Some(concat!(stringify!($type), ".", stringify!($field)));
                    }
                )+
                None
            }
        }
    };
}

conservative_reload!(
    RuntimeLimits,
    target_cpu_cores,
    target_memory_mb,
    memory_soft_limit_mb,
    max_enabled_instruments,
    max_active_trade_plans,
);
conservative_reload!(
    LlmLimits,
    max_concurrency,
    daily_call_limit,
    daily_token_limit,
    daily_cost_limit_usd,
);
conservative_reload!(
    MarketLimits,
    candle_window_per_timeframe,
    max_timeframes_per_instrument,
);
conservative_reload!(
    StorageLimits,
    sqlite_max_connections,
    sqlite_write_concurrency,
);
conservative_reload!(
    QueueLimits,
    market_event_capacity_per_instrument,
    critical_event_capacity,
);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigValidationError {
    UnknownEnvironment {
        value: Box<str>,
    },
    InvalidEnvironmentFingerprint,
    EnvironmentMismatch {
        expected: DeploymentEnvironment,
        actual: DeploymentEnvironment,
    },
    EnvironmentFingerprintMismatch,
    UnsupportedVersion {
        field: &'static str,
        value: Box<str>,
    },
    ExecutionModeNotAuthorized {
        mode: ExecutionMode,
    },
    InstrumentCountOutOfRange {
        count: usize,
    },
    NonSpotInstrument {
        instrument_id: InstrumentId,
    },
    DuplicateInstrument {
        instrument_id: InstrumentId,
    },
    NumericLimitOutOfRange {
        field: &'static str,
        value: u64,
        maximum: u64,
    },
    DecimalLimitOutOfRange {
        field: &'static str,
        maximum: DomainDecimal,
    },
    MemorySoftLimitExceedsTarget,
    ImmutableFieldChanged {
        field: &'static str,
    },
    PermissionExpansion {
        field: &'static str,
    },
    ResourceExpansion {
        field: &'static str,
    },
}

impl fmt::Display for ConfigValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownEnvironment { value } => {
                write!(formatter, "unknown deployment environment {value}")
            }
            Self::InvalidEnvironmentFingerprint => {
                formatter.write_str("invalid environment fingerprint")
            }
            Self::EnvironmentMismatch { expected, actual } => {
                write!(
                    formatter,
                    "configured environment {actual} does not match expected {expected}"
                )
            }
            Self::EnvironmentFingerprintMismatch => {
                formatter.write_str("configured environment fingerprint does not match")
            }
            Self::UnsupportedVersion { field, value } => {
                write!(formatter, "unsupported {field} value {value}")
            }
            Self::ExecutionModeNotAuthorized { mode } => {
                write!(formatter, "execution mode {mode:?} is not authorized")
            }
            Self::InstrumentCountOutOfRange { count } => {
                write!(
                    formatter,
                    "enabled instrument count {count} is outside 1..=3"
                )
            }
            Self::NonSpotInstrument { instrument_id } => {
                write!(
                    formatter,
                    "non-Spot instrument {instrument_id} is not allowed"
                )
            }
            Self::DuplicateInstrument { instrument_id } => {
                write!(formatter, "duplicate instrument {instrument_id}")
            }
            Self::NumericLimitOutOfRange {
                field,
                value,
                maximum,
            } => write!(formatter, "{field} value {value} is outside 1..={maximum}"),
            Self::DecimalLimitOutOfRange { field, maximum } => {
                write!(formatter, "{field} is outside 0..={maximum}")
            }
            Self::MemorySoftLimitExceedsTarget => {
                formatter.write_str("memory soft limit exceeds target memory")
            }
            Self::ImmutableFieldChanged { field } => {
                write!(
                    formatter,
                    "hot reload cannot change immutable field {field}"
                )
            }
            Self::PermissionExpansion { field } => {
                write!(
                    formatter,
                    "hot reload cannot expand permission through {field}"
                )
            }
            Self::ResourceExpansion { field } => {
                write!(
                    formatter,
                    "hot reload cannot increase resource limit {field}"
                )
            }
        }
    }
}

impl std::error::Error for ConfigValidationError {}
