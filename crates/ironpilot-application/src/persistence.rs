use core::fmt;

use ironpilot_domain::{AuditEntryId, OutboxMessageId, SystemState};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UnixMillis(i64);

impl UnixMillis {
    pub fn new(value: i64) -> Result<Self, ValidationError> {
        if value < 0 {
            return Err(ValidationError::NegativeTimestamp);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEntry {
    id: AuditEntryId,
    occurred_at: UnixMillis,
    category: Box<str>,
    subject_id: Option<Box<str>>,
    payload: Value,
}

impl AuditEntry {
    pub fn new(
        id: AuditEntryId,
        occurred_at: UnixMillis,
        category: impl Into<Box<str>>,
        subject_id: Option<impl Into<Box<str>>>,
        payload: Value,
    ) -> Result<Self, ValidationError> {
        let category = category.into();
        validate_label("audit category", &category)?;
        let subject_id = subject_id.map(Into::into);
        if let Some(subject_id) = &subject_id {
            validate_label("audit subject ID", subject_id)?;
        }
        ensure_structured_payload(&payload)?;
        Ok(Self {
            id,
            occurred_at,
            category,
            subject_id,
            payload,
        })
    }

    #[must_use]
    pub const fn id(&self) -> AuditEntryId {
        self.id
    }

    #[must_use]
    pub const fn occurred_at(&self) -> UnixMillis {
        self.occurred_at
    }

    #[must_use]
    pub fn category(&self) -> &str {
        &self.category
    }

    #[must_use]
    pub fn subject_id(&self) -> Option<&str> {
        self.subject_id.as_deref()
    }

    #[must_use]
    pub const fn payload(&self) -> &Value {
        &self.payload
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboxMessage {
    id: OutboxMessageId,
    created_at: UnixMillis,
    topic: Box<str>,
    payload: Value,
}

impl OutboxMessage {
    pub fn new(
        id: OutboxMessageId,
        created_at: UnixMillis,
        topic: impl Into<Box<str>>,
        payload: Value,
    ) -> Result<Self, ValidationError> {
        let topic = topic.into();
        validate_label("outbox topic", &topic)?;
        ensure_structured_payload(&payload)?;
        Ok(Self {
            id,
            created_at,
            topic,
            payload,
        })
    }

    #[must_use]
    pub const fn id(&self) -> OutboxMessageId {
        self.id
    }

    #[must_use]
    pub const fn created_at(&self) -> UnixMillis {
        self.created_at
    }

    #[must_use]
    pub fn topic(&self) -> &str {
        &self.topic
    }

    #[must_use]
    pub const fn payload(&self) -> &Value {
        &self.payload
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemStateChange {
    expected: Option<SystemState>,
    next: SystemState,
    changed_at: UnixMillis,
    audit: AuditEntry,
    outbox: Option<OutboxMessage>,
}

impl SystemStateChange {
    pub fn new(
        expected: Option<SystemState>,
        next: SystemState,
        changed_at: UnixMillis,
        audit: AuditEntry,
        outbox: Option<OutboxMessage>,
    ) -> Result<Self, ValidationError> {
        if audit.occurred_at() != changed_at {
            return Err(ValidationError::TimestampMismatch);
        }
        if outbox
            .as_ref()
            .is_some_and(|message| message.created_at() != changed_at)
        {
            return Err(ValidationError::TimestampMismatch);
        }
        if let Some(current) = expected
            && !current.can_transition_to(next)
        {
            return Err(ValidationError::IllegalSystemStateTransition {
                from: current,
                to: next,
            });
        }
        if expected.is_none() && !matches!(next, SystemState::Starting | SystemState::Halted) {
            return Err(ValidationError::IllegalInitialSystemState { state: next });
        }
        Ok(Self {
            expected,
            next,
            changed_at,
            audit,
            outbox,
        })
    }

    #[must_use]
    pub const fn expected(&self) -> Option<SystemState> {
        self.expected
    }

    #[must_use]
    pub const fn next(&self) -> SystemState {
        self.next
    }

    #[must_use]
    pub const fn changed_at(&self) -> UnixMillis {
        self.changed_at
    }

    #[must_use]
    pub const fn audit(&self) -> &AuditEntry {
        &self.audit
    }

    #[must_use]
    pub const fn outbox(&self) -> Option<&OutboxMessage> {
        self.outbox.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PersistedSystemState {
    state: SystemState,
    updated_at: UnixMillis,
}

impl PersistedSystemState {
    #[must_use]
    pub const fn new(state: SystemState, updated_at: UnixMillis) -> Self {
        Self { state, updated_at }
    }

    #[must_use]
    pub const fn state(self) -> SystemState {
        self.state
    }

    #[must_use]
    pub const fn updated_at(self) -> UnixMillis {
        self.updated_at
    }
}

fn validate_label(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(ValidationError::InvalidLabel { field });
    }
    Ok(())
}

fn ensure_structured_payload(payload: &Value) -> Result<(), ValidationError> {
    if !matches!(payload, Value::Object(_)) {
        return Err(ValidationError::PayloadMustBeObject);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationError {
    NegativeTimestamp,
    InvalidLabel { field: &'static str },
    PayloadMustBeObject,
    TimestampMismatch,
    IllegalInitialSystemState { state: SystemState },
    IllegalSystemStateTransition { from: SystemState, to: SystemState },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NegativeTimestamp => formatter.write_str("timestamp must not be negative"),
            Self::InvalidLabel { field } => write!(formatter, "{field} is invalid"),
            Self::PayloadMustBeObject => formatter.write_str("payload must be a JSON object"),
            Self::TimestampMismatch => {
                formatter.write_str("atomic write records must use the same timestamp")
            }
            Self::IllegalInitialSystemState { state } => {
                write!(formatter, "{state:?} is not a legal initial system state")
            }
            Self::IllegalSystemStateTransition { from, to } => {
                write!(
                    formatter,
                    "illegal system state transition from {from:?} to {to:?}"
                )
            }
        }
    }
}

impl std::error::Error for ValidationError {}
