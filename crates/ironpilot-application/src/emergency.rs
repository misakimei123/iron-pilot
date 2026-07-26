use core::fmt;

use ironpilot_domain::EmergencyActionId;
use serde_json::json;
use sha2::{Digest, Sha256};

pub const EMERGENCY_COMMAND_SCHEMA_VERSION_V1: &str = "ironpilot-emergency-command-v1";
pub const MAX_EMERGENCY_COMMAND_TTL_MILLIS: u64 = 300_000;
pub const MAX_EMERGENCY_AUTHORIZATION_SUBJECT_LENGTH: usize = 128;
pub const MAX_EMERGENCY_OBSERVATIONS: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmergencyCommandKind {
    CloseAllManagedExposure,
}

impl EmergencyCommandKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CloseAllManagedExposure => "CLOSE_ALL_MANAGED_EXPOSURE",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmergencyCommandHash([u8; 32]);

impl fmt::Display for EmergencyCommandHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedEmergencyCommand {
    action_id: EmergencyActionId,
    kind: EmergencyCommandKind,
    authorization_subject: Box<str>,
    authorization_evidence_hash: [u8; 32],
    confirmation_nonce_hash: [u8; 32],
    issued_at_unix_millis: u64,
    expires_at_unix_millis: u64,
    payload_json: Box<str>,
    command_hash: EmergencyCommandHash,
}

impl AuthorizedEmergencyCommand {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        action_id: EmergencyActionId,
        kind: EmergencyCommandKind,
        authorization_subject: impl Into<Box<str>>,
        authorization_evidence_hash: [u8; 32],
        confirmation_nonce_hash: [u8; 32],
        issued_at_unix_millis: u64,
        expires_at_unix_millis: u64,
    ) -> Result<Self, EmergencyCommandError> {
        let authorization_subject = authorization_subject.into();
        if authorization_subject.is_empty()
            || authorization_subject.len() > MAX_EMERGENCY_AUTHORIZATION_SUBJECT_LENGTH
            || authorization_subject.chars().any(char::is_control)
        {
            return Err(EmergencyCommandError::InvalidAuthorizationSubject);
        }
        if authorization_evidence_hash == [0; 32] || confirmation_nonce_hash == [0; 32] {
            return Err(EmergencyCommandError::MissingAuthorizationEvidence);
        }
        let ttl = expires_at_unix_millis
            .checked_sub(issued_at_unix_millis)
            .ok_or(EmergencyCommandError::InvalidValidityWindow)?;
        if issued_at_unix_millis == 0 || ttl == 0 || ttl > MAX_EMERGENCY_COMMAND_TTL_MILLIS {
            return Err(EmergencyCommandError::InvalidValidityWindow);
        }
        let payload = json!({
            "schema_version": EMERGENCY_COMMAND_SCHEMA_VERSION_V1,
            "action_id": action_id.to_string(),
            "kind": kind.as_str(),
            "authorization_subject": authorization_subject,
            "authorization_evidence_hash": hex(&authorization_evidence_hash),
            "confirmation_nonce_hash": hex(&confirmation_nonce_hash),
            "issued_at_unix_millis": issued_at_unix_millis,
            "expires_at_unix_millis": expires_at_unix_millis
        });
        let payload_json = serde_json::to_string(&payload)
            .expect("validated emergency command must serialize")
            .into_boxed_str();
        let command_hash = EmergencyCommandHash(Sha256::digest(payload_json.as_bytes()).into());
        Ok(Self {
            action_id,
            kind,
            authorization_subject,
            authorization_evidence_hash,
            confirmation_nonce_hash,
            issued_at_unix_millis,
            expires_at_unix_millis,
            payload_json,
            command_hash,
        })
    }

    #[must_use]
    pub const fn action_id(&self) -> EmergencyActionId {
        self.action_id
    }

    #[must_use]
    pub const fn kind(&self) -> EmergencyCommandKind {
        self.kind
    }

    #[must_use]
    pub fn authorization_subject(&self) -> &str {
        &self.authorization_subject
    }

    #[must_use]
    pub const fn authorization_evidence_hash(&self) -> [u8; 32] {
        self.authorization_evidence_hash
    }

    #[must_use]
    pub const fn confirmation_nonce_hash(&self) -> [u8; 32] {
        self.confirmation_nonce_hash
    }

    #[must_use]
    pub const fn issued_at_unix_millis(&self) -> u64 {
        self.issued_at_unix_millis
    }

    #[must_use]
    pub const fn expires_at_unix_millis(&self) -> u64 {
        self.expires_at_unix_millis
    }

    #[must_use]
    pub fn payload_json(&self) -> &str {
        &self.payload_json
    }

    #[must_use]
    pub const fn command_hash(&self) -> EmergencyCommandHash {
        self.command_hash
    }

    #[must_use]
    pub const fn is_valid_at(&self, now_unix_millis: u64) -> bool {
        now_unix_millis >= self.issued_at_unix_millis
            && now_unix_millis < self.expires_at_unix_millis
    }
}

fn hex(bytes: &[u8; 32]) -> String {
    let mut value = String::with_capacity(64);
    for byte in bytes {
        use core::fmt::Write as _;
        write!(value, "{byte:02x}").expect("writing to String cannot fail");
    }
    value
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmergencyCommandError {
    InvalidAuthorizationSubject,
    MissingAuthorizationEvidence,
    InvalidValidityWindow,
}

impl fmt::Display for EmergencyCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidAuthorizationSubject => "emergency authorization subject is invalid",
            Self::MissingAuthorizationEvidence => {
                "emergency authorization and confirmation evidence must be present"
            }
            Self::InvalidValidityWindow => "emergency command validity window is invalid",
        })
    }
}

impl std::error::Error for EmergencyCommandError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmergencyActionState {
    Requested,
    EntryDisabled,
    OrdersCancelled,
    ExposureReducing,
    Completed,
}

impl EmergencyActionState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "REQUESTED",
            Self::EntryDisabled => "ENTRY_DISABLED",
            Self::OrdersCancelled => "ORDERS_CANCELLED",
            Self::ExposureReducing => "EXPOSURE_REDUCING",
            Self::Completed => "COMPLETED",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmergencyEffect {
    Applied,
    Resumed,
    DuplicateNoEffect,
}

#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use super::*;

    fn action_id() -> EmergencyActionId {
        EmergencyActionId::from_str("00000000-0000-0000-0000-000000000001").expect("valid")
    }

    #[test]
    fn command_requires_bounded_ttl_and_two_independent_evidence_hashes() {
        assert_eq!(
            AuthorizedEmergencyCommand::new(
                action_id(),
                EmergencyCommandKind::CloseAllManagedExposure,
                "operator",
                [1; 32],
                [2; 32],
                10,
                10 + MAX_EMERGENCY_COMMAND_TTL_MILLIS + 1,
            ),
            Err(EmergencyCommandError::InvalidValidityWindow)
        );
        assert_eq!(
            AuthorizedEmergencyCommand::new(
                action_id(),
                EmergencyCommandKind::CloseAllManagedExposure,
                "operator",
                [0; 32],
                [2; 32],
                10,
                20,
            ),
            Err(EmergencyCommandError::MissingAuthorizationEvidence)
        );
    }

    #[test]
    fn command_hash_is_canonical_and_validity_is_half_open() {
        let first = AuthorizedEmergencyCommand::new(
            action_id(),
            EmergencyCommandKind::CloseAllManagedExposure,
            "operator",
            [1; 32],
            [2; 32],
            10,
            20,
        )
        .expect("valid");
        let second = first.clone();
        assert_eq!(first.command_hash(), second.command_hash());
        assert!(!first.is_valid_at(9));
        assert!(first.is_valid_at(10));
        assert!(!first.is_valid_at(20));
    }
}
