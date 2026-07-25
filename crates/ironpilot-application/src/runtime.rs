use core::fmt;
use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ironpilot_domain::{CorrelationId, SystemState};
use tokio::sync::{mpsc, watch};
use tokio::task::{JoinError, JoinSet};
use tokio::time::{Instant, timeout_at};

use crate::{QueueLimits, RuntimeLimits, UnixMillis};

const BYTES_PER_MEBIBYTE: u64 = 1_024 * 1_024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum QueueClass {
    Market,
    Critical,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeEvent<T> {
    correlation_id: CorrelationId,
    payload: T,
}

impl<T> RuntimeEvent<T> {
    #[must_use]
    pub const fn new(correlation_id: CorrelationId, payload: T) -> Self {
        Self {
            correlation_id,
            payload,
        }
    }

    #[must_use]
    pub const fn correlation_id(&self) -> CorrelationId {
        self.correlation_id
    }

    #[must_use]
    pub const fn payload(&self) -> &T {
        &self.payload
    }

    #[must_use]
    pub fn into_payload(self) -> T {
        self.payload
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HealthIssue {
    ResourceMetricsUnavailable,
    ResourceMetricsStale,
    MemorySoftLimitExceeded,
    MarketQueueSaturated,
    CriticalQueueSaturated,
    QueueClosed(QueueClass),
    TaskExited(Box<str>),
    CriticalTaskExited(Box<str>),
    SupportingTaskFailed { task: Box<str>, message: Box<str> },
    CriticalTaskFailed { task: Box<str>, message: Box<str> },
    TaskPanicked(Box<str>),
    TaskCapacitySaturated,
    ShuttingDown,
    ShutdownTimedOut,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HealthLevel {
    Healthy,
    Degraded,
    Untrusted,
    Halted,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResourceSample {
    observed_at: UnixMillis,
    resident_memory_bytes: u64,
    cpu_usage_percent: f32,
}

impl ResourceSample {
    #[must_use]
    pub const fn new(
        observed_at: UnixMillis,
        resident_memory_bytes: u64,
        cpu_usage_percent: f32,
    ) -> Self {
        Self {
            observed_at,
            resident_memory_bytes,
            cpu_usage_percent,
        }
    }

    #[must_use]
    pub const fn observed_at(self) -> UnixMillis {
        self.observed_at
    }

    #[must_use]
    pub const fn resident_memory_bytes(self) -> u64 {
        self.resident_memory_bytes
    }

    #[must_use]
    pub const fn cpu_usage_percent(self) -> f32 {
        self.cpu_usage_percent
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueSnapshot {
    class: QueueClass,
    capacity: usize,
    depth: usize,
    high_watermark: usize,
}

impl QueueSnapshot {
    #[must_use]
    pub const fn class(&self) -> QueueClass {
        self.class
    }

    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    #[must_use]
    pub const fn depth(&self) -> usize {
        self.depth
    }

    #[must_use]
    pub const fn high_watermark(&self) -> usize {
        self.high_watermark
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HealthSnapshot {
    level: HealthLevel,
    process_alive: bool,
    entries_allowed: bool,
    new_ai_allowed: bool,
    recommended_state: SystemState,
    issues: BTreeSet<HealthIssue>,
    resource_sample: Option<ResourceSample>,
    queues: Vec<QueueSnapshot>,
    supervised_tasks: usize,
}

impl HealthSnapshot {
    #[must_use]
    pub const fn level(&self) -> HealthLevel {
        self.level
    }

    #[must_use]
    pub const fn process_alive(&self) -> bool {
        self.process_alive
    }

    #[must_use]
    pub const fn entries_allowed(&self) -> bool {
        self.entries_allowed
    }

    #[must_use]
    pub const fn new_ai_allowed(&self) -> bool {
        self.new_ai_allowed
    }

    #[must_use]
    pub const fn recommended_state(&self) -> SystemState {
        self.recommended_state
    }

    #[must_use]
    pub const fn issues(&self) -> &BTreeSet<HealthIssue> {
        &self.issues
    }

    #[must_use]
    pub const fn resource_sample(&self) -> Option<ResourceSample> {
        self.resource_sample
    }

    #[must_use]
    pub fn queues(&self) -> &[QueueSnapshot] {
        &self.queues
    }

    #[must_use]
    pub const fn supervised_tasks(&self) -> usize {
        self.supervised_tasks
    }
}

#[derive(Debug)]
struct QueueMetrics {
    class: QueueClass,
    capacity: usize,
    depth: AtomicUsize,
    high_watermark: AtomicUsize,
}

impl QueueMetrics {
    fn new(class: QueueClass, capacity: usize) -> Self {
        Self {
            class,
            capacity,
            depth: AtomicUsize::new(0),
            high_watermark: AtomicUsize::new(0),
        }
    }

    fn record_send(&self) {
        let depth = self.depth.fetch_add(1, Ordering::AcqRel) + 1;
        self.high_watermark.fetch_max(depth, Ordering::AcqRel);
    }

    fn record_receive(&self) {
        let _ = self
            .depth
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |depth| {
                depth.checked_sub(1)
            });
    }

    fn snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            class: self.class,
            capacity: self.capacity,
            depth: self.depth.load(Ordering::Acquire),
            high_watermark: self.high_watermark.load(Ordering::Acquire),
        }
    }
}

#[derive(Debug)]
struct HealthState {
    issues: BTreeSet<HealthIssue>,
    resource_sample: Option<ResourceSample>,
    queues: Vec<Arc<QueueMetrics>>,
    supervised_tasks: usize,
}

#[derive(Clone, Debug)]
pub struct HealthMonitor {
    memory_soft_limit_bytes: u64,
    state: Arc<Mutex<HealthState>>,
}

impl HealthMonitor {
    #[must_use]
    pub fn new(runtime_limits: &RuntimeLimits) -> Self {
        let mut issues = BTreeSet::new();
        issues.insert(HealthIssue::ResourceMetricsUnavailable);
        Self {
            memory_soft_limit_bytes: u64::from(runtime_limits.memory_soft_limit_mb())
                * BYTES_PER_MEBIBYTE,
            state: Arc::new(Mutex::new(HealthState {
                issues,
                resource_sample: None,
                queues: Vec::new(),
                supervised_tasks: 0,
            })),
        }
    }

    pub fn record_resource_sample(&self, sample: ResourceSample) {
        let mut state = self.lock_state();
        state.resource_sample = Some(sample);
        state
            .issues
            .remove(&HealthIssue::ResourceMetricsUnavailable);
        state.issues.remove(&HealthIssue::ResourceMetricsStale);
        if sample.resident_memory_bytes() > self.memory_soft_limit_bytes {
            state.issues.insert(HealthIssue::MemorySoftLimitExceeded);
        } else {
            state.issues.remove(&HealthIssue::MemorySoftLimitExceeded);
        }
    }

    #[must_use]
    pub fn snapshot(&self, now: UnixMillis, maximum_sample_age: Duration) -> HealthSnapshot {
        let state = self.lock_state();
        let mut issues = state.issues.clone();
        if resource_sample_is_stale(state.resource_sample, now, maximum_sample_age) {
            issues.insert(HealthIssue::ResourceMetricsStale);
        }
        let level = health_level(&issues);
        let permits_new_work = level == HealthLevel::Healthy;
        HealthSnapshot {
            level,
            process_alive: true,
            entries_allowed: permits_new_work,
            new_ai_allowed: permits_new_work,
            recommended_state: if level == HealthLevel::Halted {
                SystemState::Halted
            } else {
                SystemState::Observing
            },
            issues,
            resource_sample: state.resource_sample,
            queues: state
                .queues
                .iter()
                .map(|metrics| metrics.snapshot())
                .collect(),
            supervised_tasks: state.supervised_tasks,
        }
    }

    fn register_queue(&self, metrics: Arc<QueueMetrics>) {
        self.lock_state().queues.push(metrics);
    }

    fn record_issue(&self, issue: HealthIssue) {
        self.lock_state().issues.insert(issue);
    }

    fn set_supervised_tasks(&self, count: usize) {
        self.lock_state().supervised_tasks = count;
    }

    fn lock_state(&self) -> MutexGuard<'_, HealthState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}

fn resource_sample_is_stale(
    sample: Option<ResourceSample>,
    now: UnixMillis,
    maximum_age: Duration,
) -> bool {
    let Some(sample) = sample else {
        return false;
    };
    let Ok(maximum_age_millis) = i64::try_from(maximum_age.as_millis()) else {
        return false;
    };
    let Some(age) = now.get().checked_sub(sample.observed_at().get()) else {
        return true;
    };
    age < 0 || age > maximum_age_millis
}

fn health_level(issues: &BTreeSet<HealthIssue>) -> HealthLevel {
    if issues.iter().any(|issue| {
        matches!(
            issue,
            HealthIssue::CriticalQueueSaturated
                | HealthIssue::QueueClosed(QueueClass::Critical)
                | HealthIssue::CriticalTaskExited(_)
                | HealthIssue::CriticalTaskFailed { .. }
                | HealthIssue::TaskPanicked(_)
                | HealthIssue::ShutdownTimedOut
        )
    }) {
        HealthLevel::Halted
    } else if issues.iter().any(|issue| {
        matches!(
            issue,
            HealthIssue::ResourceMetricsUnavailable
                | HealthIssue::ResourceMetricsStale
                | HealthIssue::QueueClosed(_)
                | HealthIssue::ShuttingDown
        )
    }) {
        HealthLevel::Untrusted
    } else if issues.is_empty() {
        HealthLevel::Healthy
    } else {
        HealthLevel::Degraded
    }
}

pub struct BoundedQueueSender<T> {
    sender: mpsc::Sender<RuntimeEvent<T>>,
    metrics: Arc<QueueMetrics>,
    health: HealthMonitor,
}

impl<T> Clone for BoundedQueueSender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            metrics: Arc::clone(&self.metrics),
            health: self.health.clone(),
        }
    }
}

pub struct BoundedQueueReceiver<T> {
    receiver: mpsc::Receiver<RuntimeEvent<T>>,
    metrics: Arc<QueueMetrics>,
    health: HealthMonitor,
}

#[derive(Debug)]
pub enum QueueSendError<T> {
    Full(RuntimeEvent<T>),
    Closed(RuntimeEvent<T>),
}

impl<T> QueueSendError<T> {
    #[must_use]
    pub const fn event(&self) -> &RuntimeEvent<T> {
        match self {
            Self::Full(event) | Self::Closed(event) => event,
        }
    }

    #[must_use]
    pub fn into_event(self) -> RuntimeEvent<T> {
        match self {
            Self::Full(event) | Self::Closed(event) => event,
        }
    }
}

impl<T> fmt::Display for QueueSendError<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full(_) => formatter.write_str("bounded runtime queue is full"),
            Self::Closed(_) => formatter.write_str("bounded runtime queue is closed"),
        }
    }
}

impl<T: fmt::Debug> std::error::Error for QueueSendError<T> {}

impl<T> BoundedQueueSender<T> {
    #[must_use]
    pub fn market(limits: &QueueLimits, health: &HealthMonitor) -> (Self, BoundedQueueReceiver<T>) {
        Self::new(
            QueueClass::Market,
            usize::from(limits.market_event_capacity_per_instrument()),
            health,
        )
    }

    #[must_use]
    pub fn critical(
        limits: &QueueLimits,
        health: &HealthMonitor,
    ) -> (Self, BoundedQueueReceiver<T>) {
        Self::new(
            QueueClass::Critical,
            usize::from(limits.critical_event_capacity()),
            health,
        )
    }

    fn new(
        class: QueueClass,
        capacity: usize,
        health: &HealthMonitor,
    ) -> (Self, BoundedQueueReceiver<T>) {
        let (sender, receiver) = mpsc::channel(capacity);
        let metrics = Arc::new(QueueMetrics::new(class, capacity));
        health.register_queue(Arc::clone(&metrics));
        (
            Self {
                sender,
                metrics: Arc::clone(&metrics),
                health: health.clone(),
            },
            BoundedQueueReceiver {
                receiver,
                metrics,
                health: health.clone(),
            },
        )
    }

    pub fn try_send(&self, event: RuntimeEvent<T>) -> Result<(), QueueSendError<T>> {
        match self.sender.try_reserve() {
            Ok(permit) => {
                self.metrics.record_send();
                permit.send(event);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(())) => {
                self.health.record_issue(match self.metrics.class {
                    QueueClass::Market => HealthIssue::MarketQueueSaturated,
                    QueueClass::Critical => HealthIssue::CriticalQueueSaturated,
                });
                Err(QueueSendError::Full(event))
            }
            Err(mpsc::error::TrySendError::Closed(())) => {
                self.health
                    .record_issue(HealthIssue::QueueClosed(self.metrics.class));
                Err(QueueSendError::Closed(event))
            }
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> QueueSnapshot {
        self.metrics.snapshot()
    }
}

impl<T> BoundedQueueReceiver<T> {
    pub async fn recv(&mut self) -> Option<RuntimeEvent<T>> {
        let event = self.receiver.recv().await;
        if event.is_some() {
            self.metrics.record_receive();
        } else {
            self.health
                .record_issue(HealthIssue::QueueClosed(self.metrics.class));
        }
        event
    }

    #[must_use]
    pub fn snapshot(&self) -> QueueSnapshot {
        self.metrics.snapshot()
    }
}

impl<T> Drop for BoundedQueueReceiver<T> {
    fn drop(&mut self) {
        self.receiver.close();
        self.metrics.depth.store(0, Ordering::Release);
        self.health
            .record_issue(HealthIssue::QueueClosed(self.metrics.class));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskImportance {
    Supporting,
    Critical,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskFailure {
    message: Box<str>,
}

impl TaskFailure {
    #[must_use]
    pub fn new(message: impl Into<Box<str>>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for TaskFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TaskFailure {}

#[derive(Clone, Debug)]
pub struct ShutdownSignal {
    receiver: watch::Receiver<bool>,
}

impl ShutdownSignal {
    pub async fn cancelled(&mut self) {
        while !*self.receiver.borrow_and_update() {
            if self.receiver.changed().await.is_err() {
                return;
            }
        }
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpawnError {
    ShuttingDown,
    TaskLimitReached { limit: usize },
}

impl fmt::Display for SpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShuttingDown => formatter.write_str("runtime supervisor is shutting down"),
            Self::TaskLimitReached { limit } => {
                write!(formatter, "runtime supervisor task limit {limit} reached")
            }
        }
    }
}

impl std::error::Error for SpawnError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShutdownReport {
    graceful: bool,
    completed: usize,
    failed: usize,
    forced: usize,
}

impl ShutdownReport {
    #[must_use]
    pub const fn graceful(self) -> bool {
        self.graceful
    }

    #[must_use]
    pub const fn completed(self) -> usize {
        self.completed
    }

    #[must_use]
    pub const fn failed(self) -> usize {
        self.failed
    }

    #[must_use]
    pub const fn forced(self) -> usize {
        self.forced
    }
}

#[derive(Debug)]
struct TaskMetadata {
    name: Box<str>,
    importance: TaskImportance,
}

pub struct RuntimeSupervisor {
    task_limit: NonZeroUsize,
    tasks: JoinSet<Result<(), TaskFailure>>,
    task_metadata: HashMap<tokio::task::Id, TaskMetadata>,
    shutdown_sender: watch::Sender<bool>,
    health: HealthMonitor,
    shutting_down: bool,
}

impl RuntimeSupervisor {
    #[must_use]
    pub fn new(task_limit: NonZeroUsize, health: HealthMonitor) -> Self {
        let (shutdown_sender, _) = watch::channel(false);
        Self {
            task_limit,
            tasks: JoinSet::new(),
            task_metadata: HashMap::new(),
            shutdown_sender,
            health,
            shutting_down: false,
        }
    }

    pub fn spawn<F>(
        &mut self,
        name: impl Into<Box<str>>,
        importance: TaskImportance,
        task: F,
    ) -> Result<(), SpawnError>
    where
        F: Future<Output = Result<(), TaskFailure>> + Send + 'static,
    {
        if self.shutting_down {
            return Err(SpawnError::ShuttingDown);
        }
        if self.tasks.len() >= self.task_limit.get() {
            self.health.record_issue(HealthIssue::TaskCapacitySaturated);
            return Err(SpawnError::TaskLimitReached {
                limit: self.task_limit.get(),
            });
        }
        let name = name.into();
        let abort_handle = self.tasks.spawn(task);
        self.task_metadata
            .insert(abort_handle.id(), TaskMetadata { name, importance });
        self.health.set_supervised_tasks(self.tasks.len());
        Ok(())
    }

    #[must_use]
    pub fn shutdown_signal(&self) -> ShutdownSignal {
        ShutdownSignal {
            receiver: self.shutdown_sender.subscribe(),
        }
    }

    pub fn reap_finished(&mut self) -> usize {
        let mut reaped = 0;
        while let Some(result) = self.tasks.try_join_next_with_id() {
            self.record_task_result(result);
            reaped += 1;
        }
        self.health.set_supervised_tasks(self.tasks.len());
        reaped
    }

    pub async fn shutdown(&mut self, timeout: Duration) -> ShutdownReport {
        self.shutting_down = true;
        self.health.record_issue(HealthIssue::ShuttingDown);
        let _ = self.shutdown_sender.send(true);
        let deadline = Instant::now() + timeout;
        let mut completed = 0;
        let mut failed = 0;

        while !self.tasks.is_empty() {
            match timeout_at(deadline, self.tasks.join_next_with_id()).await {
                Ok(Some(result)) => {
                    let succeeded = result
                        .as_ref()
                        .is_ok_and(|(_, task_result)| task_result.is_ok());
                    self.record_task_result(result);
                    if succeeded {
                        completed += 1;
                    } else {
                        failed += 1;
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        let forced = self.tasks.len();
        if forced > 0 {
            self.health.record_issue(HealthIssue::ShutdownTimedOut);
            self.tasks.abort_all();
            while self.tasks.join_next().await.is_some() {}
            self.task_metadata.clear();
        }
        self.health.set_supervised_tasks(0);

        ShutdownReport {
            graceful: forced == 0,
            completed,
            failed,
            forced,
        }
    }

    fn record_task_result(
        &mut self,
        result: Result<(tokio::task::Id, Result<(), TaskFailure>), JoinError>,
    ) {
        match result {
            Ok((task_id, result)) => {
                let metadata = self
                    .task_metadata
                    .remove(&task_id)
                    .unwrap_or_else(unknown_task_metadata);
                match result {
                    Ok(()) if !self.shutting_down => {
                        self.health.record_issue(
                            if metadata.importance == TaskImportance::Critical {
                                HealthIssue::CriticalTaskExited(metadata.name)
                            } else {
                                HealthIssue::TaskExited(metadata.name)
                            },
                        );
                    }
                    Ok(()) => {}
                    Err(error) if metadata.importance == TaskImportance::Critical => {
                        self.health.record_issue(HealthIssue::CriticalTaskFailed {
                            task: metadata.name,
                            message: error.message,
                        });
                    }
                    Err(error) => {
                        self.health.record_issue(HealthIssue::SupportingTaskFailed {
                            task: metadata.name,
                            message: error.message,
                        });
                    }
                }
            }
            Err(error) if error.is_cancelled() && self.shutting_down => {}
            Err(error) => {
                let metadata = self
                    .task_metadata
                    .remove(&error.id())
                    .unwrap_or_else(unknown_task_metadata);
                self.health
                    .record_issue(HealthIssue::TaskPanicked(metadata.name));
            }
        }
    }
}

fn unknown_task_metadata() -> TaskMetadata {
    TaskMetadata {
        name: "<unknown>".into(),
        importance: TaskImportance::Critical,
    }
}
