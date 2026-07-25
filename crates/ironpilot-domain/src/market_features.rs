use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::{Decimal, RoundingStrategy};
use sha2::{Digest, Sha256};

use crate::{DomainDecimal, Exchange, InstrumentId, InstrumentType};

pub const MARKET_FEATURES_VERSION_V1: &str = "ironpilot-market-features-v1";
pub const FEATURE_CANDLE_WINDOW: usize = 120;
pub const DONCHIAN_UPPER_PERIOD: usize = 20;
pub const DONCHIAN_LOWER_PERIOD: usize = 10;
pub const ATR_PERIOD: usize = 20;
pub const EMA_FAST_PERIOD: usize = 20;
pub const EMA_SLOW_PERIOD: usize = 50;
pub const WILDER_PERIOD: usize = 14;
pub const VOLUME_RATIO_PERIOD: usize = 20;
pub const MAX_EVENT_DEDUPLICATION_ENTRIES: usize = 1_024;
pub const MAX_TRACKED_ELIGIBILITY_INSTRUMENTS: usize = 3;

const OUTPUT_DECIMAL_PLACES: u32 = 8;
const PRIMARY_STALENESS_INTERVALS: u64 = 2;
const CONFIRMATION_STALENESS_INTERVALS: u64 = 2;
const BOOK_MAX_AGE_MILLIS: u64 = 30_000;
const BPS: i64 = 10_000;
const PERCENT: i64 = 100;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MarketTimeframe {
    FifteenMinutes,
    OneHour,
}

impl MarketTimeframe {
    #[must_use]
    pub const fn duration_millis(self) -> u64 {
        match self {
            Self::FifteenMinutes => 15 * 60 * 1_000,
            Self::OneHour => 60 * 60 * 1_000,
        }
    }

    const fn canonical_name(self) -> &'static str {
        match self {
            Self::FifteenMinutes => "15m",
            Self::OneHour => "1h",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MarketDataSource {
    RestBootstrap,
    WebSocketLive,
    Replay,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClosedCandle {
    instrument_id: InstrumentId,
    timeframe: MarketTimeframe,
    open_at_unix_millis: u64,
    close_at_unix_millis: u64,
    open: DomainDecimal,
    high: DomainDecimal,
    low: DomainDecimal,
    close: DomainDecimal,
    volume: DomainDecimal,
    turnover: DomainDecimal,
}

impl ClosedCandle {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        timeframe: MarketTimeframe,
        open_at_unix_millis: u64,
        open: DomainDecimal,
        high: DomainDecimal,
        low: DomainDecimal,
        close: DomainDecimal,
        volume: DomainDecimal,
        turnover: DomainDecimal,
        confirmed_closed: bool,
    ) -> Result<Self, MarketFeatureError> {
        if instrument_id.exchange() != Exchange::Bybit
            || instrument_id.instrument_type() != InstrumentType::Spot
        {
            return Err(MarketFeatureError::UnsupportedInstrument);
        }
        if !confirmed_closed {
            return Err(MarketFeatureError::CandleNotClosed);
        }
        if !open_at_unix_millis.is_multiple_of(timeframe.duration_millis()) {
            return Err(MarketFeatureError::CandleMisaligned);
        }
        if [open, high, low, close]
            .into_iter()
            .any(|value| value <= DomainDecimal::ZERO)
        {
            return Err(MarketFeatureError::NonPositivePrice);
        }
        if volume < DomainDecimal::ZERO || turnover < DomainDecimal::ZERO {
            return Err(MarketFeatureError::NegativeMarketAmount);
        }
        if high < open || high < low || high < close || low > open || low > high || low > close {
            return Err(MarketFeatureError::InvalidOhlcEnvelope);
        }
        let close_at_unix_millis = open_at_unix_millis
            .checked_add(timeframe.duration_millis())
            .ok_or(MarketFeatureError::TimestampOverflow)?;

        Ok(Self {
            instrument_id,
            timeframe,
            open_at_unix_millis,
            close_at_unix_millis,
            open,
            high,
            low,
            close,
            volume,
            turnover,
        })
    }

    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn timeframe(&self) -> MarketTimeframe {
        self.timeframe
    }

    #[must_use]
    pub const fn open_at_unix_millis(&self) -> u64 {
        self.open_at_unix_millis
    }

    #[must_use]
    pub const fn close_at_unix_millis(&self) -> u64 {
        self.close_at_unix_millis
    }

    #[must_use]
    pub const fn open(&self) -> DomainDecimal {
        self.open
    }

    #[must_use]
    pub const fn high(&self) -> DomainDecimal {
        self.high
    }

    #[must_use]
    pub const fn low(&self) -> DomainDecimal {
        self.low
    }

    #[must_use]
    pub const fn close(&self) -> DomainDecimal {
        self.close
    }

    #[must_use]
    pub const fn volume(&self) -> DomainDecimal {
        self.volume
    }

    #[must_use]
    pub const fn turnover(&self) -> DomainDecimal {
        self.turnover
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TopOfBook {
    instrument_id: InstrumentId,
    source_generated_at_unix_millis: u64,
    observed_at_unix_millis: u64,
    bid_price: DomainDecimal,
    bid_quantity: DomainDecimal,
    ask_price: DomainDecimal,
    ask_quantity: DomainDecimal,
}

impl TopOfBook {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        source_generated_at_unix_millis: u64,
        observed_at_unix_millis: u64,
        bid_price: DomainDecimal,
        bid_quantity: DomainDecimal,
        ask_price: DomainDecimal,
        ask_quantity: DomainDecimal,
    ) -> Result<Self, MarketFeatureError> {
        if instrument_id.exchange() != Exchange::Bybit
            || instrument_id.instrument_type() != InstrumentType::Spot
        {
            return Err(MarketFeatureError::UnsupportedInstrument);
        }
        if [bid_price, bid_quantity, ask_price, ask_quantity]
            .into_iter()
            .any(|value| value <= DomainDecimal::ZERO)
        {
            return Err(MarketFeatureError::NonPositiveBookValue);
        }
        if bid_price >= ask_price {
            return Err(MarketFeatureError::CrossedBook);
        }
        if source_generated_at_unix_millis > observed_at_unix_millis {
            return Err(MarketFeatureError::FutureBook);
        }
        Ok(Self {
            instrument_id,
            source_generated_at_unix_millis,
            observed_at_unix_millis,
            bid_price,
            bid_quantity,
            ask_price,
            ask_quantity,
        })
    }

    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn source_generated_at_unix_millis(&self) -> u64 {
        self.source_generated_at_unix_millis
    }

    #[must_use]
    pub const fn observed_at_unix_millis(&self) -> u64 {
        self.observed_at_unix_millis
    }

    #[must_use]
    pub const fn bid_price(&self) -> DomainDecimal {
        self.bid_price
    }

    #[must_use]
    pub const fn bid_quantity(&self) -> DomainDecimal {
        self.bid_quantity
    }

    #[must_use]
    pub const fn ask_price(&self) -> DomainDecimal {
        self.ask_price
    }

    #[must_use]
    pub const fn ask_quantity(&self) -> DomainDecimal {
        self.ask_quantity
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EmaAlignment {
    StrongBullish,
    Bullish,
    StrongBearish,
    Bearish,
    Mixed,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum KeyLocation {
    None,
    Support,
    Resistance,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CandlePattern {
    BigBullish,
    BigBearish,
    Hammer,
    HangingMan,
    ShootingStar,
    InvertedHammer,
    BullishEngulfing,
    BearishEngulfing,
    BullishHarami,
    BearishHarami,
    Doji,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PatternSemantic {
    BullishAttack,
    BearishAttack,
    BullishSupportRejection,
    BearishExhaustion,
    BearishResistanceRejection,
    BullishSupportTest,
    BullishReversal,
    BearishReversal,
    BearishMomentumExhaustion,
    BullishMomentumExhaustion,
    Indecision,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PatternObservation {
    pattern: CandlePattern,
    semantic: PatternSemantic,
}

impl PatternObservation {
    #[must_use]
    pub const fn pattern(self) -> CandlePattern {
        self.pattern
    }

    #[must_use]
    pub const fn semantic(self) -> PatternSemantic {
        self.semantic
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeframeFeatures {
    timeframe: MarketTimeframe,
    candle_open_at_unix_millis: u64,
    candle_close_at_unix_millis: u64,
    latest_open: DomainDecimal,
    latest_high: DomainDecimal,
    latest_low: DomainDecimal,
    latest_close: DomainDecimal,
    latest_volume: DomainDecimal,
    latest_turnover: DomainDecimal,
    donchian_upper: DomainDecimal,
    donchian_lower: DomainDecimal,
    ema_fast: DomainDecimal,
    ema_slow: DomainDecimal,
    rsi: DomainDecimal,
    atr: DomainDecimal,
    adx: DomainDecimal,
    volume_ratio: DomainDecimal,
    ema_alignment: EmaAlignment,
    key_location: KeyLocation,
    pattern: Option<PatternObservation>,
}

impl TimeframeFeatures {
    #[must_use]
    pub const fn timeframe(&self) -> MarketTimeframe {
        self.timeframe
    }

    #[must_use]
    pub const fn candle_open_at_unix_millis(&self) -> u64 {
        self.candle_open_at_unix_millis
    }

    #[must_use]
    pub const fn candle_close_at_unix_millis(&self) -> u64 {
        self.candle_close_at_unix_millis
    }

    #[must_use]
    pub const fn latest_open(&self) -> DomainDecimal {
        self.latest_open
    }

    #[must_use]
    pub const fn latest_high(&self) -> DomainDecimal {
        self.latest_high
    }

    #[must_use]
    pub const fn latest_low(&self) -> DomainDecimal {
        self.latest_low
    }

    #[must_use]
    pub const fn latest_close(&self) -> DomainDecimal {
        self.latest_close
    }

    #[must_use]
    pub const fn latest_volume(&self) -> DomainDecimal {
        self.latest_volume
    }

    #[must_use]
    pub const fn latest_turnover(&self) -> DomainDecimal {
        self.latest_turnover
    }

    #[must_use]
    pub const fn donchian_upper(&self) -> DomainDecimal {
        self.donchian_upper
    }

    #[must_use]
    pub const fn donchian_lower(&self) -> DomainDecimal {
        self.donchian_lower
    }

    #[must_use]
    pub const fn ema_fast(&self) -> DomainDecimal {
        self.ema_fast
    }

    #[must_use]
    pub const fn ema_slow(&self) -> DomainDecimal {
        self.ema_slow
    }

    #[must_use]
    pub const fn rsi(&self) -> DomainDecimal {
        self.rsi
    }

    #[must_use]
    pub const fn atr(&self) -> DomainDecimal {
        self.atr
    }

    #[must_use]
    pub const fn adx(&self) -> DomainDecimal {
        self.adx
    }

    #[must_use]
    pub const fn volume_ratio(&self) -> DomainDecimal {
        self.volume_ratio
    }

    #[must_use]
    pub const fn ema_alignment(&self) -> EmaAlignment {
        self.ema_alignment
    }

    #[must_use]
    pub const fn key_location(&self) -> KeyLocation {
        self.key_location
    }

    #[must_use]
    pub const fn pattern(&self) -> Option<PatternObservation> {
        self.pattern
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarketFeatureSnapshot {
    feature_version: &'static str,
    instrument_id: InstrumentId,
    generated_at_unix_millis: u64,
    valid_until_unix_millis: u64,
    source: MarketDataSource,
    input_hash: ContentHash,
    primary: TimeframeFeatures,
    confirmation: TimeframeFeatures,
    bid_price: DomainDecimal,
    ask_price: DomainDecimal,
    spread_bps: DomainDecimal,
    snapshot_hash: ContentHash,
}

impl MarketFeatureSnapshot {
    #[must_use]
    pub const fn feature_version(&self) -> &'static str {
        self.feature_version
    }

    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn generated_at_unix_millis(&self) -> u64 {
        self.generated_at_unix_millis
    }

    #[must_use]
    pub const fn valid_until_unix_millis(&self) -> u64 {
        self.valid_until_unix_millis
    }

    #[must_use]
    pub const fn source(&self) -> MarketDataSource {
        self.source
    }

    #[must_use]
    pub const fn input_hash(&self) -> ContentHash {
        self.input_hash
    }

    #[must_use]
    pub const fn primary(&self) -> &TimeframeFeatures {
        &self.primary
    }

    #[must_use]
    pub const fn confirmation(&self) -> &TimeframeFeatures {
        &self.confirmation
    }

    #[must_use]
    pub const fn bid_price(&self) -> DomainDecimal {
        self.bid_price
    }

    #[must_use]
    pub const fn ask_price(&self) -> DomainDecimal {
        self.ask_price
    }

    #[must_use]
    pub const fn spread_bps(&self) -> DomainDecimal {
        self.spread_bps
    }

    #[must_use]
    pub const fn snapshot_hash(&self) -> ContentHash {
        self.snapshot_hash
    }

    #[must_use]
    pub const fn is_expired_at(&self, unix_millis: u64) -> bool {
        unix_millis >= self.valid_until_unix_millis
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MarketFeatureEngine;

impl MarketFeatureEngine {
    pub fn compute(
        primary_candles: &[ClosedCandle],
        confirmation_candles: &[ClosedCandle],
        book: &TopOfBook,
        as_of_unix_millis: u64,
        source: MarketDataSource,
    ) -> Result<MarketFeatureSnapshot, MarketFeatureError> {
        let primary = validate_candle_window(
            primary_candles,
            MarketTimeframe::FifteenMinutes,
            as_of_unix_millis,
            PRIMARY_STALENESS_INTERVALS,
        )?;
        let confirmation = validate_candle_window(
            confirmation_candles,
            MarketTimeframe::OneHour,
            as_of_unix_millis,
            CONFIRMATION_STALENESS_INTERVALS,
        )?;
        let instrument_id = primary[0].instrument_id().clone();
        if confirmation[0].instrument_id() != &instrument_id
            || book.instrument_id() != &instrument_id
        {
            return Err(MarketFeatureError::InstrumentMismatch);
        }
        let latest_primary = primary
            .last()
            .ok_or(MarketFeatureError::InsufficientWarmup)?;
        let latest_confirmation = confirmation
            .last()
            .ok_or(MarketFeatureError::InsufficientWarmup)?;
        let expected_confirmation_close = latest_primary.close_at_unix_millis()
            - (latest_primary.close_at_unix_millis() % MarketTimeframe::OneHour.duration_millis());
        if latest_confirmation.close_at_unix_millis() != expected_confirmation_close {
            return Err(MarketFeatureError::TimeframesMisaligned);
        }
        if book.observed_at_unix_millis() > as_of_unix_millis
            || book.source_generated_at_unix_millis() > as_of_unix_millis
        {
            return Err(MarketFeatureError::FutureBook);
        }
        if book.observed_at_unix_millis() < latest_primary.close_at_unix_millis()
            || book.source_generated_at_unix_millis() < latest_primary.close_at_unix_millis()
        {
            return Err(MarketFeatureError::StaleBook);
        }
        let book_valid_until = book
            .observed_at_unix_millis()
            .checked_add(BOOK_MAX_AGE_MILLIS)
            .ok_or(MarketFeatureError::TimestampOverflow)?;
        if as_of_unix_millis >= book_valid_until {
            return Err(MarketFeatureError::StaleBook);
        }

        let primary_features = compute_timeframe_features(primary)?;
        let confirmation_features = compute_timeframe_features(confirmation)?;
        let spread_bps = compute_spread_bps(book)?;
        let input_hash = hash_market_inputs(&instrument_id, primary, confirmation, book);
        let primary_valid_until = latest_primary
            .close_at_unix_millis()
            .checked_add(
                MarketTimeframe::FifteenMinutes
                    .duration_millis()
                    .checked_mul(PRIMARY_STALENESS_INTERVALS)
                    .ok_or(MarketFeatureError::TimestampOverflow)?,
            )
            .ok_or(MarketFeatureError::TimestampOverflow)?;
        let confirmation_valid_until = latest_confirmation
            .close_at_unix_millis()
            .checked_add(
                MarketTimeframe::OneHour
                    .duration_millis()
                    .checked_mul(CONFIRMATION_STALENESS_INTERVALS)
                    .ok_or(MarketFeatureError::TimestampOverflow)?,
            )
            .ok_or(MarketFeatureError::TimestampOverflow)?;
        let valid_until_unix_millis = primary_valid_until
            .min(confirmation_valid_until)
            .min(book_valid_until);
        let snapshot_hash = hash_snapshot(
            &instrument_id,
            input_hash,
            &primary_features,
            &confirmation_features,
            book.bid_price(),
            book.ask_price(),
            spread_bps,
        );

        Ok(MarketFeatureSnapshot {
            feature_version: MARKET_FEATURES_VERSION_V1,
            instrument_id,
            generated_at_unix_millis: latest_primary.close_at_unix_millis(),
            valid_until_unix_millis,
            source,
            input_hash,
            primary: primary_features,
            confirmation: confirmation_features,
            bid_price: book.bid_price(),
            ask_price: book.ask_price(),
            spread_bps,
            snapshot_hash,
        })
    }
}

fn validate_candle_window(
    candles: &[ClosedCandle],
    expected_timeframe: MarketTimeframe,
    as_of_unix_millis: u64,
    staleness_intervals: u64,
) -> Result<&[ClosedCandle], MarketFeatureError> {
    if candles.len() < FEATURE_CANDLE_WINDOW {
        return Err(MarketFeatureError::InsufficientWarmup);
    }
    let active = &candles[candles.len() - FEATURE_CANDLE_WINDOW..];
    let instrument_id = active[0].instrument_id();
    for candle in active {
        if candle.timeframe() != expected_timeframe {
            return Err(MarketFeatureError::TimeframeMismatch);
        }
        if candle.instrument_id() != instrument_id {
            return Err(MarketFeatureError::InstrumentMismatch);
        }
        if candle.close_at_unix_millis() > as_of_unix_millis {
            return Err(MarketFeatureError::FutureCandle);
        }
    }
    for pair in active.windows(2) {
        if pair[1].open_at_unix_millis() == pair[0].open_at_unix_millis() {
            return Err(MarketFeatureError::DuplicateCandle);
        }
        if pair[1].open_at_unix_millis() <= pair[0].open_at_unix_millis() {
            return Err(MarketFeatureError::OutOfOrderCandle);
        }
        if pair[1].open_at_unix_millis() != pair[0].close_at_unix_millis() {
            return Err(MarketFeatureError::CandleGap);
        }
    }
    let latest = active
        .last()
        .ok_or(MarketFeatureError::InsufficientWarmup)?;
    let maximum_age = expected_timeframe
        .duration_millis()
        .checked_mul(staleness_intervals)
        .ok_or(MarketFeatureError::TimestampOverflow)?;
    let valid_until = latest
        .close_at_unix_millis()
        .checked_add(maximum_age)
        .ok_or(MarketFeatureError::TimestampOverflow)?;
    if as_of_unix_millis >= valid_until {
        return Err(MarketFeatureError::StaleCandle);
    }
    Ok(active)
}

fn compute_timeframe_features(
    candles: &[ClosedCandle],
) -> Result<TimeframeFeatures, MarketFeatureError> {
    let latest = candles
        .last()
        .ok_or(MarketFeatureError::InsufficientWarmup)?;
    let history = &candles[..candles.len() - 1];
    let donchian_upper = history[history.len() - DONCHIAN_UPPER_PERIOD..]
        .iter()
        .map(ClosedCandle::high)
        .max()
        .ok_or(MarketFeatureError::InsufficientWarmup)?;
    let donchian_lower = history[history.len() - DONCHIAN_LOWER_PERIOD..]
        .iter()
        .map(ClosedCandle::low)
        .min()
        .ok_or(MarketFeatureError::InsufficientWarmup)?;
    let closes: Vec<Decimal> = candles
        .iter()
        .map(|candle| candle.close().as_decimal())
        .collect();
    let ema_fast = ema(&closes, EMA_FAST_PERIOD)?;
    let ema_slow = ema(&closes, EMA_SLOW_PERIOD)?;
    let rsi = wilder_rsi(&closes, WILDER_PERIOD)?;
    let atr = wilder_atr(candles, ATR_PERIOD)?;
    let adx = wilder_adx(candles, WILDER_PERIOD)?;
    let previous_volumes: Vec<Decimal> = history[history.len() - VOLUME_RATIO_PERIOD..]
        .iter()
        .map(|candle| candle.volume().as_decimal())
        .collect();
    let average_volume = checked_div(
        checked_sum(&previous_volumes)?,
        decimal_from_usize(VOLUME_RATIO_PERIOD)?,
    )?;
    if average_volume.is_zero() || latest.volume() == DomainDecimal::ZERO {
        return Err(MarketFeatureError::IndicatorUnavailable);
    }
    let volume_ratio = rounded(checked_div(latest.volume().as_decimal(), average_volume)?);
    let ema_fast = rounded(ema_fast);
    let ema_slow = rounded(ema_slow);
    let rsi = rounded(rsi);
    let atr = rounded(atr);
    let adx = rounded(adx);
    let ema_alignment = ema_alignment(latest.close().as_decimal(), ema_fast, ema_slow, atr)?;
    let key_location = key_location(
        latest,
        donchian_upper.as_decimal(),
        donchian_lower.as_decimal(),
        ema_slow,
        atr,
    )?;
    let previous = history
        .last()
        .ok_or(MarketFeatureError::InsufficientWarmup)?;
    let pattern = detect_pattern(previous, latest, key_location, atr)?;

    Ok(TimeframeFeatures {
        timeframe: latest.timeframe(),
        candle_open_at_unix_millis: latest.open_at_unix_millis(),
        candle_close_at_unix_millis: latest.close_at_unix_millis(),
        latest_open: latest.open(),
        latest_high: latest.high(),
        latest_low: latest.low(),
        latest_close: latest.close(),
        latest_volume: latest.volume(),
        latest_turnover: latest.turnover(),
        donchian_upper,
        donchian_lower,
        ema_fast: domain_decimal(ema_fast)?,
        ema_slow: domain_decimal(ema_slow)?,
        rsi: domain_decimal(rsi)?,
        atr: domain_decimal(atr)?,
        adx: domain_decimal(adx)?,
        volume_ratio: domain_decimal(volume_ratio)?,
        ema_alignment,
        key_location,
        pattern,
    })
}

fn ema(values: &[Decimal], period: usize) -> Result<Decimal, MarketFeatureError> {
    if values.len() < period {
        return Err(MarketFeatureError::InsufficientWarmup);
    }
    let period_decimal = decimal_from_usize(period)?;
    let mut current = checked_div(checked_sum(&values[..period])?, period_decimal)?;
    let alpha = checked_div(Decimal::from(2), checked_add(period_decimal, Decimal::ONE)?)?;
    for value in &values[period..] {
        current = checked_add(current, checked_mul(checked_sub(*value, current)?, alpha)?)?;
    }
    Ok(current)
}

fn wilder_rsi(values: &[Decimal], period: usize) -> Result<Decimal, MarketFeatureError> {
    if values.len() <= period {
        return Err(MarketFeatureError::InsufficientWarmup);
    }
    let period_decimal = decimal_from_usize(period)?;
    let mut gains = Vec::with_capacity(values.len() - 1);
    let mut losses = Vec::with_capacity(values.len() - 1);
    for pair in values.windows(2) {
        let change = checked_sub(pair[1], pair[0])?;
        gains.push(change.max(Decimal::ZERO));
        losses.push(checked_abs(change.min(Decimal::ZERO))?);
    }
    let mut average_gain = checked_div(checked_sum(&gains[..period])?, period_decimal)?;
    let mut average_loss = checked_div(checked_sum(&losses[..period])?, period_decimal)?;
    for index in period..gains.len() {
        average_gain = wilder_average(average_gain, gains[index], period_decimal)?;
        average_loss = wilder_average(average_loss, losses[index], period_decimal)?;
    }
    if average_gain.is_zero() && average_loss.is_zero() {
        return Err(MarketFeatureError::IndicatorUnavailable);
    }
    if average_loss.is_zero() {
        return Ok(Decimal::from(PERCENT));
    }
    let relative_strength = checked_div(average_gain, average_loss)?;
    checked_sub(
        Decimal::from(PERCENT),
        checked_div(
            Decimal::from(PERCENT),
            checked_add(Decimal::ONE, relative_strength)?,
        )?,
    )
}

fn wilder_atr(candles: &[ClosedCandle], period: usize) -> Result<Decimal, MarketFeatureError> {
    let true_ranges = atr_true_ranges(candles)?;
    if true_ranges.len() < period {
        return Err(MarketFeatureError::InsufficientWarmup);
    }
    let period_decimal = decimal_from_usize(period)?;
    let mut atr = checked_div(checked_sum(&true_ranges[..period])?, period_decimal)?;
    for current_range in &true_ranges[period..] {
        atr = wilder_average(atr, *current_range, period_decimal)?;
    }
    if atr.is_zero() {
        return Err(MarketFeatureError::IndicatorUnavailable);
    }
    Ok(atr)
}

fn wilder_adx(candles: &[ClosedCandle], period: usize) -> Result<Decimal, MarketFeatureError> {
    if candles.len() < period.saturating_mul(2) {
        return Err(MarketFeatureError::InsufficientWarmup);
    }
    let true_ranges = true_ranges(candles)?;
    let mut plus_dm = Vec::with_capacity(candles.len() - 1);
    let mut minus_dm = Vec::with_capacity(candles.len() - 1);
    for pair in candles.windows(2) {
        let upward = checked_sub(pair[1].high().as_decimal(), pair[0].high().as_decimal())?;
        let downward = checked_sub(pair[0].low().as_decimal(), pair[1].low().as_decimal())?;
        plus_dm.push(if upward > downward && upward > Decimal::ZERO {
            upward
        } else {
            Decimal::ZERO
        });
        minus_dm.push(if downward > upward && downward > Decimal::ZERO {
            downward
        } else {
            Decimal::ZERO
        });
    }
    let period_decimal = decimal_from_usize(period)?;
    let mut smoothed_tr = checked_sum(&true_ranges[..period])?;
    let mut smoothed_plus = checked_sum(&plus_dm[..period])?;
    let mut smoothed_minus = checked_sum(&minus_dm[..period])?;
    let mut dx_values = Vec::with_capacity(true_ranges.len() - period + 1);
    dx_values.push(direction_index(smoothed_tr, smoothed_plus, smoothed_minus)?);
    for index in period..true_ranges.len() {
        smoothed_tr = wilder_sum(smoothed_tr, true_ranges[index], period_decimal)?;
        smoothed_plus = wilder_sum(smoothed_plus, plus_dm[index], period_decimal)?;
        smoothed_minus = wilder_sum(smoothed_minus, minus_dm[index], period_decimal)?;
        dx_values.push(direction_index(smoothed_tr, smoothed_plus, smoothed_minus)?);
    }
    if dx_values.len() < period {
        return Err(MarketFeatureError::InsufficientWarmup);
    }
    let mut adx = checked_div(checked_sum(&dx_values[..period])?, period_decimal)?;
    for dx in &dx_values[period..] {
        adx = wilder_average(adx, *dx, period_decimal)?;
    }
    Ok(adx)
}

fn true_ranges(candles: &[ClosedCandle]) -> Result<Vec<Decimal>, MarketFeatureError> {
    let mut ranges = Vec::with_capacity(candles.len().saturating_sub(1));
    for pair in candles.windows(2) {
        let high = pair[1].high().as_decimal();
        let low = pair[1].low().as_decimal();
        let previous_close = pair[0].close().as_decimal();
        let high_low = checked_sub(high, low)?;
        let high_previous = checked_abs(checked_sub(high, previous_close)?)?;
        let low_previous = checked_abs(checked_sub(low, previous_close)?)?;
        ranges.push(high_low.max(high_previous).max(low_previous));
    }
    Ok(ranges)
}

fn atr_true_ranges(candles: &[ClosedCandle]) -> Result<Vec<Decimal>, MarketFeatureError> {
    let Some(first) = candles.first() else {
        return Ok(Vec::new());
    };
    let mut ranges = Vec::with_capacity(candles.len());
    ranges.push(checked_sub(
        first.high().as_decimal(),
        first.low().as_decimal(),
    )?);
    ranges.extend(true_ranges(candles)?);
    Ok(ranges)
}

fn direction_index(
    smoothed_tr: Decimal,
    smoothed_plus: Decimal,
    smoothed_minus: Decimal,
) -> Result<Decimal, MarketFeatureError> {
    if smoothed_tr.is_zero() {
        return Err(MarketFeatureError::IndicatorUnavailable);
    }
    let plus_di = checked_div(
        checked_mul(Decimal::from(PERCENT), smoothed_plus)?,
        smoothed_tr,
    )?;
    let minus_di = checked_div(
        checked_mul(Decimal::from(PERCENT), smoothed_minus)?,
        smoothed_tr,
    )?;
    let denominator = checked_add(plus_di, minus_di)?;
    if denominator.is_zero() {
        return Ok(Decimal::ZERO);
    }
    checked_div(
        checked_mul(
            Decimal::from(PERCENT),
            checked_abs(checked_sub(plus_di, minus_di)?)?,
        )?,
        denominator,
    )
}

fn wilder_average(
    previous: Decimal,
    current: Decimal,
    period: Decimal,
) -> Result<Decimal, MarketFeatureError> {
    checked_div(
        checked_add(
            checked_mul(previous, checked_sub(period, Decimal::ONE)?)?,
            current,
        )?,
        period,
    )
}

fn wilder_sum(
    previous: Decimal,
    current: Decimal,
    period: Decimal,
) -> Result<Decimal, MarketFeatureError> {
    checked_add(
        checked_sub(previous, checked_div(previous, period)?)?,
        current,
    )
}

fn ema_alignment(
    close: Decimal,
    ema_fast: Decimal,
    ema_slow: Decimal,
    atr: Decimal,
) -> Result<EmaAlignment, MarketFeatureError> {
    if atr <= Decimal::ZERO {
        return Err(MarketFeatureError::IndicatorUnavailable);
    }
    let separation = checked_abs(checked_sub(ema_fast, ema_slow)?)?;
    let strong_threshold = checked_mul(parse_raw_decimal("0.5")?, atr)?;
    if close > ema_fast && ema_fast > ema_slow {
        return Ok(if separation >= strong_threshold {
            EmaAlignment::StrongBullish
        } else {
            EmaAlignment::Bullish
        });
    }
    if close < ema_fast && ema_fast < ema_slow {
        return Ok(if separation >= strong_threshold {
            EmaAlignment::StrongBearish
        } else {
            EmaAlignment::Bearish
        });
    }
    Ok(EmaAlignment::Mixed)
}

fn key_location(
    candle: &ClosedCandle,
    donchian_upper: Decimal,
    donchian_lower: Decimal,
    ema_slow: Decimal,
    atr: Decimal,
) -> Result<KeyLocation, MarketFeatureError> {
    if atr <= Decimal::ZERO {
        return Err(MarketFeatureError::IndicatorUnavailable);
    }
    let tolerance = checked_mul(parse_raw_decimal("0.25")?, atr)?;
    let mut support_distance = level_distance(
        candle.close().as_decimal(),
        candle.low().as_decimal(),
        donchian_lower,
    )?;
    let mut resistance_distance = level_distance(
        candle.close().as_decimal(),
        candle.high().as_decimal(),
        donchian_upper,
    )?;
    if candle.close().as_decimal() > ema_slow {
        support_distance = support_distance.min(level_distance(
            candle.close().as_decimal(),
            candle.low().as_decimal(),
            ema_slow,
        )?);
    } else if candle.close().as_decimal() < ema_slow {
        resistance_distance = resistance_distance.min(level_distance(
            candle.close().as_decimal(),
            candle.high().as_decimal(),
            ema_slow,
        )?);
    }
    let support = support_distance <= tolerance;
    let resistance = resistance_distance <= tolerance;
    Ok(match (support, resistance) {
        (true, true) if support_distance < resistance_distance => KeyLocation::Support,
        (true, true) if resistance_distance < support_distance => KeyLocation::Resistance,
        (true, true) => KeyLocation::None,
        (true, false) => KeyLocation::Support,
        (false, true) => KeyLocation::Resistance,
        (false, false) => KeyLocation::None,
    })
}

fn level_distance(
    close: Decimal,
    extreme: Decimal,
    level: Decimal,
) -> Result<Decimal, MarketFeatureError> {
    Ok(checked_abs(checked_sub(close, level)?)?.min(checked_abs(checked_sub(extreme, level)?)?))
}

fn detect_pattern(
    previous: &ClosedCandle,
    current: &ClosedCandle,
    key_location: KeyLocation,
    atr: Decimal,
) -> Result<Option<PatternObservation>, MarketFeatureError> {
    if key_location == KeyLocation::None {
        return Ok(None);
    }
    let open = current.open().as_decimal();
    let close = current.close().as_decimal();
    let high = current.high().as_decimal();
    let low = current.low().as_decimal();
    let current_low = open.min(close);
    let current_high = open.max(close);
    let current_body = checked_sub(current_high, current_low)?;
    let previous_open = previous.open().as_decimal();
    let previous_close = previous.close().as_decimal();
    let previous_low = previous_open.min(previous_close);
    let previous_high = previous_open.max(previous_close);
    let previous_body = checked_sub(previous_high, previous_low)?;
    let observation = if previous_close < previous_open
        && close > open
        && current_low <= previous_low
        && current_high >= previous_high
    {
        Some(PatternObservation {
            pattern: CandlePattern::BullishEngulfing,
            semantic: PatternSemantic::BullishReversal,
        })
    } else if previous_close > previous_open
        && close < open
        && current_low <= previous_low
        && current_high >= previous_high
    {
        Some(PatternObservation {
            pattern: CandlePattern::BearishEngulfing,
            semantic: PatternSemantic::BearishReversal,
        })
    } else if previous_body >= atr
        && current_body <= checked_mul(parse_raw_decimal("0.5")?, previous_body)?
        && current_low >= previous_low
        && current_high <= previous_high
        && previous_close < previous_open
        && close > open
    {
        Some(PatternObservation {
            pattern: CandlePattern::BullishHarami,
            semantic: PatternSemantic::BearishMomentumExhaustion,
        })
    } else if previous_body >= atr
        && current_body <= checked_mul(parse_raw_decimal("0.5")?, previous_body)?
        && current_low >= previous_low
        && current_high <= previous_high
        && previous_close > previous_open
        && close < open
    {
        Some(PatternObservation {
            pattern: CandlePattern::BearishHarami,
            semantic: PatternSemantic::BullishMomentumExhaustion,
        })
    } else if current_body > checked_mul(Decimal::from(2), atr)? && close > open {
        Some(PatternObservation {
            pattern: CandlePattern::BigBullish,
            semantic: PatternSemantic::BullishAttack,
        })
    } else if current_body > checked_mul(Decimal::from(2), atr)? && close < open {
        Some(PatternObservation {
            pattern: CandlePattern::BigBearish,
            semantic: PatternSemantic::BearishAttack,
        })
    } else {
        let candle_range = checked_sub(high, low)?;
        if current_body > Decimal::ZERO {
            let lower_shadow = checked_sub(current_low, low)?;
            let upper_shadow = checked_sub(high, current_high)?;
            if lower_shadow > checked_mul(Decimal::from(2), current_body)?
                && upper_shadow < checked_mul(parse_raw_decimal("0.1")?, current_body)?
            {
                Some(if key_location == KeyLocation::Support {
                    PatternObservation {
                        pattern: CandlePattern::Hammer,
                        semantic: PatternSemantic::BullishSupportRejection,
                    }
                } else {
                    PatternObservation {
                        pattern: CandlePattern::HangingMan,
                        semantic: PatternSemantic::BearishExhaustion,
                    }
                })
            } else if upper_shadow > checked_mul(Decimal::from(2), current_body)?
                && lower_shadow < checked_mul(parse_raw_decimal("0.1")?, current_body)?
            {
                Some(if key_location == KeyLocation::Support {
                    PatternObservation {
                        pattern: CandlePattern::InvertedHammer,
                        semantic: PatternSemantic::BullishSupportTest,
                    }
                } else {
                    PatternObservation {
                        pattern: CandlePattern::ShootingStar,
                        semantic: PatternSemantic::BearishResistanceRejection,
                    }
                })
            } else if candle_range > Decimal::ZERO
                && current_body < checked_mul(parse_raw_decimal("0.1")?, candle_range)?
            {
                Some(PatternObservation {
                    pattern: CandlePattern::Doji,
                    semantic: PatternSemantic::Indecision,
                })
            } else {
                None
            }
        } else if candle_range > Decimal::ZERO {
            Some(PatternObservation {
                pattern: CandlePattern::Doji,
                semantic: PatternSemantic::Indecision,
            })
        } else {
            None
        }
    };
    Ok(observation)
}

fn compute_spread_bps(book: &TopOfBook) -> Result<DomainDecimal, MarketFeatureError> {
    let bid = book.bid_price().as_decimal();
    let ask = book.ask_price().as_decimal();
    let midpoint = checked_div(checked_add(bid, ask)?, Decimal::from(2))?;
    domain_decimal(rounded(checked_div(
        checked_mul(checked_sub(ask, bid)?, Decimal::from(BPS))?,
        midpoint,
    )?))
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EligibilityEventKind {
    StructureChanged,
    KeyLocationReached,
    VolatilityExpanded,
    VolumeAnomaly,
    BreakoutAttempt,
    RetestEvent,
    PositionReviewDue,
    InvalidationRiskIncreased,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PrefilterRejectionReason {
    SnapshotExpired,
    SystemDisallowsNewAi,
    InstrumentDisabled,
    ActiveTradePlanNotDue,
    InsufficientLiquidity,
    SpreadTooWide,
    NoInformationDelta,
    DuplicateEvent,
    CooldownActive,
    LlmConcurrencyExhausted,
    DailyCallBudgetExhausted,
    DailyTokenBudgetExhausted,
    DailyCostBudgetExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LlmBudgetUsage {
    calls_used: u32,
    tokens_used: u64,
    cost_used_usd: DomainDecimal,
    in_flight: u8,
}

impl LlmBudgetUsage {
    #[must_use]
    pub const fn new(
        calls_used: u32,
        tokens_used: u64,
        cost_used_usd: DomainDecimal,
        in_flight: u8,
    ) -> Self {
        Self {
            calls_used,
            tokens_used,
            cost_used_usd,
            in_flight,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrefilterContext {
    now_unix_millis: u64,
    system_allows_new_ai: bool,
    instrument_enabled: bool,
    active_trade_plan: bool,
    position_review_due: bool,
    budget: LlmBudgetUsage,
}

impl PrefilterContext {
    #[must_use]
    pub const fn new(
        now_unix_millis: u64,
        system_allows_new_ai: bool,
        instrument_enabled: bool,
        active_trade_plan: bool,
        position_review_due: bool,
        budget: LlmBudgetUsage,
    ) -> Self {
        Self {
            now_unix_millis,
            system_allows_new_ai,
            instrument_enabled,
            active_trade_plan,
            position_review_due,
            budget,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EligibilityPolicy {
    minimum_quote_turnover: DomainDecimal,
    maximum_spread_bps: DomainDecimal,
    cooldown_millis: u64,
    event_ttl_millis: u64,
    daily_call_limit: u32,
    daily_token_limit: u64,
    daily_cost_limit_usd: DomainDecimal,
    maximum_concurrency: u8,
    reserved_tokens_per_call: u64,
    reserved_cost_per_call_usd: DomainDecimal,
    volume_anomaly_ratio: DomainDecimal,
    volatility_expansion_ratio: DomainDecimal,
    invalidation_risk_ratio: DomainDecimal,
}

impl EligibilityPolicy {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        minimum_quote_turnover: DomainDecimal,
        maximum_spread_bps: DomainDecimal,
        cooldown_millis: u64,
        event_ttl_millis: u64,
        daily_call_limit: u32,
        daily_token_limit: u64,
        daily_cost_limit_usd: DomainDecimal,
        maximum_concurrency: u8,
        reserved_tokens_per_call: u64,
        reserved_cost_per_call_usd: DomainDecimal,
        volume_anomaly_ratio: DomainDecimal,
        volatility_expansion_ratio: DomainDecimal,
        invalidation_risk_ratio: DomainDecimal,
    ) -> Result<Self, MarketFeatureError> {
        if minimum_quote_turnover < DomainDecimal::ZERO
            || maximum_spread_bps <= DomainDecimal::ZERO
            || cooldown_millis == 0
            || event_ttl_millis == 0
            || daily_call_limit == 0
            || daily_token_limit == 0
            || daily_cost_limit_usd <= DomainDecimal::ZERO
            || maximum_concurrency == 0
            || reserved_tokens_per_call == 0
            || reserved_cost_per_call_usd <= DomainDecimal::ZERO
            || volume_anomaly_ratio <= DomainDecimal::ZERO
            || volatility_expansion_ratio <= DomainDecimal::ZERO
            || invalidation_risk_ratio <= DomainDecimal::ZERO
        {
            return Err(MarketFeatureError::InvalidEligibilityPolicy);
        }
        Ok(Self {
            minimum_quote_turnover,
            maximum_spread_bps,
            cooldown_millis,
            event_ttl_millis,
            daily_call_limit,
            daily_token_limit,
            daily_cost_limit_usd,
            maximum_concurrency,
            reserved_tokens_per_call,
            reserved_cost_per_call_usd,
            volume_anomaly_ratio,
            volatility_expansion_ratio,
            invalidation_risk_ratio,
        })
    }

    pub fn vertical_slice_defaults() -> Result<Self, MarketFeatureError> {
        Self::new(
            parse_decimal("10000")?,
            parse_decimal("50")?,
            15 * 60 * 1_000,
            5 * 60 * 1_000,
            40,
            200_000,
            parse_decimal("2.00")?,
            1,
            4_000,
            parse_decimal("0.05")?,
            parse_decimal("2.0")?,
            parse_decimal("1.25")?,
            parse_decimal("1.5")?,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EligibilityEvent {
    feature_version: &'static str,
    instrument_id: InstrumentId,
    snapshot_hash: ContentHash,
    kinds: Vec<EligibilityEventKind>,
    emitted_at_unix_millis: u64,
    valid_until_unix_millis: u64,
    event_hash: ContentHash,
}

impl EligibilityEvent {
    #[must_use]
    pub const fn feature_version(&self) -> &'static str {
        self.feature_version
    }

    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn snapshot_hash(&self) -> ContentHash {
        self.snapshot_hash
    }

    #[must_use]
    pub fn kinds(&self) -> &[EligibilityEventKind] {
        &self.kinds
    }

    #[must_use]
    pub const fn emitted_at_unix_millis(&self) -> u64 {
        self.emitted_at_unix_millis
    }

    #[must_use]
    pub const fn valid_until_unix_millis(&self) -> u64 {
        self.valid_until_unix_millis
    }

    #[must_use]
    pub const fn event_hash(&self) -> ContentHash {
        self.event_hash
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrefilterDecision {
    Eligible(EligibilityEvent),
    Rejected(Vec<PrefilterRejectionReason>),
}

impl PrefilterDecision {
    #[must_use]
    pub const fn event(&self) -> Option<&EligibilityEvent> {
        match self {
            Self::Eligible(event) => Some(event),
            Self::Rejected(_) => None,
        }
    }

    #[must_use]
    pub fn rejection_reasons(&self) -> &[PrefilterRejectionReason] {
        match self {
            Self::Eligible(_) => &[],
            Self::Rejected(reasons) => reasons,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EligibilityEventEngine {
    policy: EligibilityPolicy,
    seen_events: BTreeMap<ContentHash, u64>,
    cooldown_until: BTreeMap<InstrumentId, u64>,
}

impl EligibilityEventEngine {
    #[must_use]
    pub const fn new(policy: EligibilityPolicy) -> Self {
        Self {
            policy,
            seen_events: BTreeMap::new(),
            cooldown_until: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn deduplication_entries(&self) -> usize {
        self.seen_events.len()
    }

    #[must_use]
    pub fn cooldown_entries(&self) -> usize {
        self.cooldown_until.len()
    }

    pub fn evaluate(
        &mut self,
        snapshot: &MarketFeatureSnapshot,
        previous: Option<&MarketFeatureSnapshot>,
        context: PrefilterContext,
    ) -> Result<PrefilterDecision, MarketFeatureError> {
        self.prune(context.now_unix_millis);
        let mut reasons = BTreeSet::new();
        if snapshot.is_expired_at(context.now_unix_millis) {
            reasons.insert(PrefilterRejectionReason::SnapshotExpired);
        }
        if !context.system_allows_new_ai {
            reasons.insert(PrefilterRejectionReason::SystemDisallowsNewAi);
        }
        if !context.instrument_enabled {
            reasons.insert(PrefilterRejectionReason::InstrumentDisabled);
        }
        if context.active_trade_plan && !context.position_review_due {
            reasons.insert(PrefilterRejectionReason::ActiveTradePlanNotDue);
        }
        if snapshot.primary().latest_turnover() < self.policy.minimum_quote_turnover {
            reasons.insert(PrefilterRejectionReason::InsufficientLiquidity);
        }
        if snapshot.spread_bps() > self.policy.maximum_spread_bps {
            reasons.insert(PrefilterRejectionReason::SpreadTooWide);
        }
        self.collect_budget_rejections(context.budget, &mut reasons)?;

        let kinds = self.event_kinds(snapshot, previous, context)?;
        if kinds.is_empty() {
            reasons.insert(PrefilterRejectionReason::NoInformationDelta);
        }
        let event_hash = hash_event(snapshot, &kinds);
        if self.seen_events.contains_key(&event_hash) {
            reasons.insert(PrefilterRejectionReason::DuplicateEvent);
        }
        if self
            .cooldown_until
            .get(snapshot.instrument_id())
            .is_some_and(|until| context.now_unix_millis < *until)
        {
            reasons.insert(PrefilterRejectionReason::CooldownActive);
        }
        if !reasons.is_empty() {
            return Ok(PrefilterDecision::Rejected(reasons.into_iter().collect()));
        }

        let valid_until_unix_millis = context
            .now_unix_millis
            .checked_add(self.policy.event_ttl_millis)
            .ok_or(MarketFeatureError::TimestampOverflow)?;
        let cooldown_until = context
            .now_unix_millis
            .checked_add(self.policy.cooldown_millis)
            .ok_or(MarketFeatureError::TimestampOverflow)?;
        if !self.cooldown_until.contains_key(snapshot.instrument_id())
            && self.cooldown_until.len() >= MAX_TRACKED_ELIGIBILITY_INSTRUMENTS
        {
            return Err(MarketFeatureError::EligibilityStateCapacityExceeded);
        }
        self.seen_events.insert(event_hash, valid_until_unix_millis);
        self.cooldown_until
            .insert(snapshot.instrument_id().clone(), cooldown_until);
        self.enforce_deduplication_bound();

        Ok(PrefilterDecision::Eligible(EligibilityEvent {
            feature_version: MARKET_FEATURES_VERSION_V1,
            instrument_id: snapshot.instrument_id().clone(),
            snapshot_hash: snapshot.snapshot_hash(),
            kinds,
            emitted_at_unix_millis: context.now_unix_millis,
            valid_until_unix_millis,
            event_hash,
        }))
    }

    fn collect_budget_rejections(
        &self,
        usage: LlmBudgetUsage,
        reasons: &mut BTreeSet<PrefilterRejectionReason>,
    ) -> Result<(), MarketFeatureError> {
        if usage.in_flight >= self.policy.maximum_concurrency {
            reasons.insert(PrefilterRejectionReason::LlmConcurrencyExhausted);
        }
        if usage.calls_used >= self.policy.daily_call_limit {
            reasons.insert(PrefilterRejectionReason::DailyCallBudgetExhausted);
        }
        if usage
            .tokens_used
            .checked_add(self.policy.reserved_tokens_per_call)
            .is_none_or(|tokens| tokens > self.policy.daily_token_limit)
        {
            reasons.insert(PrefilterRejectionReason::DailyTokenBudgetExhausted);
        }
        let projected_cost = checked_add(
            usage.cost_used_usd.as_decimal(),
            self.policy.reserved_cost_per_call_usd.as_decimal(),
        )?;
        if projected_cost > self.policy.daily_cost_limit_usd.as_decimal() {
            reasons.insert(PrefilterRejectionReason::DailyCostBudgetExhausted);
        }
        Ok(())
    }

    fn event_kinds(
        &self,
        snapshot: &MarketFeatureSnapshot,
        previous: Option<&MarketFeatureSnapshot>,
        context: PrefilterContext,
    ) -> Result<Vec<EligibilityEventKind>, MarketFeatureError> {
        let mut kinds = BTreeSet::new();
        if let Some(previous) = previous {
            if previous.instrument_id() != snapshot.instrument_id() {
                return Err(MarketFeatureError::InstrumentMismatch);
            }
            if previous.primary().ema_alignment() != snapshot.primary().ema_alignment()
                || previous.primary().key_location() != snapshot.primary().key_location()
            {
                kinds.insert(EligibilityEventKind::StructureChanged);
            }
            if snapshot.primary().key_location() != KeyLocation::None
                && snapshot.primary().key_location() != previous.primary().key_location()
            {
                kinds.insert(EligibilityEventKind::KeyLocationReached);
            }
            if ratio_at_least(
                snapshot.primary().atr(),
                previous.primary().atr(),
                self.policy.volatility_expansion_ratio,
            )? {
                kinds.insert(EligibilityEventKind::VolatilityExpanded);
            }
            if previous.primary().volume_ratio() < self.policy.volume_anomaly_ratio
                && snapshot.primary().volume_ratio() >= self.policy.volume_anomaly_ratio
            {
                kinds.insert(EligibilityEventKind::VolumeAnomaly);
            }
            if breakout_attempt(snapshot.primary()) && !breakout_attempt(previous.primary()) {
                kinds.insert(EligibilityEventKind::BreakoutAttempt);
            }
            if breakout_attempt(previous.primary())
                && snapshot.primary().key_location() != KeyLocation::None
            {
                kinds.insert(EligibilityEventKind::RetestEvent);
            }
            if context.active_trade_plan
                && ratio_at_least(
                    snapshot.primary().atr(),
                    previous.primary().atr(),
                    self.policy.invalidation_risk_ratio,
                )?
            {
                kinds.insert(EligibilityEventKind::InvalidationRiskIncreased);
            }
        } else {
            kinds.insert(EligibilityEventKind::StructureChanged);
            if snapshot.primary().key_location() != KeyLocation::None {
                kinds.insert(EligibilityEventKind::KeyLocationReached);
            }
            if snapshot.primary().volume_ratio() >= self.policy.volume_anomaly_ratio {
                kinds.insert(EligibilityEventKind::VolumeAnomaly);
            }
            if breakout_attempt(snapshot.primary()) {
                kinds.insert(EligibilityEventKind::BreakoutAttempt);
            }
        }
        if context.position_review_due {
            kinds.insert(EligibilityEventKind::PositionReviewDue);
        }
        Ok(kinds.into_iter().collect())
    }

    fn prune(&mut self, now_unix_millis: u64) {
        self.seen_events
            .retain(|_, valid_until| now_unix_millis < *valid_until);
        self.cooldown_until
            .retain(|_, valid_until| now_unix_millis < *valid_until);
    }

    fn enforce_deduplication_bound(&mut self) {
        while self.seen_events.len() > MAX_EVENT_DEDUPLICATION_ENTRIES {
            let oldest = self
                .seen_events
                .iter()
                .min_by_key(|(hash, valid_until)| (**valid_until, **hash))
                .map(|(hash, _)| *hash);
            if let Some(oldest) = oldest {
                self.seen_events.remove(&oldest);
            } else {
                break;
            }
        }
    }
}

fn breakout_attempt(features: &TimeframeFeatures) -> bool {
    features.latest_close() > features.donchian_upper()
        || features.latest_close() < features.donchian_lower()
}

fn ratio_at_least(
    current: DomainDecimal,
    previous: DomainDecimal,
    ratio: DomainDecimal,
) -> Result<bool, MarketFeatureError> {
    if previous <= DomainDecimal::ZERO {
        return Ok(false);
    }
    Ok(current.as_decimal() >= checked_mul(previous.as_decimal(), ratio.as_decimal())?)
}

fn hash_snapshot(
    instrument_id: &InstrumentId,
    input_hash: ContentHash,
    primary: &TimeframeFeatures,
    confirmation: &TimeframeFeatures,
    bid_price: DomainDecimal,
    ask_price: DomainDecimal,
    spread_bps: DomainDecimal,
) -> ContentHash {
    let mut hasher = CanonicalHasher::new("market-snapshot-v1");
    hasher.field(MARKET_FEATURES_VERSION_V1);
    hasher.field(&instrument_id.to_string());
    hasher.bytes(input_hash.as_bytes());
    hash_timeframe(&mut hasher, primary);
    hash_timeframe(&mut hasher, confirmation);
    hasher.decimal(bid_price);
    hasher.decimal(ask_price);
    hasher.decimal(spread_bps);
    hasher.finish()
}

fn hash_market_inputs(
    instrument_id: &InstrumentId,
    primary: &[ClosedCandle],
    confirmation: &[ClosedCandle],
    book: &TopOfBook,
) -> ContentHash {
    let mut hasher = CanonicalHasher::new("market-input-v1");
    hasher.field(MARKET_FEATURES_VERSION_V1);
    hasher.field(&instrument_id.to_string());
    for candles in [primary, confirmation] {
        hasher.field(
            candles
                .first()
                .map_or("missing", |candle| candle.timeframe().canonical_name()),
        );
        for candle in candles {
            hasher.u64(candle.open_at_unix_millis());
            for value in [
                candle.open(),
                candle.high(),
                candle.low(),
                candle.close(),
                candle.volume(),
                candle.turnover(),
            ] {
                hasher.decimal(value);
            }
        }
    }
    for value in [
        book.bid_price(),
        book.bid_quantity(),
        book.ask_price(),
        book.ask_quantity(),
    ] {
        hasher.decimal(value);
    }
    hasher.finish()
}

fn hash_timeframe(hasher: &mut CanonicalHasher, features: &TimeframeFeatures) {
    hasher.field(features.timeframe.canonical_name());
    hasher.u64(features.candle_open_at_unix_millis);
    hasher.u64(features.candle_close_at_unix_millis);
    for value in [
        features.latest_open,
        features.latest_high,
        features.latest_low,
        features.latest_close,
        features.latest_volume,
        features.latest_turnover,
        features.donchian_upper,
        features.donchian_lower,
        features.ema_fast,
        features.ema_slow,
        features.rsi,
        features.atr,
        features.adx,
        features.volume_ratio,
    ] {
        hasher.decimal(value);
    }
    hasher.field(match features.ema_alignment {
        EmaAlignment::StrongBullish => "strong-bullish",
        EmaAlignment::Bullish => "bullish",
        EmaAlignment::StrongBearish => "strong-bearish",
        EmaAlignment::Bearish => "bearish",
        EmaAlignment::Mixed => "mixed",
    });
    hasher.field(match features.key_location {
        KeyLocation::None => "none",
        KeyLocation::Support => "support",
        KeyLocation::Resistance => "resistance",
    });
    if let Some(pattern) = features.pattern {
        hasher.field(&format!("{:?}", pattern.pattern));
        hasher.field(&format!("{:?}", pattern.semantic));
    } else {
        hasher.field("no-pattern");
    }
}

fn hash_event(snapshot: &MarketFeatureSnapshot, kinds: &[EligibilityEventKind]) -> ContentHash {
    let mut hasher = CanonicalHasher::new("eligibility-event-v1");
    hasher.field(MARKET_FEATURES_VERSION_V1);
    hasher.field(&snapshot.instrument_id().to_string());
    hasher.bytes(snapshot.snapshot_hash().as_bytes());
    for kind in kinds {
        hasher.field(&format!("{kind:?}"));
    }
    hasher.finish()
}

struct CanonicalHasher(Sha256);

impl CanonicalHasher {
    fn new(schema: &str) -> Self {
        let mut value = Self(Sha256::new());
        value.field(schema);
        value
    }

    fn field(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn decimal(&mut self, value: DomainDecimal) {
        self.field(&value.as_decimal().normalize().to_string());
    }

    fn u64(&mut self, value: u64) {
        self.field(&value.to_string());
    }

    fn bytes(&mut self, value: &[u8]) {
        self.0.update(value.len().to_be_bytes());
        self.0.update(value);
    }

    fn finish(self) -> ContentHash {
        ContentHash(self.0.finalize().into())
    }
}

fn checked_sum(values: &[Decimal]) -> Result<Decimal, MarketFeatureError> {
    values
        .iter()
        .try_fold(Decimal::ZERO, |total, value| checked_add(total, *value))
}

fn checked_add(left: Decimal, right: Decimal) -> Result<Decimal, MarketFeatureError> {
    left.checked_add(right)
        .ok_or(MarketFeatureError::ArithmeticOverflow)
}

fn checked_sub(left: Decimal, right: Decimal) -> Result<Decimal, MarketFeatureError> {
    left.checked_sub(right)
        .ok_or(MarketFeatureError::ArithmeticOverflow)
}

fn checked_mul(left: Decimal, right: Decimal) -> Result<Decimal, MarketFeatureError> {
    left.checked_mul(right)
        .ok_or(MarketFeatureError::ArithmeticOverflow)
}

fn checked_div(left: Decimal, right: Decimal) -> Result<Decimal, MarketFeatureError> {
    if right.is_zero() {
        return Err(MarketFeatureError::DivisionByZero);
    }
    left.checked_div(right)
        .ok_or(MarketFeatureError::ArithmeticOverflow)
}

fn checked_abs(value: Decimal) -> Result<Decimal, MarketFeatureError> {
    if value < Decimal::ZERO {
        checked_sub(Decimal::ZERO, value)
    } else {
        Ok(value)
    }
}

fn decimal_from_usize(value: usize) -> Result<Decimal, MarketFeatureError> {
    i64::try_from(value)
        .map(Decimal::from)
        .map_err(|_| MarketFeatureError::ArithmeticOverflow)
}

fn domain_decimal(value: Decimal) -> Result<DomainDecimal, MarketFeatureError> {
    value
        .normalize()
        .to_string()
        .parse()
        .map_err(|_| MarketFeatureError::ArithmeticOverflow)
}

fn rounded(value: Decimal) -> Decimal {
    value.round_dp_with_strategy(OUTPUT_DECIMAL_PLACES, RoundingStrategy::MidpointNearestEven)
}

fn parse_raw_decimal(value: &str) -> Result<Decimal, MarketFeatureError> {
    value
        .parse()
        .map_err(|_| MarketFeatureError::ArithmeticOverflow)
}

fn parse_decimal(value: &str) -> Result<DomainDecimal, MarketFeatureError> {
    value
        .parse()
        .map_err(|_| MarketFeatureError::InvalidEligibilityPolicy)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarketFeatureError {
    UnsupportedInstrument,
    CandleNotClosed,
    CandleMisaligned,
    NonPositivePrice,
    NegativeMarketAmount,
    InvalidOhlcEnvelope,
    TimestampOverflow,
    NonPositiveBookValue,
    CrossedBook,
    FutureBook,
    StaleBook,
    InsufficientWarmup,
    TimeframeMismatch,
    InstrumentMismatch,
    FutureCandle,
    StaleCandle,
    DuplicateCandle,
    OutOfOrderCandle,
    CandleGap,
    TimeframesMisaligned,
    IndicatorUnavailable,
    EligibilityStateCapacityExceeded,
    ArithmeticOverflow,
    DivisionByZero,
    InvalidEligibilityPolicy,
}

impl fmt::Display for MarketFeatureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedInstrument => "market feature input must use a Bybit Spot instrument",
            Self::CandleNotClosed => "market feature input candle is not closed",
            Self::CandleMisaligned => "market feature input candle is not timeframe-aligned",
            Self::NonPositivePrice => "market feature input contains a non-positive price",
            Self::NegativeMarketAmount => "market feature input contains a negative amount",
            Self::InvalidOhlcEnvelope => "market feature input has an invalid OHLC envelope",
            Self::TimestampOverflow => "market feature timestamp arithmetic overflowed",
            Self::NonPositiveBookValue => "top of book contains a non-positive value",
            Self::CrossedBook => "top of book is crossed or locked",
            Self::FutureBook => "top of book is from the future",
            Self::StaleBook => "top of book is stale",
            Self::InsufficientWarmup => "market feature input does not satisfy warm-up",
            Self::TimeframeMismatch => "market feature input timeframe does not match",
            Self::InstrumentMismatch => "market feature input instruments do not match",
            Self::FutureCandle => "market feature input contains a future candle",
            Self::StaleCandle => "market feature input candle is stale",
            Self::DuplicateCandle => "market feature input contains a duplicate candle",
            Self::OutOfOrderCandle => "market feature input candles are out of order",
            Self::CandleGap => "market feature input contains a candle gap",
            Self::TimeframesMisaligned => "15m and 1h feature windows are not aligned",
            Self::IndicatorUnavailable => {
                "market feature indicator is unavailable for the supplied input"
            }
            Self::EligibilityStateCapacityExceeded => {
                "eligibility state exceeds the configured instrument bound"
            }
            Self::ArithmeticOverflow => "market feature decimal arithmetic overflowed",
            Self::DivisionByZero => "market feature decimal arithmetic divided by zero",
            Self::InvalidEligibilityPolicy => "eligibility policy is invalid",
        })
    }
}

impl std::error::Error for MarketFeatureError {}

#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use super::*;

    fn instrument() -> InstrumentId {
        InstrumentId::from_str("bybit:spot:BTCUSDT").expect("test instrument must be valid")
    }

    fn decimal(value: &str) -> DomainDecimal {
        DomainDecimal::from_str(value).expect("test decimal must be valid")
    }

    fn candle(index: u64, open: &str, high: &str, low: &str, close: &str) -> ClosedCandle {
        ClosedCandle::new(
            instrument(),
            MarketTimeframe::FifteenMinutes,
            index * MarketTimeframe::FifteenMinutes.duration_millis(),
            decimal(open),
            decimal(high),
            decimal(low),
            decimal(close),
            decimal("100"),
            decimal("10000"),
            true,
        )
        .expect("test candle must be valid")
    }

    #[test]
    fn all_eleven_patterns_use_the_frozen_priority_and_semantics() {
        let neutral = candle(0, "100", "101", "99", "100");
        let cases = [
            (
                neutral.clone(),
                candle(1, "100", "122", "99", "121"),
                KeyLocation::Support,
                CandlePattern::BigBullish,
                PatternSemantic::BullishAttack,
            ),
            (
                neutral.clone(),
                candle(1, "121", "122", "99", "100"),
                KeyLocation::Resistance,
                CandlePattern::BigBearish,
                PatternSemantic::BearishAttack,
            ),
            (
                neutral.clone(),
                candle(1, "100", "102.1", "95", "102"),
                KeyLocation::Support,
                CandlePattern::Hammer,
                PatternSemantic::BullishSupportRejection,
            ),
            (
                neutral.clone(),
                candle(1, "100", "102.1", "95", "102"),
                KeyLocation::Resistance,
                CandlePattern::HangingMan,
                PatternSemantic::BearishExhaustion,
            ),
            (
                neutral.clone(),
                candle(1, "100", "107", "99.9", "102"),
                KeyLocation::Resistance,
                CandlePattern::ShootingStar,
                PatternSemantic::BearishResistanceRejection,
            ),
            (
                neutral.clone(),
                candle(1, "100", "107", "99.9", "102"),
                KeyLocation::Support,
                CandlePattern::InvertedHammer,
                PatternSemantic::BullishSupportTest,
            ),
            (
                candle(0, "105", "106", "99", "100"),
                candle(1, "99", "107", "98", "106"),
                KeyLocation::Support,
                CandlePattern::BullishEngulfing,
                PatternSemantic::BullishReversal,
            ),
            (
                candle(0, "100", "106", "99", "105"),
                candle(1, "106", "107", "98", "99"),
                KeyLocation::Resistance,
                CandlePattern::BearishEngulfing,
                PatternSemantic::BearishReversal,
            ),
            (
                candle(0, "120", "121", "99", "100"),
                candle(1, "105", "111", "104", "110"),
                KeyLocation::Support,
                CandlePattern::BullishHarami,
                PatternSemantic::BearishMomentumExhaustion,
            ),
            (
                candle(0, "100", "121", "99", "120"),
                candle(1, "115", "116", "109", "110"),
                KeyLocation::Resistance,
                CandlePattern::BearishHarami,
                PatternSemantic::BullishMomentumExhaustion,
            ),
            (
                neutral,
                candle(1, "100", "105", "95", "100"),
                KeyLocation::Support,
                CandlePattern::Doji,
                PatternSemantic::Indecision,
            ),
        ];

        for (previous, current, location, expected_pattern, expected_semantic) in cases {
            let observation = detect_pattern(
                &previous,
                &current,
                location,
                parse_raw_decimal("10").expect("test ATR must parse"),
            )
            .expect("pattern evaluation must succeed")
            .expect("pattern must be observed");
            assert_eq!(observation.pattern(), expected_pattern);
            assert_eq!(observation.semantic(), expected_semantic);
        }
    }

    #[test]
    fn engulfing_wins_a_multi_pattern_conflict() {
        let previous = candle(0, "105", "106", "99", "100");
        let current = candle(1, "99", "126", "98", "125");
        let observation = detect_pattern(
            &previous,
            &current,
            KeyLocation::Support,
            parse_raw_decimal("10").expect("test ATR must parse"),
        )
        .expect("pattern evaluation must succeed")
        .expect("pattern must be observed");

        assert_eq!(observation.pattern(), CandlePattern::BullishEngulfing);
    }

    #[test]
    fn unavailable_flat_momentum_is_not_fabricated_as_neutral() {
        let flat = vec![Decimal::from(100); FEATURE_CANDLE_WINDOW];
        assert_eq!(
            wilder_rsi(&flat, WILDER_PERIOD),
            Err(MarketFeatureError::IndicatorUnavailable)
        );
    }

    #[test]
    fn event_deduplication_storage_is_hard_bounded() {
        let policy = EligibilityPolicy::vertical_slice_defaults().expect("policy must be valid");
        let mut engine = EligibilityEventEngine::new(policy);
        for index in 0..=MAX_EVENT_DEDUPLICATION_ENTRIES {
            let mut bytes = [0_u8; 32];
            bytes[..8].copy_from_slice(
                &u64::try_from(index)
                    .expect("test index must fit")
                    .to_be_bytes(),
            );
            engine.seen_events.insert(
                ContentHash(bytes),
                u64::try_from(index).expect("test index must fit") + 1,
            );
        }

        engine.enforce_deduplication_bound();

        assert_eq!(
            engine.deduplication_entries(),
            MAX_EVENT_DEDUPLICATION_ENTRIES
        );
    }
}
