use core::fmt;
use core::str::FromStr;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseStableIdError {
    InvalidUuid,
    NilUuid,
}

impl fmt::Display for ParseStableIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUuid => formatter.write_str("stable ID must be a UUID"),
            Self::NilUuid => formatter.write_str("stable ID must not be the nil UUID"),
        }
    }
}

impl std::error::Error for ParseStableIdError {}

macro_rules! stable_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new(value: Uuid) -> Result<Self, ParseStableIdError> {
                if value.is_nil() {
                    return Err(ParseStableIdError::NilUuid);
                }

                Ok(Self(value))
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.hyphenated().fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = ParseStableIdError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                let uuid = Uuid::parse_str(value).map_err(|_| ParseStableIdError::InvalidUuid)?;
                Self::new(uuid)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::from_str(&value).map_err(D::Error::custom)
            }
        }
    };
}

stable_id!(SnapshotId);
stable_id!(EligibilityEventId);
stable_id!(DecisionId);
stable_id!(AiDecisionContextId);
stable_id!(AiTradingPlanId);
stable_id!(TradePlanId);
stable_id!(TradePlanActionId);
stable_id!(OrderIntentId);
stable_id!(OrderId);
stable_id!(FillId);
stable_id!(ManagedLotId);
stable_id!(ReconciliationRunId);
stable_id!(AuditEntryId);
stable_id!(OutboxMessageId);
stable_id!(RuntimeInstanceId);
stable_id!(CorrelationId);
