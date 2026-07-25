use core::str::FromStr;
use std::path::PathBuf;

use ironpilot_adapters::{
    CONFIG_PATH_ENV, ENVIRONMENT_FINGERPRINT_ENV, ENVIRONMENT_NAME_ENV, LoadConfigError,
    load_startup_config_from_vars, parse_and_validate_yaml, parse_yaml_config,
};
use ironpilot_application::{
    ConfigValidationError, DeploymentEnvironment, EnvironmentFingerprint, ExecutionMode,
    StartupIdentity,
};

const VALID_YAML: &str = include_str!("../../../config/ironpilot.example.yaml");

fn identity() -> StartupIdentity {
    StartupIdentity::new(
        DeploymentEnvironment::Development,
        EnvironmentFingerprint::from_str("development-paper-local")
            .expect("fixture fingerprint is valid"),
    )
}

#[test]
fn checked_in_yaml_matches_the_frozen_schema_and_defaults() {
    let config =
        parse_and_validate_yaml(VALID_YAML, &identity()).expect("example config must be valid");

    assert_eq!(config.instrument_ids().count(), 1);
    assert_eq!(
        config
            .instrument_ids()
            .next()
            .expect("one instrument")
            .to_string(),
        "bybit:spot:BTCUSDT"
    );
    assert_eq!(config.environment(), DeploymentEnvironment::Development);
    assert_eq!(
        config.environment_fingerprint().as_str(),
        "development-paper-local"
    );
    assert_eq!(config.permissions().execution_mode(), ExecutionMode::Paper);
    assert!(config.permissions().ai_trading_plans());
    assert_eq!(
        config.versions().market_features(),
        "ironpilot-market-features-v1"
    );
    assert_eq!(
        config.versions().ai_decision_context(),
        "ironpilot-ai-decision-context-v1"
    );
    assert_eq!(config.versions().ai_trading_plan(), "3.0");
    assert_eq!(config.runtime_limits().target_cpu_cores(), 2);
    assert_eq!(config.runtime_limits().target_memory_mb(), 2_048);
    assert_eq!(config.runtime_limits().memory_soft_limit_mb(), 1_400);
    assert_eq!(config.runtime_limits().max_enabled_instruments(), 3);
    assert_eq!(config.runtime_limits().max_active_trade_plans(), 2);
    assert_eq!(config.llm_limits().max_concurrency(), 1);
    assert_eq!(config.llm_limits().daily_call_limit(), 40);
    assert_eq!(config.llm_limits().daily_token_limit(), 200_000);
    assert_eq!(
        config.llm_limits().daily_cost_limit_usd().to_string(),
        "2.00"
    );
    assert_eq!(config.market_limits().candle_window_per_timeframe(), 500);
    assert_eq!(config.market_limits().max_timeframes_per_instrument(), 2);
    assert_eq!(config.storage_limits().sqlite_max_connections(), 4);
    assert_eq!(config.storage_limits().sqlite_write_concurrency(), 1);
    assert_eq!(
        config.queue_limits().market_event_capacity_per_instrument(),
        1_024
    );
    assert_eq!(config.queue_limits().critical_event_capacity(), 256);
}

#[test]
fn environment_name_and_fingerprint_must_match_the_process_identity() {
    let wrong_environment = StartupIdentity::new(
        DeploymentEnvironment::Paper,
        EnvironmentFingerprint::from_str("development-paper-local")
            .expect("fixture fingerprint is valid"),
    );
    assert!(matches!(
        parse_and_validate_yaml(VALID_YAML, &wrong_environment),
        Err(LoadConfigError::Validation(
            ConfigValidationError::EnvironmentMismatch { .. }
        ))
    ));

    let wrong_fingerprint = StartupIdentity::new(
        DeploymentEnvironment::Development,
        EnvironmentFingerprint::from_str("different-paper-host")
            .expect("fixture fingerprint is valid"),
    );
    assert_eq!(
        parse_and_validate_yaml(VALID_YAML, &wrong_fingerprint),
        Err(LoadConfigError::Validation(
            ConfigValidationError::EnvironmentFingerprintMismatch
        ))
    );

    let malformed_fingerprint =
        VALID_YAML.replace("development-paper-local", "INVALID FINGERPRINT");
    assert!(matches!(
        parse_and_validate_yaml(&malformed_fingerprint, &identity()),
        Err(LoadConfigError::Yaml { .. })
    ));
}

#[test]
fn unknown_semantic_versions_fail_closed() {
    for (current, unknown, field) in [
        (
            "ironpilot-config-v2",
            "ironpilot-config-v3",
            "schema_version",
        ),
        (
            "ironpilot-market-features-v1",
            "ironpilot-market-features-v2",
            "versions.market_features",
        ),
        (
            "ironpilot-ai-decision-context-v1",
            "ironpilot-ai-decision-context-v2",
            "versions.ai_decision_context",
        ),
        (
            "ai_trading_plan: \"3.0\"",
            "ai_trading_plan: \"4.0\"",
            "versions.ai_trading_plan",
        ),
    ] {
        let yaml = VALID_YAML.replacen(current, unknown, 1);
        assert!(matches!(
            parse_and_validate_yaml(&yaml, &identity()),
            Err(LoadConfigError::Validation(
                ConfigValidationError::UnsupportedVersion {
                    field: actual_field,
                    ..
                }
            )) if actual_field == field
        ));
    }
}

#[test]
fn instrument_scope_is_unique_spot_only_and_at_most_three() {
    let four_instruments = VALID_YAML.replace(
        "  - id: bybit:spot:BTCUSDT",
        "  - id: bybit:spot:BTCUSDT\n  - id: bybit:spot:ETHUSDT\n  - id: bybit:spot:SOLUSDT\n  - id: bybit:spot:XRPUSDT",
    );
    assert_eq!(
        parse_and_validate_yaml(&four_instruments, &identity()),
        Err(LoadConfigError::Validation(
            ConfigValidationError::InstrumentCountOutOfRange { count: 4 }
        ))
    );

    let perpetual = VALID_YAML.replace("bybit:spot:BTCUSDT", "bybit:linear_perpetual:BTCUSDT");
    assert!(matches!(
        parse_and_validate_yaml(&perpetual, &identity()),
        Err(LoadConfigError::Validation(
            ConfigValidationError::NonSpotInstrument { .. }
        ))
    ));

    let duplicate = VALID_YAML.replace(
        "  - id: bybit:spot:BTCUSDT",
        "  - id: bybit:spot:BTCUSDT\n  - id: bybit:spot:BTCUSDT",
    );
    assert!(matches!(
        parse_and_validate_yaml(&duplicate, &identity()),
        Err(LoadConfigError::Validation(
            ConfigValidationError::DuplicateInstrument { .. }
        ))
    ));
}

#[test]
fn spot_only_schema_rejects_leverage_margin_and_unknown_fields() {
    for illegal_field in [
        "    leverage: 2\n",
        "    margin_mode: isolated\n",
        "    position_mode: hedge\n",
    ] {
        let yaml = VALID_YAML.replace(
            "  - id: bybit:spot:BTCUSDT\n",
            &format!("  - id: bybit:spot:BTCUSDT\n{illegal_field}"),
        );
        assert!(matches!(
            parse_and_validate_yaml(&yaml, &identity()),
            Err(LoadConfigError::Yaml { .. })
        ));
    }
}

#[test]
fn yaml_contract_rejects_unknown_duplicate_and_multiple_documents() {
    let unknown = format!("{VALID_YAML}\nfuture_permission: true\n");
    assert!(parse_and_validate_yaml(&unknown, &identity()).is_err());

    let duplicate = VALID_YAML.replacen(
        "schema_version: ironpilot-config-v2",
        "schema_version: ironpilot-config-v2\nschema_version: ironpilot-config-v2",
        1,
    );
    assert!(parse_and_validate_yaml(&duplicate, &identity()).is_err());

    let multiple = format!("{VALID_YAML}\n---\nschema_version: ironpilot-config-v2\n");
    assert!(parse_and_validate_yaml(&multiple, &identity()).is_err());

    let alias = VALID_YAML
        .replace(
            "schema_version: ironpilot-config-v2",
            "schema_version: &schema ironpilot-config-v2",
        )
        .replace(
            "market_features: ironpilot-market-features-v1",
            "market_features: *schema",
        );
    assert!(parse_and_validate_yaml(&alias, &identity()).is_err());

    let oversized = "x".repeat(65_537);
    assert!(matches!(
        parse_and_validate_yaml(&oversized, &identity()),
        Err(LoadConfigError::ConfigTooLarge { .. })
    ));
}

#[test]
fn every_2c2g_resource_ceiling_is_enforced() {
    for (safe, excessive, field) in [
        (
            "target_cpu_cores: 2",
            "target_cpu_cores: 3",
            "runtime.target_cpu_cores",
        ),
        (
            "target_memory_mb: 2048",
            "target_memory_mb: 2049",
            "runtime.target_memory_mb",
        ),
        (
            "memory_soft_limit_mb: 1400",
            "memory_soft_limit_mb: 1401",
            "runtime.memory_soft_limit_mb",
        ),
        (
            "max_enabled_instruments: 3",
            "max_enabled_instruments: 4",
            "runtime.max_enabled_instruments",
        ),
        (
            "max_active_trade_plans: 2",
            "max_active_trade_plans: 3",
            "runtime.max_active_trade_plans",
        ),
        (
            "max_concurrency: 1",
            "max_concurrency: 2",
            "llm.max_concurrency",
        ),
        (
            "daily_call_limit: 40",
            "daily_call_limit: 41",
            "llm.daily_call_limit",
        ),
        (
            "daily_token_limit: 200000",
            "daily_token_limit: 200001",
            "llm.daily_token_limit",
        ),
        (
            "candle_window_per_timeframe: 500",
            "candle_window_per_timeframe: 501",
            "market.candle_window_per_timeframe",
        ),
        (
            "max_timeframes_per_instrument: 2",
            "max_timeframes_per_instrument: 3",
            "market.max_timeframes_per_instrument",
        ),
        (
            "sqlite_max_connections: 4",
            "sqlite_max_connections: 5",
            "storage.sqlite_max_connections",
        ),
        (
            "sqlite_write_concurrency: 1",
            "sqlite_write_concurrency: 2",
            "storage.sqlite_write_concurrency",
        ),
        (
            "market_event_capacity_per_instrument: 1024",
            "market_event_capacity_per_instrument: 1025",
            "queues.market_event_capacity_per_instrument",
        ),
        (
            "critical_event_capacity: 256",
            "critical_event_capacity: 257",
            "queues.critical_event_capacity",
        ),
    ] {
        let yaml = VALID_YAML.replacen(safe, excessive, 1);
        assert!(matches!(
            parse_and_validate_yaml(&yaml, &identity()),
            Err(LoadConfigError::Validation(
                ConfigValidationError::NumericLimitOutOfRange {
                    field: actual_field,
                    ..
                }
            )) if actual_field == field
        ));
    }

    let excessive_cost = VALID_YAML.replace(
        "daily_cost_limit_usd: \"2.00\"",
        "daily_cost_limit_usd: \"2.01\"",
    );
    assert!(matches!(
        parse_and_validate_yaml(&excessive_cost, &identity()),
        Err(LoadConfigError::Validation(
            ConfigValidationError::DecimalLimitOutOfRange {
                field: "llm.daily_cost_limit_usd",
                ..
            }
        ))
    ));

    let negative_cost = VALID_YAML.replace(
        "daily_cost_limit_usd: \"2.00\"",
        "daily_cost_limit_usd: \"-0.01\"",
    );
    assert!(matches!(
        parse_and_validate_yaml(&negative_cost, &identity()),
        Err(LoadConfigError::Validation(
            ConfigValidationError::DecimalLimitOutOfRange {
                field: "llm.daily_cost_limit_usd",
                ..
            }
        ))
    ));
}

#[test]
fn testnet_and_live_permissions_are_not_authorized() {
    for mode in ["testnet", "live"] {
        let yaml = VALID_YAML.replace("execution_mode: paper", &format!("execution_mode: {mode}"));
        assert_eq!(
            parse_and_validate_yaml(&yaml, &identity()),
            Err(LoadConfigError::Validation(
                ConfigValidationError::ExecutionModeNotAuthorized {
                    mode: if mode == "testnet" {
                        ExecutionMode::Testnet
                    } else {
                        ExecutionMode::Live
                    }
                }
            ))
        );
    }
}

#[test]
fn hot_reload_rejects_permission_instrument_and_resource_expansion() {
    let observe_yaml = VALID_YAML
        .replace("execution_mode: paper", "execution_mode: observe_only")
        .replace("ai_trading_plans: true", "ai_trading_plans: false")
        .replace("target_cpu_cores: 2", "target_cpu_cores: 1");
    let current =
        parse_and_validate_yaml(&observe_yaml, &identity()).expect("conservative config is valid");

    let paper_candidate = parse_yaml_config(
        &observe_yaml.replace("execution_mode: observe_only", "execution_mode: paper"),
    )
    .expect("candidate parses");
    assert_eq!(
        current.validate_reload(paper_candidate, &identity()),
        Err(ConfigValidationError::PermissionExpansion {
            field: "permissions.execution_mode"
        })
    );

    let ai_candidate = parse_yaml_config(
        &observe_yaml.replace("ai_trading_plans: false", "ai_trading_plans: true"),
    )
    .expect("candidate parses");
    assert_eq!(
        current.validate_reload(ai_candidate, &identity()),
        Err(ConfigValidationError::PermissionExpansion {
            field: "permissions.ai_trading_plans"
        })
    );

    let instrument_candidate = parse_yaml_config(&observe_yaml.replace(
        "  - id: bybit:spot:BTCUSDT",
        "  - id: bybit:spot:BTCUSDT\n  - id: bybit:spot:ETHUSDT",
    ))
    .expect("candidate parses");
    assert_eq!(
        current.validate_reload(instrument_candidate, &identity()),
        Err(ConfigValidationError::PermissionExpansion {
            field: "instruments"
        })
    );

    let resource_candidate =
        parse_yaml_config(&observe_yaml.replace("target_cpu_cores: 1", "target_cpu_cores: 2"))
            .expect("candidate parses");
    assert_eq!(
        current.validate_reload(resource_candidate, &identity()),
        Err(ConfigValidationError::ResourceExpansion {
            field: "RuntimeLimits.target_cpu_cores"
        })
    );
}

#[test]
fn hot_reload_accepts_only_monotonic_restrictions() {
    let two_instruments = VALID_YAML.replace(
        "  - id: bybit:spot:BTCUSDT",
        "  - id: bybit:spot:BTCUSDT\n  - id: bybit:spot:ETHUSDT",
    );
    let current =
        parse_and_validate_yaml(&two_instruments, &identity()).expect("current config is valid");
    let restricted = VALID_YAML
        .replace("execution_mode: paper", "execution_mode: observe_only")
        .replace("ai_trading_plans: true", "ai_trading_plans: false")
        .replace("target_cpu_cores: 2", "target_cpu_cores: 1")
        .replace("daily_call_limit: 40", "daily_call_limit: 20")
        .replace(
            "market_event_capacity_per_instrument: 1024",
            "market_event_capacity_per_instrument: 512",
        );

    let candidate = parse_yaml_config(&restricted).expect("candidate parses");
    let reloaded = current
        .validate_reload(candidate, &identity())
        .expect("restriction-only reload must pass");

    assert_eq!(reloaded.instrument_ids().count(), 1);
}

#[test]
fn environment_loader_is_strict_and_reads_the_yaml_path() {
    let vars = valid_environment_variables();
    assert!(load_startup_config_from_vars(vars.clone()).is_ok());

    let missing = vars
        .iter()
        .filter(|(key, _)| *key != ENVIRONMENT_FINGERPRINT_ENV)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        load_startup_config_from_vars(missing),
        Err(LoadConfigError::MissingEnvironmentVariable {
            name: ENVIRONMENT_FINGERPRINT_ENV
        })
    );

    let mut unknown = vars;
    unknown.push(("IRONPILOT_ENABLE_LIVE", "true".to_owned()));
    assert_eq!(
        load_startup_config_from_vars(unknown),
        Err(LoadConfigError::UnknownEnvironmentVariable {
            name: "IRONPILOT_ENABLE_LIVE".into()
        })
    );
}

fn valid_environment_variables() -> Vec<(&'static str, String)> {
    vec![
        (
            CONFIG_PATH_ENV,
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("config")
                .join("ironpilot.example.yaml")
                .display()
                .to_string(),
        ),
        (ENVIRONMENT_NAME_ENV, "development".to_owned()),
        (
            ENVIRONMENT_FINGERPRINT_ENV,
            "development-paper-local".to_owned(),
        ),
    ]
}
