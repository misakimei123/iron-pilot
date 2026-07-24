use ironpilot_domain::{OrderState, SystemState, TradePlanState};
use proptest::prelude::*;

proptest! {
    #[test]
    fn system_transitions_are_atomic_and_fail_closed(
        from in proptest::sample::select(SystemState::ALL.to_vec()),
        to in proptest::sample::select(SystemState::ALL.to_vec()),
    ) {
        let mut current = from;
        let result = current.transition_to(to);

        if from.can_transition_to(to) {
            prop_assert_eq!(result, Ok(()));
            prop_assert_eq!(current, to);
        } else {
            prop_assert!(result.is_err());
            prop_assert_eq!(current, from);
        }
    }

    #[test]
    fn trade_plan_transitions_are_atomic_and_fail_closed(
        from in proptest::sample::select(TradePlanState::ALL.to_vec()),
        to in proptest::sample::select(TradePlanState::ALL.to_vec()),
    ) {
        let mut current = from;
        let result = current.transition_to(to);

        if from.can_transition_to(to) {
            prop_assert_eq!(result, Ok(()));
            prop_assert_eq!(current, to);
        } else {
            prop_assert!(result.is_err());
            prop_assert_eq!(current, from);
        }
    }

    #[test]
    fn order_transitions_are_atomic_and_fail_closed(
        from in proptest::sample::select(OrderState::ALL.to_vec()),
        to in proptest::sample::select(OrderState::ALL.to_vec()),
    ) {
        let mut current = from;
        let result = current.transition_to(to);

        if from.can_transition_to(to) {
            prop_assert_eq!(result, Ok(()));
            prop_assert_eq!(current, to);
        } else {
            prop_assert!(result.is_err());
            prop_assert_eq!(current, from);
        }
    }
}

#[test]
fn terminal_trade_plan_states_never_transition() {
    for terminal in [
        TradePlanState::Rejected,
        TradePlanState::Cancelled,
        TradePlanState::Closed,
    ] {
        assert!(terminal.is_terminal());
        assert!(
            TradePlanState::ALL
                .into_iter()
                .all(|next| !terminal.can_transition_to(next))
        );
    }
}

#[test]
fn terminal_order_states_never_transition() {
    for terminal in [
        OrderState::Filled,
        OrderState::Cancelled,
        OrderState::Rejected,
        OrderState::Expired,
    ] {
        assert!(terminal.is_terminal());
        assert!(
            OrderState::ALL
                .into_iter()
                .all(|next| !terminal.can_transition_to(next))
        );
    }
}

#[test]
fn unknown_serialized_states_are_rejected() {
    assert!(serde_json::from_str::<SystemState>("\"UNKNOWN\"").is_err());
    assert!(serde_json::from_str::<TradePlanState>("\"UNKNOWN\"").is_err());
    assert!(serde_json::from_str::<OrderState>("\"UNKNOWN\"").is_err());
}
