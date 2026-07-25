use core::str::FromStr;
use std::num::NonZeroUsize;
use std::time::Duration;

use ironpilot_adapters::{ProcessResourceSampler, parse_and_validate_yaml};
use ironpilot_application::{
    BoundedQueueSender, DeploymentEnvironment, EnvironmentFingerprint, HealthIssue, HealthLevel,
    HealthMonitor, QueueSendError, ResourceSample, RuntimeEvent, RuntimeSupervisor, SpawnError,
    StartupIdentity, TaskFailure, TaskImportance, UnixMillis,
};
use ironpilot_domain::CorrelationId;

const VALID_YAML: &str = include_str!("../../../config/ironpilot.example.yaml");
const MEBIBYTE: u64 = 1_024 * 1_024;

fn identity() -> StartupIdentity {
    StartupIdentity::new(
        DeploymentEnvironment::Development,
        EnvironmentFingerprint::from_str("development-paper-local")
            .expect("fixture fingerprint is valid"),
    )
}

fn timestamp(value: i64) -> UnixMillis {
    UnixMillis::new(value).expect("fixture timestamp is valid")
}

fn correlation_id() -> CorrelationId {
    CorrelationId::from_str("a5125e6f-2e8f-45f7-93cc-8f168a03f10a")
        .expect("fixture correlation ID is valid")
}

#[test]
fn queue_capacities_match_the_resource_budget_and_saturation_is_visible() {
    let config =
        parse_and_validate_yaml(VALID_YAML, &identity()).expect("example config must be valid");
    let health = HealthMonitor::new(config.runtime_limits());
    health.record_resource_sample(ResourceSample::new(timestamp(1_000), MEBIBYTE, 1.0));

    let (market_sender, _market_receiver) =
        BoundedQueueSender::market(config.queue_limits(), &health);
    assert_eq!(market_sender.snapshot().capacity(), 1_024);
    for sequence in 0..1_024 {
        market_sender
            .try_send(RuntimeEvent::new(correlation_id(), sequence))
            .expect("events up to the configured capacity must be retained");
    }
    let rejected = market_sender
        .try_send(RuntimeEvent::new(correlation_id(), 1_024))
        .expect_err("the bounded market queue must reject overflow");
    assert!(matches!(rejected, QueueSendError::Full(_)));
    assert_eq!(rejected.into_event().into_payload(), 1_024);
    assert_eq!(market_sender.snapshot().depth(), 1_024);
    assert_eq!(market_sender.snapshot().high_watermark(), 1_024);

    let market_health = health.snapshot(timestamp(1_000), Duration::from_secs(1));
    assert_eq!(market_health.level(), HealthLevel::Degraded);
    assert!(!market_health.entries_allowed());
    assert!(
        market_health
            .issues()
            .contains(&HealthIssue::MarketQueueSaturated)
    );

    let (critical_sender, _critical_receiver) =
        BoundedQueueSender::critical(config.queue_limits(), &health);
    assert_eq!(critical_sender.snapshot().capacity(), 256);
    for sequence in 0..256 {
        critical_sender
            .try_send(RuntimeEvent::new(correlation_id(), sequence))
            .expect("critical events up to the configured capacity must be retained");
    }
    let rejected = critical_sender
        .try_send(RuntimeEvent::new(correlation_id(), 256))
        .expect_err("critical overflow must be returned to the caller");
    assert_eq!(rejected.event().correlation_id(), correlation_id());
    assert_eq!(rejected.into_event().into_payload(), 256);

    let critical_health = health.snapshot(timestamp(1_000), Duration::from_secs(1));
    assert_eq!(critical_health.level(), HealthLevel::Halted);
    assert_eq!(
        critical_health.recommended_state(),
        ironpilot_domain::SystemState::Halted
    );
    assert!(
        critical_health
            .issues()
            .contains(&HealthIssue::CriticalQueueSaturated)
    );
}

#[test]
fn health_requires_fresh_metrics_and_disables_new_work_above_the_memory_soft_limit() {
    let config =
        parse_and_validate_yaml(VALID_YAML, &identity()).expect("example config must be valid");
    let health = HealthMonitor::new(config.runtime_limits());

    let unavailable = health.snapshot(timestamp(1_000), Duration::from_secs(1));
    assert!(unavailable.process_alive());
    assert_eq!(unavailable.level(), HealthLevel::Untrusted);
    assert!(!unavailable.entries_allowed());
    assert!(!unavailable.new_ai_allowed());

    health.record_resource_sample(ResourceSample::new(
        timestamp(1_000),
        1_399 * MEBIBYTE,
        12.5,
    ));
    let healthy = health.snapshot(timestamp(1_500), Duration::from_secs(1));
    assert_eq!(healthy.level(), HealthLevel::Healthy);
    assert!(healthy.entries_allowed());
    assert!(healthy.new_ai_allowed());

    let stale = health.snapshot(timestamp(2_001), Duration::from_secs(1));
    assert_eq!(stale.level(), HealthLevel::Untrusted);
    assert!(stale.issues().contains(&HealthIssue::ResourceMetricsStale));

    health.record_resource_sample(ResourceSample::new(
        timestamp(3_000),
        1_400 * MEBIBYTE + 1,
        10.0,
    ));
    let over_limit = health.snapshot(timestamp(3_000), Duration::from_secs(1));
    assert_eq!(over_limit.level(), HealthLevel::Degraded);
    assert!(!over_limit.entries_allowed());
    assert!(!over_limit.new_ai_allowed());
    assert!(
        over_limit
            .issues()
            .contains(&HealthIssue::MemorySoftLimitExceeded)
    );
}

#[test]
fn process_sampler_reports_current_process_rss_and_cpu() {
    let mut sampler = ProcessResourceSampler::new().expect("current process must be observable");
    let sample = sampler
        .sample(timestamp(1_000))
        .expect("current process metrics must be available");

    assert!(sample.resident_memory_bytes() > 0);
    assert!(sample.cpu_usage_percent().is_finite());
    assert!(!sample.cpu_usage_percent().is_sign_negative());
}

#[tokio::test]
async fn supervisor_bounds_tasks_and_distinguishes_graceful_from_forced_shutdown() {
    let config =
        parse_and_validate_yaml(VALID_YAML, &identity()).expect("example config must be valid");
    let health = HealthMonitor::new(config.runtime_limits());
    health.record_resource_sample(ResourceSample::new(timestamp(1_000), MEBIBYTE, 1.0));

    let mut graceful = RuntimeSupervisor::new(
        NonZeroUsize::new(1).expect("one is non-zero"),
        health.clone(),
    );
    let mut shutdown = graceful.shutdown_signal();
    graceful
        .spawn("cooperative", TaskImportance::Critical, async move {
            shutdown.cancelled().await;
            Ok(())
        })
        .expect("first task fits the supervisor");
    assert_eq!(
        graceful.spawn("overflow", TaskImportance::Supporting, async { Ok(()) }),
        Err(SpawnError::TaskLimitReached { limit: 1 })
    );
    let report = graceful.shutdown(Duration::from_secs(1)).await;
    assert!(report.graceful());
    assert_eq!(report.completed(), 1);
    assert_eq!(report.failed(), 0);
    assert_eq!(report.forced(), 0);

    let forced_health = HealthMonitor::new(config.runtime_limits());
    let mut forced = RuntimeSupervisor::new(
        NonZeroUsize::new(1).expect("one is non-zero"),
        forced_health.clone(),
    );
    forced
        .spawn("stuck", TaskImportance::Critical, async {
            std::future::pending::<()>().await;
            Ok(())
        })
        .expect("first task fits the supervisor");
    let report = forced.shutdown(Duration::ZERO).await;
    assert!(!report.graceful());
    assert_eq!(report.forced(), 1);
    assert!(
        forced_health
            .snapshot(timestamp(1_000), Duration::from_secs(1))
            .issues()
            .contains(&HealthIssue::ShutdownTimedOut)
    );
}

#[tokio::test]
async fn critical_task_failure_halts_runtime_health() {
    let config =
        parse_and_validate_yaml(VALID_YAML, &identity()).expect("example config must be valid");
    let health = HealthMonitor::new(config.runtime_limits());
    health.record_resource_sample(ResourceSample::new(timestamp(1_000), MEBIBYTE, 1.0));
    let mut supervisor = RuntimeSupervisor::new(
        NonZeroUsize::new(1).expect("one is non-zero"),
        health.clone(),
    );
    supervisor
        .spawn("critical-feed", TaskImportance::Critical, async {
            Err(TaskFailure::new("feed disconnected"))
        })
        .expect("task fits the supervisor");

    tokio::task::yield_now().await;
    assert_eq!(supervisor.reap_finished(), 1);
    let snapshot = health.snapshot(timestamp(1_000), Duration::from_secs(1));
    assert_eq!(snapshot.level(), HealthLevel::Halted);
    assert!(
        snapshot
            .issues()
            .contains(&HealthIssue::CriticalTaskFailed {
                task: "critical-feed".into(),
                message: "feed disconnected".into(),
            })
    );
}
