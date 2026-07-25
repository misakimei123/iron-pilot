use core::fmt;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::{
    AI_DECISION_CONTEXT_SCHEMA_VERSION_V1, AI_TRADING_PLAN_SCHEMA_VERSION_V3, ClosedCandle,
    DomainDecimal, EligibilityEventEngine, EligibilityEventKind, EligibilityPolicy, InstrumentId,
    LlmBudgetUsage, MARKET_FEATURES_VERSION_V1, MAX_TRACKED_ELIGIBILITY_INSTRUMENTS,
    MarketDataSource, MarketFeatureEngine, MarketFeatureError, MarketFeatureSnapshot,
    MarketTimeframe, PrefilterContext, PrefilterDecision, PrefilterRejectionReason, TopOfBook,
};

pub const MARKET_REPLAY_SCHEMA_VERSION_V2: &str = "ironpilot-market-replay-v2";
pub const MARKET_REPLAY_REPORT_VERSION_V2: &str = "ironpilot-market-replay-report-v2";
pub const REPLAY_DETERMINISTIC_SEED_V1: u64 = 0x4952_4f4e_5049_4c4f;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ReplayHash([u8; 32]);

impl ReplayHash {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for ReplayHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayClock {
    next_unix_millis: Option<u64>,
    end_unix_millis: u64,
    step_millis: u64,
}

impl ReplayClock {
    pub fn new(
        start_unix_millis: u64,
        end_unix_millis: u64,
        step_millis: u64,
    ) -> Result<Self, ReplayError> {
        if step_millis == 0
            || start_unix_millis > end_unix_millis
            || !start_unix_millis.is_multiple_of(step_millis)
            || !end_unix_millis.is_multiple_of(step_millis)
            || !(end_unix_millis - start_unix_millis).is_multiple_of(step_millis)
        {
            return Err(ReplayError::InvalidClock);
        }
        Ok(Self {
            next_unix_millis: Some(start_unix_millis),
            end_unix_millis,
            step_millis,
        })
    }
}

impl Iterator for ReplayClock {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next_unix_millis?;
        self.next_unix_millis = if current == self.end_unix_millis {
            None
        } else {
            current
                .checked_add(self.step_millis)
                .filter(|next| *next <= self.end_unix_millis)
        };
        Some(current)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayInstrumentData {
    instrument_id: InstrumentId,
    primary_candles: Vec<ClosedCandle>,
    confirmation_candles: Vec<ClosedCandle>,
    books: Vec<TopOfBook>,
}

impl ReplayInstrumentData {
    pub fn new(
        instrument_id: InstrumentId,
        primary_candles: Vec<ClosedCandle>,
        confirmation_candles: Vec<ClosedCandle>,
        books: Vec<TopOfBook>,
    ) -> Result<Self, ReplayError> {
        validate_candle_series(
            &instrument_id,
            &primary_candles,
            MarketTimeframe::FifteenMinutes,
        )?;
        validate_candle_series(
            &instrument_id,
            &confirmation_candles,
            MarketTimeframe::OneHour,
        )?;
        if books.is_empty() {
            return Err(ReplayError::MissingMarketData);
        }
        for book in &books {
            if book.instrument_id() != &instrument_id {
                return Err(ReplayError::InstrumentMismatch);
            }
        }
        if books
            .windows(2)
            .any(|pair| pair[0].observed_at_unix_millis() >= pair[1].observed_at_unix_millis())
        {
            return Err(ReplayError::MarketDataOutOfOrder);
        }
        Ok(Self {
            instrument_id,
            primary_candles,
            confirmation_candles,
            books,
        })
    }

    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub fn primary_candles(&self) -> &[ClosedCandle] {
        &self.primary_candles
    }

    #[must_use]
    pub fn confirmation_candles(&self) -> &[ClosedCandle] {
        &self.confirmation_candles
    }

    #[must_use]
    pub fn books(&self) -> &[TopOfBook] {
        &self.books
    }
}

fn validate_candle_series(
    instrument_id: &InstrumentId,
    candles: &[ClosedCandle],
    timeframe: MarketTimeframe,
) -> Result<(), ReplayError> {
    if candles.is_empty() {
        return Err(ReplayError::MissingMarketData);
    }
    for candle in candles {
        if candle.instrument_id() != instrument_id {
            return Err(ReplayError::InstrumentMismatch);
        }
        if candle.timeframe() != timeframe {
            return Err(ReplayError::TimeframeMismatch);
        }
    }
    if candles.windows(2).any(|pair| {
        pair[0].close_at_unix_millis() != pair[1].open_at_unix_millis()
            || pair[0].open_at_unix_millis() >= pair[1].open_at_unix_millis()
    }) {
        return Err(ReplayError::MarketDataOutOfOrder);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayDataset {
    instruments: Vec<ReplayInstrumentData>,
    dataset_hash: ReplayHash,
}

impl ReplayDataset {
    pub fn new(mut instruments: Vec<ReplayInstrumentData>) -> Result<Self, ReplayError> {
        if instruments.is_empty() || instruments.len() > MAX_TRACKED_ELIGIBILITY_INSTRUMENTS {
            return Err(ReplayError::InstrumentCountOutOfRange);
        }
        instruments.sort_by(|left, right| left.instrument_id.cmp(&right.instrument_id));
        if instruments
            .windows(2)
            .any(|pair| pair[0].instrument_id == pair[1].instrument_id)
        {
            return Err(ReplayError::DuplicateInstrument);
        }
        let dataset_hash = hash_dataset(&instruments);
        Ok(Self {
            instruments,
            dataset_hash,
        })
    }

    #[must_use]
    pub fn instruments(&self) -> &[ReplayInstrumentData] {
        &self.instruments
    }

    #[must_use]
    pub const fn dataset_hash(&self) -> ReplayHash {
        self.dataset_hash
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayManifest {
    schema_version: &'static str,
    feature_version: &'static str,
    context_schema_version: &'static str,
    ai_trading_plan_schema_version: &'static str,
    deterministic_seed: u64,
    start_unix_millis: u64,
    end_unix_millis: u64,
    step_millis: u64,
    instruments: Vec<InstrumentId>,
    dataset_hash: ReplayHash,
    manifest_hash: ReplayHash,
}

impl ReplayManifest {
    pub fn new(
        dataset: &ReplayDataset,
        start_unix_millis: u64,
        end_unix_millis: u64,
    ) -> Result<Self, ReplayError> {
        let step_millis = MarketTimeframe::FifteenMinutes.duration_millis();
        ReplayClock::new(start_unix_millis, end_unix_millis, step_millis)?;
        let instruments = dataset
            .instruments()
            .iter()
            .map(|data| data.instrument_id().clone())
            .collect();
        let mut manifest = Self {
            schema_version: MARKET_REPLAY_SCHEMA_VERSION_V2,
            feature_version: MARKET_FEATURES_VERSION_V1,
            context_schema_version: AI_DECISION_CONTEXT_SCHEMA_VERSION_V1,
            ai_trading_plan_schema_version: AI_TRADING_PLAN_SCHEMA_VERSION_V3,
            deterministic_seed: REPLAY_DETERMINISTIC_SEED_V1,
            start_unix_millis,
            end_unix_millis,
            step_millis,
            instruments,
            dataset_hash: dataset.dataset_hash(),
            manifest_hash: ReplayHash([0; 32]),
        };
        manifest.manifest_hash = hash_manifest(&manifest);
        Ok(manifest)
    }

    #[must_use]
    pub const fn schema_version(&self) -> &'static str {
        self.schema_version
    }

    #[must_use]
    pub const fn feature_version(&self) -> &'static str {
        self.feature_version
    }

    #[must_use]
    pub const fn context_schema_version(&self) -> &'static str {
        self.context_schema_version
    }

    #[must_use]
    pub const fn ai_trading_plan_schema_version(&self) -> &'static str {
        self.ai_trading_plan_schema_version
    }

    #[must_use]
    pub const fn deterministic_seed(&self) -> u64 {
        self.deterministic_seed
    }

    #[must_use]
    pub const fn start_unix_millis(&self) -> u64 {
        self.start_unix_millis
    }

    #[must_use]
    pub const fn end_unix_millis(&self) -> u64 {
        self.end_unix_millis
    }

    #[must_use]
    pub const fn step_millis(&self) -> u64 {
        self.step_millis
    }

    #[must_use]
    pub fn instruments(&self) -> &[InstrumentId] {
        &self.instruments
    }

    #[must_use]
    pub const fn dataset_hash(&self) -> ReplayHash {
        self.dataset_hash
    }

    #[must_use]
    pub const fn manifest_hash(&self) -> ReplayHash {
        self.manifest_hash
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        let mut output = String::new();
        write!(
            output,
            "{{\"schema_version\":\"{}\",\"feature_version\":\"{}\",\"context_schema_version\":\"{}\",\"ai_trading_plan_schema_version\":\"{}\",\"deterministic_seed\":{},\"start_unix_millis\":{},\"end_unix_millis\":{},\"step_millis\":{},\"instruments\":[",
            self.schema_version,
            self.feature_version,
            self.context_schema_version,
            self.ai_trading_plan_schema_version,
            self.deterministic_seed,
            self.start_unix_millis,
            self.end_unix_millis,
            self.step_millis
        )
        .expect("writing to String cannot fail");
        for (index, instrument_id) in self.instruments.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            write!(output, "\"{instrument_id}\"").expect("writing to String cannot fail");
        }
        write!(
            output,
            "],\"dataset_hash\":\"{}\",\"manifest_hash\":\"{}\"}}",
            self.dataset_hash, self.manifest_hash
        )
        .expect("writing to String cannot fail");
        output
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplayEligibilityOutcome {
    Event {
        event_hash: ReplayHash,
        kinds: Vec<EligibilityEventKind>,
    },
    Rejected(Vec<PrefilterRejectionReason>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayRecord {
    as_of_unix_millis: u64,
    instrument_id: InstrumentId,
    latest_primary_close_at_unix_millis: u64,
    latest_confirmation_close_at_unix_millis: u64,
    snapshot_hash: ReplayHash,
    eligibility: ReplayEligibilityOutcome,
}

impl ReplayRecord {
    #[must_use]
    pub const fn as_of_unix_millis(&self) -> u64 {
        self.as_of_unix_millis
    }

    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn latest_primary_close_at_unix_millis(&self) -> u64 {
        self.latest_primary_close_at_unix_millis
    }

    #[must_use]
    pub const fn latest_confirmation_close_at_unix_millis(&self) -> u64 {
        self.latest_confirmation_close_at_unix_millis
    }

    #[must_use]
    pub const fn snapshot_hash(&self) -> ReplayHash {
        self.snapshot_hash
    }

    #[must_use]
    pub const fn eligibility(&self) -> &ReplayEligibilityOutcome {
        &self.eligibility
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayReport {
    schema_version: &'static str,
    manifest_hash: ReplayHash,
    context_schema_version: &'static str,
    ai_trading_plan_schema_version: &'static str,
    deterministic_seed: u64,
    records: Vec<ReplayRecord>,
    output_hash: ReplayHash,
}

impl ReplayReport {
    #[must_use]
    pub const fn schema_version(&self) -> &'static str {
        self.schema_version
    }

    #[must_use]
    pub const fn manifest_hash(&self) -> ReplayHash {
        self.manifest_hash
    }

    #[must_use]
    pub const fn context_schema_version(&self) -> &'static str {
        self.context_schema_version
    }

    #[must_use]
    pub const fn ai_trading_plan_schema_version(&self) -> &'static str {
        self.ai_trading_plan_schema_version
    }

    #[must_use]
    pub const fn deterministic_seed(&self) -> u64 {
        self.deterministic_seed
    }

    #[must_use]
    pub fn records(&self) -> &[ReplayRecord] {
        &self.records
    }

    #[must_use]
    pub const fn output_hash(&self) -> ReplayHash {
        self.output_hash
    }

    #[must_use]
    pub fn event_count(&self) -> usize {
        self.records
            .iter()
            .filter(|record| matches!(record.eligibility, ReplayEligibilityOutcome::Event { .. }))
            .count()
    }

    #[must_use]
    pub fn rejected_count(&self) -> usize {
        self.records.len() - self.event_count()
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        let mut output = String::new();
        write!(
            output,
            "{{\"schema_version\":\"{}\",\"manifest_hash\":\"{}\",\"context_schema_version\":\"{}\",\"ai_trading_plan_schema_version\":\"{}\",\"deterministic_seed\":{},\"record_count\":{},\"event_count\":{},\"rejected_count\":{},\"records\":[",
            self.schema_version,
            self.manifest_hash,
            self.context_schema_version,
            self.ai_trading_plan_schema_version,
            self.deterministic_seed,
            self.records.len(),
            self.event_count(),
            self.rejected_count()
        )
        .expect("writing to String cannot fail");
        for (index, record) in self.records.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            write!(
                output,
                "{{\"as_of_unix_millis\":{},\"instrument_id\":\"{}\",\"latest_primary_close_at_unix_millis\":{},\"latest_confirmation_close_at_unix_millis\":{},\"snapshot_hash\":\"{}\",\"eligibility\":",
                record.as_of_unix_millis,
                record.instrument_id,
                record.latest_primary_close_at_unix_millis,
                record.latest_confirmation_close_at_unix_millis,
                record.snapshot_hash
            )
            .expect("writing to String cannot fail");
            write_eligibility_json(&mut output, &record.eligibility);
            output.push('}');
        }
        write!(output, "],\"output_hash\":\"{}\"}}", self.output_hash)
            .expect("writing to String cannot fail");
        output
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ReplayRunner;

impl ReplayRunner {
    pub fn run(
        manifest: &ReplayManifest,
        dataset: &ReplayDataset,
    ) -> Result<ReplayReport, ReplayError> {
        validate_manifest(manifest, dataset)?;
        let policy =
            EligibilityPolicy::vertical_slice_defaults().map_err(ReplayError::MarketFeature)?;
        let mut states: Vec<ReplayInstrumentState> = dataset
            .instruments()
            .iter()
            .map(|data| ReplayInstrumentState {
                instrument_id: data.instrument_id().clone(),
                engine: EligibilityEventEngine::new(policy),
                previous: None,
            })
            .collect();
        let clock = ReplayClock::new(
            manifest.start_unix_millis,
            manifest.end_unix_millis,
            manifest.step_millis,
        )?;
        let mut records = Vec::new();

        for now_unix_millis in clock {
            for (data, state) in dataset.instruments().iter().zip(&mut states) {
                if data.instrument_id() != &state.instrument_id {
                    return Err(ReplayError::InstrumentMismatch);
                }
                let primary = available_candles(&data.primary_candles, now_unix_millis);
                let confirmation = available_candles(&data.confirmation_candles, now_unix_millis);
                let book = available_book(&data.books, now_unix_millis)
                    .ok_or(ReplayError::MissingMarketData)?;
                let snapshot = MarketFeatureEngine::compute(
                    primary,
                    confirmation,
                    book,
                    now_unix_millis,
                    MarketDataSource::Replay,
                )
                .map_err(ReplayError::MarketFeature)?;
                let latest_primary_close_at_unix_millis = primary
                    .last()
                    .ok_or(ReplayError::MissingMarketData)?
                    .close_at_unix_millis();
                let latest_confirmation_close_at_unix_millis = confirmation
                    .last()
                    .ok_or(ReplayError::MissingMarketData)?
                    .close_at_unix_millis();
                if latest_primary_close_at_unix_millis > now_unix_millis
                    || latest_confirmation_close_at_unix_millis > now_unix_millis
                    || book.observed_at_unix_millis() > now_unix_millis
                {
                    return Err(ReplayError::FutureMarketData);
                }
                let decision = state
                    .engine
                    .evaluate(
                        &snapshot,
                        state.previous.as_ref(),
                        PrefilterContext::new(
                            now_unix_millis,
                            true,
                            true,
                            false,
                            false,
                            LlmBudgetUsage::new(0, 0, DomainDecimal::ZERO, 0),
                        ),
                    )
                    .map_err(ReplayError::MarketFeature)?;
                let eligibility = match decision {
                    PrefilterDecision::Eligible(event) => ReplayEligibilityOutcome::Event {
                        event_hash: ReplayHash(*event.event_hash().as_bytes()),
                        kinds: event.kinds().to_vec(),
                    },
                    PrefilterDecision::Rejected(reasons) => {
                        ReplayEligibilityOutcome::Rejected(reasons)
                    }
                };
                records.push(ReplayRecord {
                    as_of_unix_millis: now_unix_millis,
                    instrument_id: data.instrument_id().clone(),
                    latest_primary_close_at_unix_millis,
                    latest_confirmation_close_at_unix_millis,
                    snapshot_hash: ReplayHash(*snapshot.snapshot_hash().as_bytes()),
                    eligibility,
                });
                state.previous = Some(snapshot);
            }
        }

        let output_hash = hash_report(manifest, &records);
        Ok(ReplayReport {
            schema_version: MARKET_REPLAY_REPORT_VERSION_V2,
            manifest_hash: manifest.manifest_hash,
            context_schema_version: manifest.context_schema_version,
            ai_trading_plan_schema_version: manifest.ai_trading_plan_schema_version,
            deterministic_seed: manifest.deterministic_seed,
            records,
            output_hash,
        })
    }
}

struct ReplayInstrumentState {
    instrument_id: InstrumentId,
    engine: EligibilityEventEngine,
    previous: Option<MarketFeatureSnapshot>,
}

fn validate_manifest(
    manifest: &ReplayManifest,
    dataset: &ReplayDataset,
) -> Result<(), ReplayError> {
    if manifest.schema_version != MARKET_REPLAY_SCHEMA_VERSION_V2
        || manifest.feature_version != MARKET_FEATURES_VERSION_V1
    {
        return Err(ReplayError::VersionMismatch);
    }
    if manifest.context_schema_version != AI_DECISION_CONTEXT_SCHEMA_VERSION_V1
        || manifest.ai_trading_plan_schema_version != AI_TRADING_PLAN_SCHEMA_VERSION_V3
    {
        return Err(ReplayError::AiAuthorityVersionMismatch);
    }
    if manifest.deterministic_seed != REPLAY_DETERMINISTIC_SEED_V1 {
        return Err(ReplayError::DeterministicSeedMismatch);
    }
    if manifest.dataset_hash != dataset.dataset_hash()
        || manifest.manifest_hash != hash_manifest(manifest)
    {
        return Err(ReplayError::ContentHashMismatch);
    }
    let dataset_instruments: Vec<&InstrumentId> = dataset
        .instruments()
        .iter()
        .map(ReplayInstrumentData::instrument_id)
        .collect();
    if manifest.instruments.len() != dataset_instruments.len()
        || manifest
            .instruments
            .iter()
            .zip(dataset_instruments)
            .any(|(manifest_id, dataset_id)| manifest_id != dataset_id)
    {
        return Err(ReplayError::InstrumentMismatch);
    }
    Ok(())
}

fn available_candles(candles: &[ClosedCandle], now_unix_millis: u64) -> &[ClosedCandle] {
    let end = candles.partition_point(|candle| candle.close_at_unix_millis() <= now_unix_millis);
    &candles[..end]
}

fn available_book(books: &[TopOfBook], now_unix_millis: u64) -> Option<&TopOfBook> {
    let end = books.partition_point(|book| book.observed_at_unix_millis() <= now_unix_millis);
    end.checked_sub(1).map(|index| &books[index])
}

fn hash_dataset(instruments: &[ReplayInstrumentData]) -> ReplayHash {
    let mut hasher = ReplayHasher::new("market-replay-dataset-v1");
    hasher.usize(instruments.len());
    for data in instruments {
        hasher.field(&data.instrument_id.to_string());
        hash_candles(&mut hasher, &data.primary_candles);
        hash_candles(&mut hasher, &data.confirmation_candles);
        hasher.usize(data.books.len());
        for book in &data.books {
            hasher.u64(book.source_generated_at_unix_millis());
            hasher.u64(book.observed_at_unix_millis());
            for value in [
                book.bid_price(),
                book.bid_quantity(),
                book.ask_price(),
                book.ask_quantity(),
            ] {
                hasher.decimal(value);
            }
        }
    }
    hasher.finish()
}

fn hash_candles(hasher: &mut ReplayHasher, candles: &[ClosedCandle]) {
    hasher.usize(candles.len());
    for candle in candles {
        hasher.field(match candle.timeframe() {
            MarketTimeframe::FifteenMinutes => "15m",
            MarketTimeframe::OneHour => "1h",
        });
        hasher.u64(candle.open_at_unix_millis());
        hasher.u64(candle.close_at_unix_millis());
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

fn hash_manifest(manifest: &ReplayManifest) -> ReplayHash {
    let mut hasher = ReplayHasher::new("market-replay-manifest-v2");
    hasher.field(manifest.schema_version);
    hasher.field(manifest.feature_version);
    hasher.field(manifest.context_schema_version);
    hasher.field(manifest.ai_trading_plan_schema_version);
    hasher.u64(manifest.deterministic_seed);
    hasher.u64(manifest.start_unix_millis);
    hasher.u64(manifest.end_unix_millis);
    hasher.u64(manifest.step_millis);
    hasher.usize(manifest.instruments.len());
    for instrument_id in &manifest.instruments {
        hasher.field(&instrument_id.to_string());
    }
    hasher.bytes(manifest.dataset_hash.as_bytes());
    hasher.finish()
}

fn hash_report(manifest: &ReplayManifest, records: &[ReplayRecord]) -> ReplayHash {
    let mut hasher = ReplayHasher::new("market-replay-report-output-v2");
    hasher.bytes(manifest.manifest_hash.as_bytes());
    hasher.field(manifest.context_schema_version);
    hasher.field(manifest.ai_trading_plan_schema_version);
    hasher.u64(manifest.deterministic_seed);
    hasher.usize(records.len());
    for record in records {
        hasher.u64(record.as_of_unix_millis);
        hasher.field(&record.instrument_id.to_string());
        hasher.u64(record.latest_primary_close_at_unix_millis);
        hasher.u64(record.latest_confirmation_close_at_unix_millis);
        hasher.bytes(record.snapshot_hash.as_bytes());
        match &record.eligibility {
            ReplayEligibilityOutcome::Event { event_hash, kinds } => {
                hasher.field("event");
                hasher.bytes(event_hash.as_bytes());
                hasher.usize(kinds.len());
                for kind in kinds {
                    hasher.field(event_kind_name(*kind));
                }
            }
            ReplayEligibilityOutcome::Rejected(reasons) => {
                hasher.field("rejected");
                hasher.usize(reasons.len());
                for reason in reasons {
                    hasher.field(rejection_reason_name(*reason));
                }
            }
        }
    }
    hasher.finish()
}

fn write_eligibility_json(output: &mut String, outcome: &ReplayEligibilityOutcome) {
    match outcome {
        ReplayEligibilityOutcome::Event { event_hash, kinds } => {
            write!(
                output,
                "{{\"status\":\"event\",\"event_hash\":\"{event_hash}\",\"kinds\":["
            )
            .expect("writing to String cannot fail");
            for (index, kind) in kinds.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write!(output, "\"{}\"", event_kind_name(*kind))
                    .expect("writing to String cannot fail");
            }
            output.push_str("]}");
        }
        ReplayEligibilityOutcome::Rejected(reasons) => {
            output.push_str("{\"status\":\"rejected\",\"reasons\":[");
            for (index, reason) in reasons.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write!(output, "\"{}\"", rejection_reason_name(*reason))
                    .expect("writing to String cannot fail");
            }
            output.push_str("]}");
        }
    }
}

const fn event_kind_name(kind: EligibilityEventKind) -> &'static str {
    match kind {
        EligibilityEventKind::StructureChanged => "structure_changed",
        EligibilityEventKind::KeyLocationReached => "key_location_reached",
        EligibilityEventKind::VolatilityExpanded => "volatility_expanded",
        EligibilityEventKind::VolumeAnomaly => "volume_anomaly",
        EligibilityEventKind::BreakoutAttempt => "breakout_attempt",
        EligibilityEventKind::RetestEvent => "retest_event",
        EligibilityEventKind::PositionReviewDue => "position_review_due",
        EligibilityEventKind::InvalidationRiskIncreased => "invalidation_risk_increased",
    }
}

const fn rejection_reason_name(reason: PrefilterRejectionReason) -> &'static str {
    match reason {
        PrefilterRejectionReason::SnapshotExpired => "snapshot_expired",
        PrefilterRejectionReason::SystemDisallowsNewAi => "system_disallows_new_ai",
        PrefilterRejectionReason::InstrumentDisabled => "instrument_disabled",
        PrefilterRejectionReason::ActiveTradePlanNotDue => "active_trade_plan_not_due",
        PrefilterRejectionReason::InsufficientLiquidity => "insufficient_liquidity",
        PrefilterRejectionReason::SpreadTooWide => "spread_too_wide",
        PrefilterRejectionReason::NoInformationDelta => "no_information_delta",
        PrefilterRejectionReason::DuplicateEvent => "duplicate_event",
        PrefilterRejectionReason::CooldownActive => "cooldown_active",
        PrefilterRejectionReason::LlmConcurrencyExhausted => "llm_concurrency_exhausted",
        PrefilterRejectionReason::DailyCallBudgetExhausted => "daily_call_budget_exhausted",
        PrefilterRejectionReason::DailyTokenBudgetExhausted => "daily_token_budget_exhausted",
        PrefilterRejectionReason::DailyCostBudgetExhausted => "daily_cost_budget_exhausted",
    }
}

struct ReplayHasher(Sha256);

impl ReplayHasher {
    fn new(schema: &str) -> Self {
        let mut hasher = Self(Sha256::new());
        hasher.field(schema);
        hasher
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

    fn usize(&mut self, value: usize) {
        self.field(&value.to_string());
    }

    fn bytes(&mut self, value: &[u8]) {
        self.0.update(value.len().to_be_bytes());
        self.0.update(value);
    }

    fn finish(self) -> ReplayHash {
        ReplayHash(self.0.finalize().into())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayError {
    InvalidClock,
    InstrumentCountOutOfRange,
    DuplicateInstrument,
    InstrumentMismatch,
    TimeframeMismatch,
    MarketDataOutOfOrder,
    MissingMarketData,
    FutureMarketData,
    VersionMismatch,
    AiAuthorityVersionMismatch,
    DeterministicSeedMismatch,
    ContentHashMismatch,
    MarketFeature(MarketFeatureError),
}

impl fmt::Display for ReplayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidClock => formatter.write_str("replay clock range or alignment is invalid"),
            Self::InstrumentCountOutOfRange => {
                formatter.write_str("replay dataset must contain one to three instruments")
            }
            Self::DuplicateInstrument => {
                formatter.write_str("replay dataset contains a duplicate instrument")
            }
            Self::InstrumentMismatch => {
                formatter.write_str("replay manifest or market data instrument does not match")
            }
            Self::TimeframeMismatch => {
                formatter.write_str("replay candle timeframe does not match its series")
            }
            Self::MarketDataOutOfOrder => {
                formatter.write_str("replay market data is not strictly ordered and contiguous")
            }
            Self::MissingMarketData => {
                formatter.write_str("replay market data is missing at the requested clock instant")
            }
            Self::FutureMarketData => {
                formatter.write_str("replay selected market data from after the clock instant")
            }
            Self::VersionMismatch => {
                formatter.write_str("replay schema or feature version mismatch")
            }
            Self::AiAuthorityVersionMismatch => {
                formatter.write_str("replay AI Context or AITradingPlan version mismatch")
            }
            Self::DeterministicSeedMismatch => {
                formatter.write_str("replay deterministic seed mismatch")
            }
            Self::ContentHashMismatch => {
                formatter.write_str("replay manifest or dataset content hash mismatch")
            }
            Self::MarketFeature(error) => {
                write!(formatter, "replay feature evaluation failed: {error}")
            }
        }
    }
}

impl std::error::Error for ReplayError {}

#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use serde_json::Value;

    use super::*;
    use crate::FEATURE_CANDLE_WINDOW;

    const HOUR: u64 = 60 * 60 * 1_000;
    const QUARTER_HOUR: u64 = 15 * 60 * 1_000;

    fn instrument(symbol: &str) -> InstrumentId {
        InstrumentId::from_str(&format!("bybit:spot:{symbol}"))
            .expect("test instrument must be valid")
    }

    fn decimal(mantissa: i128, scale: u32) -> DomainDecimal {
        DomainDecimal::from_mantissa_scale(mantissa, scale).expect("test decimal must be valid")
    }

    fn candle(
        instrument_id: &InstrumentId,
        timeframe: MarketTimeframe,
        index: u64,
    ) -> ClosedCandle {
        let base = i128::from(10_000 + index % 17);
        ClosedCandle::new(
            instrument_id.clone(),
            timeframe,
            index * timeframe.duration_millis(),
            decimal(base, 2),
            decimal(base + 20, 2),
            decimal(base - 20, 2),
            decimal(base + 5, 2),
            decimal(100 + i128::from(index % 5), 0),
            decimal(20_000, 0),
            true,
        )
        .expect("test candle must be valid")
    }

    fn dataset_for(symbol: &str, end_unix_millis: u64) -> ReplayDataset {
        let instrument_id = instrument(symbol);
        let primary_count = end_unix_millis / QUARTER_HOUR;
        let confirmation_count = end_unix_millis / HOUR;
        let primary = (0..primary_count)
            .map(|index| candle(&instrument_id, MarketTimeframe::FifteenMinutes, index))
            .collect();
        let confirmation = (0..confirmation_count)
            .map(|index| candle(&instrument_id, MarketTimeframe::OneHour, index))
            .collect();
        let books = (480..=primary_count)
            .map(|close_index| {
                let observed_at = close_index * QUARTER_HOUR;
                TopOfBook::new(
                    instrument_id.clone(),
                    observed_at,
                    observed_at,
                    decimal(10_000, 2),
                    decimal(10, 0),
                    decimal(10_001, 2),
                    decimal(10, 0),
                )
                .expect("test book must be valid")
            })
            .collect();
        ReplayDataset::new(vec![
            ReplayInstrumentData::new(instrument_id, primary, confirmation, books)
                .expect("test instrument data must be valid"),
        ])
        .expect("test dataset must be valid")
    }

    #[test]
    fn replay_clock_is_inclusive_and_strictly_aligned() {
        let ticks: Vec<u64> = ReplayClock::new(QUARTER_HOUR, 3 * QUARTER_HOUR, QUARTER_HOUR)
            .expect("clock must be valid")
            .collect();
        assert_eq!(
            ticks,
            vec![QUARTER_HOUR, 2 * QUARTER_HOUR, 3 * QUARTER_HOUR]
        );
        assert_eq!(
            ReplayClock::new(1, QUARTER_HOUR, QUARTER_HOUR),
            Err(ReplayError::InvalidClock)
        );
    }

    #[test]
    fn same_manifest_replays_to_identical_output_hash_and_json() {
        let start = 120 * HOUR;
        let end = start + HOUR;
        let dataset = dataset_for("BTCUSDT", end);
        let manifest = ReplayManifest::new(&dataset, start, end).expect("manifest must be valid");

        let first = ReplayRunner::run(&manifest, &dataset).expect("first replay must succeed");
        let second = ReplayRunner::run(&manifest, &dataset).expect("second replay must succeed");

        assert_eq!(first.output_hash(), second.output_hash());
        assert_eq!(first, second);
        assert_eq!(first.to_json(), second.to_json());
        assert_eq!(first.records().len(), 5);
        assert!(first.event_count() >= 1);
    }

    #[test]
    fn replay_is_bound_to_v3_context_plan_versions_and_fixed_seed() {
        let start = 120 * HOUR;
        let dataset = dataset_for("BTCUSDT", start);
        let manifest = ReplayManifest::new(&dataset, start, start).expect("manifest must be valid");
        let report = ReplayRunner::run(&manifest, &dataset).expect("replay must succeed");

        assert_eq!(
            manifest.context_schema_version(),
            AI_DECISION_CONTEXT_SCHEMA_VERSION_V1
        );
        assert_eq!(
            report.context_schema_version(),
            AI_DECISION_CONTEXT_SCHEMA_VERSION_V1
        );
        assert_eq!(
            manifest.ai_trading_plan_schema_version(),
            AI_TRADING_PLAN_SCHEMA_VERSION_V3
        );
        assert_eq!(
            report.ai_trading_plan_schema_version(),
            AI_TRADING_PLAN_SCHEMA_VERSION_V3
        );
        assert_eq!(manifest.deterministic_seed(), REPLAY_DETERMINISTIC_SEED_V1);
        assert_eq!(report.deterministic_seed(), REPLAY_DETERMINISTIC_SEED_V1);
    }

    #[test]
    fn replay_never_selects_candles_or_books_after_the_clock_instant() {
        let start = 120 * HOUR;
        let dataset = dataset_for("BTCUSDT", start + HOUR);
        let manifest = ReplayManifest::new(&dataset, start, start).expect("manifest must be valid");
        let report = ReplayRunner::run(&manifest, &dataset).expect("replay must succeed");

        let record = &report.records()[0];
        assert_eq!(record.as_of_unix_millis(), start);
        assert_eq!(record.latest_primary_close_at_unix_millis(), start);
        assert_eq!(record.latest_confirmation_close_at_unix_millis(), start);
        assert!(
            dataset.instruments()[0]
                .primary_candles()
                .iter()
                .any(|candle| candle.close_at_unix_millis() > start)
        );
        assert!(
            dataset.instruments()[0]
                .books()
                .iter()
                .any(|book| book.observed_at_unix_millis() > start)
        );
    }

    #[test]
    fn manifest_rejects_a_different_dataset_hash() {
        let start = 120 * HOUR;
        let first_dataset = dataset_for("BTCUSDT", start);
        let second_dataset = dataset_for("ETHUSDT", start);
        let manifest =
            ReplayManifest::new(&first_dataset, start, start).expect("manifest must be valid");

        assert_eq!(
            ReplayRunner::run(&manifest, &second_dataset),
            Err(ReplayError::ContentHashMismatch)
        );
    }

    #[test]
    fn manifest_and_report_are_valid_json_without_external_context_or_performance_claims() {
        let start = 120 * HOUR;
        let dataset = dataset_for("BTCUSDT", start);
        let manifest = ReplayManifest::new(&dataset, start, start).expect("manifest must be valid");
        let report = ReplayRunner::run(&manifest, &dataset).expect("replay must succeed");
        let manifest_json = manifest.to_json();
        let report_json = report.to_json();

        serde_json::from_str::<Value>(&manifest_json).expect("manifest JSON must parse");
        serde_json::from_str::<Value>(&report_json).expect("report JSON must parse");
        for forbidden in ["news", "pnl", "profit", "return", "alpha"] {
            assert!(!manifest_json.to_ascii_lowercase().contains(forbidden));
            assert!(!report_json.to_ascii_lowercase().contains(forbidden));
        }
    }

    #[test]
    fn dataset_hash_is_order_independent_across_instruments_after_canonical_sorting() {
        let end = 120 * HOUR;
        let btc = dataset_for("BTCUSDT", end).instruments.remove(0);
        let eth = dataset_for("ETHUSDT", end).instruments.remove(0);
        let first =
            ReplayDataset::new(vec![btc.clone(), eth.clone()]).expect("dataset must be valid");
        let second = ReplayDataset::new(vec![eth, btc]).expect("dataset must be valid");

        assert_eq!(first.dataset_hash(), second.dataset_hash());
        assert_eq!(first.instruments(), second.instruments());
    }

    #[test]
    fn dataset_bounds_and_series_order_fail_closed() {
        assert_eq!(
            ReplayDataset::new(Vec::new()),
            Err(ReplayError::InstrumentCountOutOfRange)
        );
        let instrument_id = instrument("BTCUSDT");
        let reversed = vec![
            candle(&instrument_id, MarketTimeframe::FifteenMinutes, 1),
            candle(&instrument_id, MarketTimeframe::FifteenMinutes, 0),
        ];
        assert_eq!(
            ReplayInstrumentData::new(
                instrument_id.clone(),
                reversed,
                vec![candle(&instrument_id, MarketTimeframe::OneHour, 0)],
                vec![
                    TopOfBook::new(
                        instrument_id,
                        QUARTER_HOUR,
                        QUARTER_HOUR,
                        decimal(10_000, 2),
                        decimal(1, 0),
                        decimal(10_001, 2),
                        decimal(1, 0),
                    )
                    .expect("test book must be valid")
                ]
            ),
            Err(ReplayError::MarketDataOutOfOrder)
        );
    }

    #[test]
    fn warmup_is_enforced_by_the_same_market_feature_engine() {
        let instrument_id = instrument("BTCUSDT");
        let primary = (0..FEATURE_CANDLE_WINDOW as u64 - 1)
            .map(|index| candle(&instrument_id, MarketTimeframe::FifteenMinutes, index))
            .collect();
        let confirmation = (0..FEATURE_CANDLE_WINDOW as u64)
            .map(|index| candle(&instrument_id, MarketTimeframe::OneHour, index))
            .collect();
        let start = (FEATURE_CANDLE_WINDOW as u64 - 1) * QUARTER_HOUR;
        let book = TopOfBook::new(
            instrument_id.clone(),
            start,
            start,
            decimal(10_000, 2),
            decimal(1, 0),
            decimal(10_001, 2),
            decimal(1, 0),
        )
        .expect("test book must be valid");
        let dataset = ReplayDataset::new(vec![
            ReplayInstrumentData::new(instrument_id, primary, confirmation, vec![book])
                .expect("dataset input must be valid"),
        ])
        .expect("dataset must be valid");
        let manifest = ReplayManifest::new(&dataset, start, start).expect("manifest must be valid");

        assert_eq!(
            ReplayRunner::run(&manifest, &dataset),
            Err(ReplayError::MarketFeature(
                MarketFeatureError::InsufficientWarmup
            ))
        );
    }
}
