use core::str::FromStr;

use ironpilot_domain::{
    DecisionId, DomainDecimal, InstrumentId, ParseDomainDecimalError, ParseStableIdError,
};
use proptest::prelude::*;

proptest! {
    #[test]
    fn exact_decimal_string_roundtrips(mantissa in any::<i64>(), scale in 0_u32..=28) {
        let value = DomainDecimal::from_mantissa_scale(i128::from(mantissa), scale)
            .expect("i64 mantissa and valid scale must fit");
        let json = serde_json::to_string(&value).expect("decimal must serialize");
        let roundtrip: DomainDecimal =
            serde_json::from_str(&json).expect("serialized decimal must deserialize");

        prop_assert_eq!(roundtrip, value);
    }
}

#[test]
fn decimal_rejects_json_floating_point_numbers() {
    let result = serde_json::from_str::<DomainDecimal>("12.5");

    assert!(result.is_err());
}

#[test]
fn decimal_rejects_scale_above_twenty_eight() {
    assert_eq!(
        DomainDecimal::from_mantissa_scale(1, 29),
        Err(ParseDomainDecimalError::ScaleOutOfRange { scale: 29 })
    );
}

#[test]
fn decimal_rejects_unrepresentable_magnitude_without_panicking() {
    assert_eq!(
        DomainDecimal::from_mantissa_scale(i128::MAX, 0),
        Err(ParseDomainDecimalError::MagnitudeOutOfRange)
    );
}

#[test]
fn stable_ids_reject_nil_and_roundtrip_as_uuid_strings() {
    let valid = "018f0f3e-7b4d-7cc0-a6c8-7f8519262a1f";
    let id = DecisionId::from_str(valid).expect("non-nil UUID must be accepted");

    assert_eq!(id.to_string(), valid);
    assert_eq!(
        DecisionId::from_str("00000000-0000-0000-0000-000000000000"),
        Err(ParseStableIdError::NilUuid)
    );
}

#[test]
fn instrument_ids_are_canonical_and_closed_to_unknown_kinds() {
    let instrument =
        InstrumentId::from_str("bybit:spot:BTCUSDT").expect("canonical Spot ID must parse");

    assert_eq!(instrument.to_string(), "bybit:spot:BTCUSDT");
    assert!(InstrumentId::from_str("bybit:spot:btcusdt").is_err());
    assert!(InstrumentId::from_str("other:spot:BTCUSDT").is_err());
    assert!(InstrumentId::from_str("bybit:option:BTCUSDT").is_err());
}
