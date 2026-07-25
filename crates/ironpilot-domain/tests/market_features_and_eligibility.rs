use core::str::FromStr;

use ironpilot_domain::{
    ATR_PERIOD, CandlePattern, ClosedCandle, EligibilityEventEngine, EligibilityEventKind,
    EligibilityPolicy, EmaAlignment, FEATURE_CANDLE_WINDOW, KeyLocation, LlmBudgetUsage,
    MARKET_FEATURES_VERSION_V1, MarketDataSource, MarketFeatureEngine, MarketFeatureError,
    MarketTimeframe, PatternSemantic, PrefilterContext, PrefilterDecision,
    PrefilterRejectionReason, TopOfBook,
};
use ironpilot_domain::{DomainDecimal, InstrumentId};

const END_AT: u64 = 1_800_000_000_000;

fn decimal(value: &str) -> DomainDecimal {
    DomainDecimal::from_str(value).expect("test decimal must be valid")
}

fn instrument() -> InstrumentId {
    InstrumentId::from_str("bybit:spot:BTCUSDT").expect("test instrument must be valid")
}

fn linear_candles(timeframe: MarketTimeframe, end_at: u64, count: usize) -> Vec<ClosedCandle> {
    let duration = timeframe.duration_millis();
    let count_u64 = u64::try_from(count).expect("test count must fit");
    let first_open = end_at - duration * count_u64;
    (0..count)
        .map(|index| {
            let price = 100 + i64::try_from(index).expect("test index must fit");
            let turnover = price * 1_000;
            ClosedCandle::new(
                instrument(),
                timeframe,
                first_open + duration * u64::try_from(index).expect("test index must fit"),
                decimal(&price.to_string()),
                decimal(&(price + 1).to_string()),
                decimal(&(price - 1).to_string()),
                decimal(&price.to_string()),
                decimal(&price.to_string()),
                decimal(&turnover.to_string()),
                true,
            )
            .expect("test candle must be valid")
        })
        .collect()
}

fn top_of_book(as_of: u64) -> TopOfBook {
    TopOfBook::new(
        instrument(),
        as_of - 1_000,
        as_of - 500,
        decimal("218.9"),
        decimal("10"),
        decimal("219.1"),
        decimal("12"),
    )
    .expect("test book must be valid")
}

fn snapshot(source: MarketDataSource) -> ironpilot_domain::MarketFeatureSnapshot {
    let as_of = END_AT + 1_000;
    MarketFeatureEngine::compute(
        &linear_candles(
            MarketTimeframe::FifteenMinutes,
            END_AT,
            FEATURE_CANDLE_WINDOW,
        ),
        &linear_candles(MarketTimeframe::OneHour, END_AT, FEATURE_CANDLE_WINDOW),
        &top_of_book(as_of),
        as_of,
        source,
    )
    .expect("test snapshot must be valid")
}

fn available_budget() -> LlmBudgetUsage {
    LlmBudgetUsage::new(0, 0, DomainDecimal::ZERO, 0)
}

fn eligible_context() -> PrefilterContext {
    PrefilterContext::new(END_AT + 1_000, true, true, false, false, available_budget())
}

#[test]
fn frozen_feature_vector_is_exact_and_complete() {
    let result = snapshot(MarketDataSource::Replay);

    assert_eq!(result.feature_version(), MARKET_FEATURES_VERSION_V1);
    assert_eq!(ATR_PERIOD, 20);
    assert_eq!(result.primary().donchian_upper(), decimal("219"));
    assert_eq!(result.primary().donchian_lower(), decimal("208"));
    assert_eq!(result.primary().atr(), decimal("2"));
    assert_eq!(result.primary().ema_fast(), decimal("209.5"));
    assert_eq!(result.primary().ema_slow(), decimal("194.5"));
    assert_eq!(result.primary().volume_ratio(), decimal("1.05035971"));
    assert_eq!(result.primary().rsi(), decimal("100"));
    assert_eq!(result.primary().adx(), decimal("100"));
    assert_eq!(
        result.primary().ema_alignment(),
        EmaAlignment::StrongBullish
    );
    assert_eq!(result.primary().key_location(), KeyLocation::Resistance);
    let pattern = result
        .primary()
        .pattern()
        .expect("key-location doji must be observed");
    assert_eq!(pattern.pattern(), CandlePattern::Doji);
    assert_eq!(pattern.semantic(), PatternSemantic::Indecision);
    assert!(result.spread_bps() > DomainDecimal::ZERO);
}

#[test]
fn canonical_snapshot_and_event_hashes_ignore_transport_and_restart_state() {
    let rest = snapshot(MarketDataSource::RestBootstrap);
    let websocket = snapshot(MarketDataSource::WebSocketLive);
    assert_ne!(rest.source(), websocket.source());
    assert_eq!(rest.input_hash(), websocket.input_hash());
    assert_eq!(rest.snapshot_hash(), websocket.snapshot_hash());

    let policy = EligibilityPolicy::vertical_slice_defaults().expect("policy must be valid");
    let first = EligibilityEventEngine::new(policy)
        .evaluate(&rest, None, eligible_context())
        .expect("prefilter must evaluate");
    let second = EligibilityEventEngine::new(policy)
        .evaluate(&websocket, None, eligible_context())
        .expect("prefilter must evaluate");

    assert_eq!(
        first.event().expect("first event").event_hash(),
        second.event().expect("second event").event_hash()
    );
    assert_eq!(
        first.event().expect("first event").kinds(),
        &[
            EligibilityEventKind::StructureChanged,
            EligibilityEventKind::KeyLocationReached,
        ]
    );
}

#[test]
fn candle_quality_future_stale_gap_duplicate_and_alignment_fail_closed() {
    let as_of = END_AT + 1_000;
    let valid_primary = linear_candles(
        MarketTimeframe::FifteenMinutes,
        END_AT,
        FEATURE_CANDLE_WINDOW,
    );
    let valid_confirmation =
        linear_candles(MarketTimeframe::OneHour, END_AT, FEATURE_CANDLE_WINDOW);
    let book = top_of_book(as_of);

    let future = MarketFeatureEngine::compute(
        &valid_primary,
        &valid_confirmation,
        &book,
        END_AT - 1,
        MarketDataSource::Replay,
    );
    assert_eq!(future, Err(MarketFeatureError::FutureCandle));

    let stale = MarketFeatureEngine::compute(
        &valid_primary,
        &valid_confirmation,
        &TopOfBook::new(
            instrument(),
            END_AT + 2 * MarketTimeframe::FifteenMinutes.duration_millis() - 1_000,
            END_AT + 2 * MarketTimeframe::FifteenMinutes.duration_millis() - 500,
            decimal("218.9"),
            decimal("10"),
            decimal("219.1"),
            decimal("12"),
        )
        .expect("stale test book must be structurally valid"),
        END_AT + 2 * MarketTimeframe::FifteenMinutes.duration_millis(),
        MarketDataSource::Replay,
    );
    assert_eq!(stale, Err(MarketFeatureError::StaleCandle));

    let mut with_gap = linear_candles(
        MarketTimeframe::FifteenMinutes,
        END_AT,
        FEATURE_CANDLE_WINDOW + 1,
    );
    with_gap.remove(30);
    assert_eq!(
        MarketFeatureEngine::compute(
            &with_gap,
            &valid_confirmation,
            &book,
            as_of,
            MarketDataSource::Replay,
        ),
        Err(MarketFeatureError::CandleGap)
    );

    let mut duplicate = valid_primary.clone();
    duplicate[50] = duplicate[49].clone();
    assert_eq!(
        MarketFeatureEngine::compute(
            &duplicate,
            &valid_confirmation,
            &book,
            as_of,
            MarketDataSource::Replay,
        ),
        Err(MarketFeatureError::DuplicateCandle)
    );

    let misaligned_confirmation = linear_candles(
        MarketTimeframe::OneHour,
        END_AT - MarketTimeframe::OneHour.duration_millis(),
        FEATURE_CANDLE_WINDOW,
    );
    assert_eq!(
        MarketFeatureEngine::compute(
            &valid_primary,
            &misaligned_confirmation,
            &book,
            as_of,
            MarketDataSource::Replay,
        ),
        Err(MarketFeatureError::TimeframesMisaligned)
    );
}

#[test]
fn unclosed_and_invalid_candles_never_enter_the_feature_window() {
    let result = ClosedCandle::new(
        instrument(),
        MarketTimeframe::FifteenMinutes,
        END_AT,
        decimal("100"),
        decimal("101"),
        decimal("99"),
        decimal("100"),
        decimal("10"),
        decimal("1000"),
        false,
    );
    assert_eq!(result, Err(MarketFeatureError::CandleNotClosed));

    let unsupported = ClosedCandle::new(
        InstrumentId::from_str("bybit:linear_perpetual:BTCUSDT")
            .expect("test perpetual instrument must be valid"),
        MarketTimeframe::FifteenMinutes,
        END_AT,
        decimal("100"),
        decimal("101"),
        decimal("99"),
        decimal("100"),
        decimal("10"),
        decimal("1000"),
        true,
    );
    assert_eq!(unsupported, Err(MarketFeatureError::UnsupportedInstrument));
}

#[test]
fn duplicate_and_cooldown_rejections_are_explicit() {
    let snapshot = snapshot(MarketDataSource::Replay);
    let policy = EligibilityPolicy::vertical_slice_defaults().expect("policy must be valid");
    let mut engine = EligibilityEventEngine::new(policy);
    let first = engine
        .evaluate(&snapshot, None, eligible_context())
        .expect("prefilter must evaluate");
    assert!(matches!(first, PrefilterDecision::Eligible(_)));

    let second = engine
        .evaluate(&snapshot, None, eligible_context())
        .expect("prefilter must evaluate");
    assert_eq!(
        second.rejection_reasons(),
        &[
            PrefilterRejectionReason::DuplicateEvent,
            PrefilterRejectionReason::CooldownActive,
        ]
    );
    assert_eq!(engine.deduplication_entries(), 1);
    assert_eq!(engine.cooldown_entries(), 1);
}

#[test]
fn event_ttl_and_cooldown_expire_deterministically() {
    let snapshot = snapshot(MarketDataSource::Replay);
    let policy = EligibilityPolicy::new(
        decimal("10000"),
        decimal("50"),
        1,
        1,
        40,
        200_000,
        decimal("2.00"),
        1,
        4_000,
        decimal("0.05"),
        decimal("2"),
        decimal("1.25"),
        decimal("1.5"),
    )
    .expect("short deterministic policy must be valid");
    let mut engine = EligibilityEventEngine::new(policy);
    let first = engine
        .evaluate(&snapshot, None, eligible_context())
        .expect("prefilter must evaluate");
    assert_eq!(
        first
            .event()
            .expect("first event")
            .valid_until_unix_millis(),
        END_AT + 1_001
    );

    let after_expiry =
        PrefilterContext::new(END_AT + 1_002, true, true, false, false, available_budget());
    let second = engine
        .evaluate(&snapshot, None, after_expiry)
        .expect("prefilter must evaluate after expiry");

    assert!(matches!(second, PrefilterDecision::Eligible(_)));
    assert_eq!(engine.deduplication_entries(), 1);
    assert_eq!(engine.cooldown_entries(), 1);
}

#[test]
fn every_state_and_budget_rejection_is_explainable() {
    let snapshot = snapshot(MarketDataSource::Replay);
    let policy = EligibilityPolicy::vertical_slice_defaults().expect("policy must be valid");
    let mut engine = EligibilityEventEngine::new(policy);
    let context = PrefilterContext::new(
        END_AT + 1_000,
        false,
        false,
        true,
        false,
        LlmBudgetUsage::new(40, 200_000, decimal("2.00"), 1),
    );
    let decision = engine
        .evaluate(&snapshot, Some(&snapshot), context)
        .expect("prefilter must evaluate");

    assert_eq!(
        decision.rejection_reasons(),
        &[
            PrefilterRejectionReason::SystemDisallowsNewAi,
            PrefilterRejectionReason::InstrumentDisabled,
            PrefilterRejectionReason::ActiveTradePlanNotDue,
            PrefilterRejectionReason::NoInformationDelta,
            PrefilterRejectionReason::LlmConcurrencyExhausted,
            PrefilterRejectionReason::DailyCallBudgetExhausted,
            PrefilterRejectionReason::DailyTokenBudgetExhausted,
            PrefilterRejectionReason::DailyCostBudgetExhausted,
        ]
    );
}

#[test]
fn unchanged_features_have_no_information_delta() {
    let current = snapshot(MarketDataSource::WebSocketLive);
    let previous = snapshot(MarketDataSource::RestBootstrap);
    let policy = EligibilityPolicy::vertical_slice_defaults().expect("policy must be valid");
    let decision = EligibilityEventEngine::new(policy)
        .evaluate(&current, Some(&previous), eligible_context())
        .expect("prefilter must evaluate");

    assert_eq!(
        decision.rejection_reasons(),
        &[PrefilterRejectionReason::NoInformationDelta]
    );
}
