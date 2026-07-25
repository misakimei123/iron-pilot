use core::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidTransition<S> {
    pub from: S,
    pub to: S,
}

impl<S: fmt::Debug> fmt::Display for InvalidTransition<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "illegal state transition from {:?} to {:?}",
            self.from, self.to
        )
    }
}

impl<S: fmt::Debug> std::error::Error for InvalidTransition<S> {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SystemState {
    Starting,
    Recovering,
    Observing,
    EntryEnabled,
    ReduceOnly,
    Halted,
}

impl SystemState {
    pub const ALL: [Self; 6] = [
        Self::Starting,
        Self::Recovering,
        Self::Observing,
        Self::EntryEnabled,
        Self::ReduceOnly,
        Self::Halted,
    ];

    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Starting, Self::Recovering | Self::Halted)
                | (
                    Self::Recovering,
                    Self::Observing | Self::ReduceOnly | Self::Halted
                )
                | (
                    Self::Observing,
                    Self::EntryEnabled | Self::ReduceOnly | Self::Halted | Self::Recovering
                )
                | (
                    Self::EntryEnabled,
                    Self::Observing | Self::ReduceOnly | Self::Halted | Self::Recovering
                )
                | (
                    Self::ReduceOnly,
                    Self::Observing | Self::Halted | Self::Recovering
                )
                | (Self::Halted, Self::Recovering)
        )
    }

    pub fn transition_to(&mut self, next: Self) -> Result<(), InvalidTransition<Self>> {
        transition(self, next, Self::can_transition_to)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TradePlanState {
    Proposed,
    Accepted,
    EntryPending,
    Active,
    ExitPending,
    RecoveryRequired,
    Rejected,
    Cancelled,
    Closed,
}

impl TradePlanState {
    pub const ALL: [Self; 9] = [
        Self::Proposed,
        Self::Accepted,
        Self::EntryPending,
        Self::Active,
        Self::ExitPending,
        Self::RecoveryRequired,
        Self::Rejected,
        Self::Cancelled,
        Self::Closed,
    ];

    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Proposed,
                Self::Accepted | Self::Rejected | Self::Cancelled
            ) | (
                Self::Accepted,
                Self::EntryPending | Self::Active | Self::Closed | Self::Cancelled
            ) | (
                Self::EntryPending,
                Self::Active | Self::Cancelled | Self::RecoveryRequired
            ) | (Self::Active, Self::ExitPending | Self::RecoveryRequired)
                | (Self::ExitPending, Self::Closed | Self::RecoveryRequired)
                | (
                    Self::RecoveryRequired,
                    Self::Active | Self::ExitPending | Self::Closed | Self::Cancelled
                )
        )
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "PROPOSED",
            Self::Accepted => "ACCEPTED",
            Self::EntryPending => "ENTRY_PENDING",
            Self::Active => "ACTIVE",
            Self::ExitPending => "EXIT_PENDING",
            Self::RecoveryRequired => "RECOVERY_REQUIRED",
            Self::Rejected => "REJECTED",
            Self::Cancelled => "CANCELLED",
            Self::Closed => "CLOSED",
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Rejected | Self::Cancelled | Self::Closed)
    }

    pub fn transition_to(&mut self, next: Self) -> Result<(), InvalidTransition<Self>> {
        transition(self, next, Self::can_transition_to)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderState {
    PendingSubmit,
    Submitted,
    PartiallyFilled,
    RecoveryRequired,
    Filled,
    Cancelled,
    Rejected,
    Expired,
}

impl OrderState {
    pub const ALL: [Self; 8] = [
        Self::PendingSubmit,
        Self::Submitted,
        Self::PartiallyFilled,
        Self::RecoveryRequired,
        Self::Filled,
        Self::Cancelled,
        Self::Rejected,
        Self::Expired,
    ];

    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::PendingSubmit,
                Self::Submitted | Self::Cancelled | Self::Rejected | Self::RecoveryRequired
            ) | (
                Self::Submitted,
                Self::PartiallyFilled
                    | Self::Filled
                    | Self::Cancelled
                    | Self::Rejected
                    | Self::Expired
                    | Self::RecoveryRequired
            ) | (
                Self::PartiallyFilled,
                Self::Filled | Self::Cancelled | Self::Expired | Self::RecoveryRequired
            ) | (
                Self::RecoveryRequired,
                Self::Submitted
                    | Self::PartiallyFilled
                    | Self::Filled
                    | Self::Cancelled
                    | Self::Rejected
                    | Self::Expired
            )
        )
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Filled | Self::Cancelled | Self::Rejected | Self::Expired
        )
    }

    pub fn transition_to(&mut self, next: Self) -> Result<(), InvalidTransition<Self>> {
        transition(self, next, Self::can_transition_to)
    }
}

fn transition<S>(
    current: &mut S,
    next: S,
    is_allowed: impl FnOnce(S, S) -> bool,
) -> Result<(), InvalidTransition<S>>
where
    S: Copy,
{
    let from = *current;
    if !is_allowed(from, next) {
        return Err(InvalidTransition { from, to: next });
    }

    *current = next;
    Ok(())
}
