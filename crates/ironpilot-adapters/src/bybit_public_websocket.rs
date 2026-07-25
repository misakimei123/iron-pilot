use core::fmt;
use core::str::FromStr;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use ironpilot_application::{
    BoundedQueueSender, QueueSendError, RuntimeEvent, ShutdownSignal, UnixMillis,
};
use ironpilot_domain::{CorrelationId, DomainDecimal, Exchange, InstrumentId, InstrumentType};
use reqwest::Url;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::{Instant, MissedTickBehavior, interval_at, sleep, sleep_until, timeout};
use tokio_tungstenite::connect_async_with_config;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

pub const BYBIT_MAINNET_SPOT_PUBLIC_WEBSOCKET_URL: &str = "wss://stream.bybit.com/v5/public/spot";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const SUBSCRIPTION_TIMEOUT: Duration = Duration::from_secs(10);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
const MAX_CONSECUTIVE_SESSION_FAILURES: u8 = 8;
const MAX_MESSAGE_BYTES: usize = 256 * 1024;
const MAX_FRAME_BYTES: usize = 64 * 1024;
const READ_BUFFER_BYTES: usize = 16 * 1024;
const WRITE_BUFFER_BYTES: usize = 8 * 1024;
const MAX_WRITE_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum KlineInterval {
    FifteenMinutes,
    OneHour,
}

impl KlineInterval {
    const fn bybit_value(self) -> &'static str {
        match self {
            Self::FifteenMinutes => "15",
            Self::OneHour => "60",
        }
    }

    const fn duration_millis(self) -> i64 {
        match self {
            Self::FifteenMinutes => 15 * 60 * 1_000,
            Self::OneHour => 60 * 60 * 1_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KlineUpdate {
    instrument_id: InstrumentId,
    interval: KlineInterval,
    source_generated_at: UnixMillis,
    start_at: UnixMillis,
    end_at: UnixMillis,
    last_trade_at: UnixMillis,
    open: DomainDecimal,
    high: DomainDecimal,
    low: DomainDecimal,
    close: DomainDecimal,
    volume: DomainDecimal,
    turnover: DomainDecimal,
    confirmed: bool,
}

impl KlineUpdate {
    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn interval(&self) -> KlineInterval {
        self.interval
    }

    #[must_use]
    pub const fn source_generated_at(&self) -> UnixMillis {
        self.source_generated_at
    }

    #[must_use]
    pub const fn start_at(&self) -> UnixMillis {
        self.start_at
    }

    #[must_use]
    pub const fn end_at(&self) -> UnixMillis {
        self.end_at
    }

    #[must_use]
    pub const fn last_trade_at(&self) -> UnixMillis {
        self.last_trade_at
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

    #[must_use]
    pub const fn confirmed(&self) -> bool {
        self.confirmed
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BestBookSnapshot {
    instrument_id: InstrumentId,
    source_generated_at: UnixMillis,
    matching_engine_at: UnixMillis,
    update_id: u64,
    cross_sequence: u64,
    bid_price: DomainDecimal,
    bid_quantity: DomainDecimal,
    ask_price: DomainDecimal,
    ask_quantity: DomainDecimal,
}

impl BestBookSnapshot {
    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn source_generated_at(&self) -> UnixMillis {
        self.source_generated_at
    }

    #[must_use]
    pub const fn matching_engine_at(&self) -> UnixMillis {
        self.matching_engine_at
    }

    #[must_use]
    pub const fn update_id(&self) -> u64 {
        self.update_id
    }

    #[must_use]
    pub const fn cross_sequence(&self) -> u64 {
        self.cross_sequence
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BybitMarketEvent {
    Kline(KlineUpdate),
    BestBook(BestBookSnapshot),
}

impl BybitMarketEvent {
    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        match self {
            Self::Kline(update) => update.instrument_id(),
            Self::BestBook(snapshot) => snapshot.instrument_id(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TopicFreshness {
    source_generated_at: UnixMillis,
    observed_at: UnixMillis,
}

impl TopicFreshness {
    #[must_use]
    pub const fn source_generated_at(self) -> UnixMillis {
        self.source_generated_at
    }

    #[must_use]
    pub const fn observed_at(self) -> UnixMillis {
        self.observed_at
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolFreshness {
    instrument_id: InstrumentId,
    kline_15: Option<TopicFreshness>,
    kline_60: Option<TopicFreshness>,
    best_book: Option<TopicFreshness>,
}

impl SymbolFreshness {
    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub const fn kline_15(&self) -> Option<TopicFreshness> {
        self.kline_15
    }

    #[must_use]
    pub const fn kline_60(&self) -> Option<TopicFreshness> {
        self.kline_60
    }

    #[must_use]
    pub const fn best_book(&self) -> Option<TopicFreshness> {
        self.best_book
    }

    #[must_use]
    pub fn is_fresh_at(&self, now: UnixMillis, maximum_age: Duration) -> bool {
        [self.kline_15, self.kline_60, self.best_book]
            .into_iter()
            .all(|freshness| {
                freshness.is_some_and(|freshness| {
                    timestamp_is_fresh(freshness.observed_at(), now, maximum_age)
                })
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedFreshnessSnapshot {
    symbols: Vec<SymbolFreshness>,
}

impl FeedFreshnessSnapshot {
    #[must_use]
    pub fn symbols(&self) -> &[SymbolFreshness] {
        &self.symbols
    }

    #[must_use]
    pub fn symbol(&self, instrument_id: &InstrumentId) -> Option<&SymbolFreshness> {
        self.symbols
            .iter()
            .find(|freshness| freshness.instrument_id() == instrument_id)
    }
}

#[derive(Clone, Debug)]
pub struct FeedFreshnessRegistry {
    state: Arc<Mutex<BTreeMap<InstrumentId, MutableSymbolFreshness>>>,
}

impl FeedFreshnessRegistry {
    fn new(instrument_ids: &[InstrumentId]) -> Self {
        Self {
            state: Arc::new(Mutex::new(
                instrument_ids
                    .iter()
                    .cloned()
                    .map(|instrument_id| (instrument_id, MutableSymbolFreshness::default()))
                    .collect(),
            )),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> FeedFreshnessSnapshot {
        let state = self.lock_state();
        FeedFreshnessSnapshot {
            symbols: state
                .iter()
                .map(|(instrument_id, freshness)| SymbolFreshness {
                    instrument_id: instrument_id.clone(),
                    kline_15: freshness.kline_15,
                    kline_60: freshness.kline_60,
                    best_book: freshness.best_book,
                })
                .collect(),
        }
    }

    fn record(&self, topic: &TopicSpec, source_generated_at: UnixMillis, observed_at: UnixMillis) {
        let mut state = self.lock_state();
        let Some(symbol) = state.get_mut(&topic.instrument_id) else {
            return;
        };
        let freshness = Some(TopicFreshness {
            source_generated_at,
            observed_at,
        });
        match topic.kind {
            TopicKind::Kline(KlineInterval::FifteenMinutes) => symbol.kline_15 = freshness,
            TopicKind::Kline(KlineInterval::OneHour) => symbol.kline_60 = freshness,
            TopicKind::BestBook => symbol.best_book = freshness,
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, BTreeMap<InstrumentId, MutableSymbolFreshness>> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct MutableSymbolFreshness {
    kline_15: Option<TopicFreshness>,
    kline_60: Option<TopicFreshness>,
    best_book: Option<TopicFreshness>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubscriptionPlan {
    instrument_ids: Vec<InstrumentId>,
    topics: Vec<Box<str>>,
    specs: BTreeMap<Box<str>, TopicSpec>,
}

impl SubscriptionPlan {
    pub fn for_spot_instruments(
        instrument_ids: &[InstrumentId],
    ) -> Result<Self, BybitMarketStreamError> {
        validate_spot_instruments(instrument_ids)?;
        let mut instrument_ids = instrument_ids.to_vec();
        instrument_ids.sort();

        let mut topics = Vec::with_capacity(instrument_ids.len() * 3);
        let mut specs = BTreeMap::new();
        for instrument_id in &instrument_ids {
            for kind in [
                TopicKind::Kline(KlineInterval::FifteenMinutes),
                TopicKind::Kline(KlineInterval::OneHour),
                TopicKind::BestBook,
            ] {
                let topic = topic_name(instrument_id, kind);
                specs.insert(
                    topic.clone(),
                    TopicSpec {
                        instrument_id: instrument_id.clone(),
                        kind,
                    },
                );
                topics.push(topic);
            }
        }

        Ok(Self {
            instrument_ids,
            topics,
            specs,
        })
    }

    #[must_use]
    pub fn instrument_ids(&self) -> &[InstrumentId] {
        &self.instrument_ids
    }

    #[must_use]
    pub fn topics(&self) -> &[Box<str>] {
        &self.topics
    }

    fn request_id(connection_attempt: u32) -> String {
        format!("ironpilot-sub-{connection_attempt}")
    }

    fn subscription_request(&self, connection_attempt: u32) -> String {
        json!({
            "req_id": Self::request_id(connection_attempt),
            "op": "subscribe",
            "args": self.topics,
        })
        .to_string()
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum TopicKind {
    Kline(KlineInterval),
    BestBook,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TopicSpec {
    instrument_id: InstrumentId,
    kind: TopicKind,
}

fn topic_name(instrument_id: &InstrumentId, kind: TopicKind) -> Box<str> {
    let symbol = instrument_id.symbol().as_str();
    match kind {
        TopicKind::Kline(interval) => format!("kline.{}.{symbol}", interval.bybit_value()).into(),
        TopicKind::BestBook => format!("orderbook.1.{symbol}").into(),
    }
}

pub struct BybitPublicWebSocketClient {
    endpoint: Box<str>,
    plan: SubscriptionPlan,
    protocol: ProtocolProcessor,
    router: EventRouter,
    freshness: FeedFreshnessRegistry,
}

impl BybitPublicWebSocketClient {
    pub fn mainnet(
        instrument_ids: &[InstrumentId],
        routes: Vec<(InstrumentId, BoundedQueueSender<BybitMarketEvent>)>,
        correlation_id: CorrelationId,
    ) -> Result<Self, BybitMarketStreamError> {
        Self::with_endpoint(
            BYBIT_MAINNET_SPOT_PUBLIC_WEBSOCKET_URL,
            instrument_ids,
            routes,
            correlation_id,
        )
    }

    pub fn with_endpoint(
        endpoint: &str,
        instrument_ids: &[InstrumentId],
        routes: Vec<(InstrumentId, BoundedQueueSender<BybitMarketEvent>)>,
        correlation_id: CorrelationId,
    ) -> Result<Self, BybitMarketStreamError> {
        validate_endpoint(endpoint)?;
        let plan = SubscriptionPlan::for_spot_instruments(instrument_ids)?;
        let freshness = FeedFreshnessRegistry::new(plan.instrument_ids());
        let router = EventRouter::new(plan.instrument_ids(), routes, correlation_id)?;
        let protocol = ProtocolProcessor::new(plan.clone(), freshness.clone());
        Ok(Self {
            endpoint: endpoint.into(),
            plan,
            protocol,
            router,
            freshness,
        })
    }

    #[must_use]
    pub const fn subscription_plan(&self) -> &SubscriptionPlan {
        &self.plan
    }

    #[must_use]
    pub fn freshness_registry(&self) -> FeedFreshnessRegistry {
        self.freshness.clone()
    }

    pub async fn run(mut self, mut shutdown: ShutdownSignal) -> Result<(), BybitMarketStreamError> {
        let mut connection_attempt = 0_u32;
        let mut consecutive_failures = 0_u8;

        loop {
            if shutdown.is_cancelled() {
                return Ok(());
            }
            connection_attempt = connection_attempt.saturating_add(1);
            match run_session(
                &self.endpoint,
                &self.plan,
                &mut self.protocol,
                &self.router,
                connection_attempt,
                &mut shutdown,
            )
            .await
            {
                SessionEnd::Shutdown => return Ok(()),
                SessionEnd::Failed {
                    error,
                    delivered_data,
                } => {
                    if !error.is_retryable() {
                        return Err(error);
                    }
                    consecutive_failures = if delivered_data {
                        1
                    } else {
                        consecutive_failures.saturating_add(1)
                    };
                    if consecutive_failures >= MAX_CONSECUTIVE_SESSION_FAILURES {
                        return Err(BybitMarketStreamError::new(
                            error.kind,
                            format!(
                                "Bybit public WebSocket failed {consecutive_failures} consecutive sessions: {error}"
                            ),
                        ));
                    }

                    let delay = reconnect_delay(consecutive_failures);
                    tokio::select! {
                        () = shutdown.cancelled() => return Ok(()),
                        () = sleep(delay) => {}
                    }
                }
            }
        }
    }
}

fn validate_endpoint(endpoint: &str) -> Result<(), BybitMarketStreamError> {
    let url = Url::parse(endpoint).map_err(|error| {
        BybitMarketStreamError::new(
            MarketStreamErrorKind::InvalidConfiguration,
            format!("invalid Bybit public WebSocket endpoint: {error}"),
        )
    })?;
    if url.scheme() != "wss"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/v5/public/spot"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(BybitMarketStreamError::new(
            MarketStreamErrorKind::InvalidConfiguration,
            "Bybit public WebSocket endpoint must be a credential-free WSS Spot endpoint",
        ));
    }
    Ok(())
}

fn validate_spot_instruments(
    instrument_ids: &[InstrumentId],
) -> Result<(), BybitMarketStreamError> {
    if !(1..=3).contains(&instrument_ids.len()) {
        return Err(BybitMarketStreamError::new(
            MarketStreamErrorKind::InvalidConfiguration,
            "Bybit public WebSocket requires 1..=3 instruments",
        ));
    }
    for instrument_id in instrument_ids {
        if instrument_id.exchange() != Exchange::Bybit
            || instrument_id.instrument_type() != InstrumentType::Spot
        {
            return Err(BybitMarketStreamError::new(
                MarketStreamErrorKind::InvalidConfiguration,
                format!("unsupported public WebSocket instrument {instrument_id}"),
            ));
        }
    }
    let mut ordered = instrument_ids.to_vec();
    ordered.sort();
    if ordered.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(BybitMarketStreamError::new(
            MarketStreamErrorKind::InvalidConfiguration,
            "Bybit public WebSocket instrument set contains a duplicate",
        ));
    }
    Ok(())
}

fn websocket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(READ_BUFFER_BYTES)
        .write_buffer_size(WRITE_BUFFER_BYTES)
        .max_write_buffer_size(MAX_WRITE_BUFFER_BYTES)
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES))
}

async fn run_session(
    endpoint: &str,
    plan: &SubscriptionPlan,
    protocol: &mut ProtocolProcessor,
    router: &EventRouter,
    connection_attempt: u32,
    shutdown: &mut ShutdownSignal,
) -> SessionEnd {
    let connection = match timeout(
        CONNECT_TIMEOUT,
        connect_async_with_config(endpoint, Some(websocket_config()), false),
    )
    .await
    {
        Ok(Ok((connection, _response))) => connection,
        Ok(Err(error)) => {
            return SessionEnd::failed(
                BybitMarketStreamError::new(
                    MarketStreamErrorKind::Connect,
                    format!("Bybit public WebSocket connection failed: {error}"),
                ),
                false,
            );
        }
        Err(_) => {
            return SessionEnd::failed(
                BybitMarketStreamError::new(
                    MarketStreamErrorKind::Timeout,
                    "Bybit public WebSocket connection timed out",
                ),
                false,
            );
        }
    };

    let (mut writer, mut reader) = connection.split();
    let expected_request_id = SubscriptionPlan::request_id(connection_attempt);
    if let Err(error) = writer
        .send(Message::text(plan.subscription_request(connection_attempt)))
        .await
    {
        return SessionEnd::failed(transport_error("subscription write", error), false);
    }

    let subscription_deadline = sleep_until(Instant::now() + SUBSCRIPTION_TIMEOUT);
    tokio::pin!(subscription_deadline);
    let mut heartbeat = interval_at(Instant::now() + HEARTBEAT_INTERVAL, HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut subscription_confirmed = false;
    let mut awaiting_pong = false;
    let mut delivered_data = false;

    loop {
        tokio::select! {
            () = shutdown.cancelled() => {
                let _ = writer.close().await;
                return SessionEnd::Shutdown;
            }
            () = &mut subscription_deadline, if !subscription_confirmed => {
                return SessionEnd::failed(
                    BybitMarketStreamError::new(
                        MarketStreamErrorKind::Timeout,
                        "Bybit public WebSocket subscription acknowledgement timed out",
                    ),
                    delivered_data,
                );
            }
            _ = heartbeat.tick() => {
                if awaiting_pong {
                    return SessionEnd::failed(
                        BybitMarketStreamError::new(
                            MarketStreamErrorKind::HeartbeatTimeout,
                            "Bybit public WebSocket heartbeat pong was not received",
                        ),
                        delivered_data,
                    );
                }
                if let Err(error) = writer
                    .send(Message::text(json!({"op": "ping"}).to_string()))
                    .await
                {
                    return SessionEnd::failed(
                        transport_error("heartbeat write", error),
                        delivered_data,
                    );
                }
                awaiting_pong = true;
            }
            message = reader.next() => {
                let message = match message {
                    Some(Ok(message)) => message,
                    Some(Err(error)) => {
                        return SessionEnd::failed(
                            transport_error("stream read", error),
                            delivered_data,
                        );
                    }
                    None => {
                        return SessionEnd::failed(
                            BybitMarketStreamError::new(
                                MarketStreamErrorKind::Transport,
                                "Bybit public WebSocket stream ended",
                            ),
                            delivered_data,
                        );
                    }
                };

                match message {
                    Message::Text(text) => {
                        let observed_at = match current_unix_millis() {
                            Ok(timestamp) => timestamp,
                            Err(error) => return SessionEnd::failed(error, delivered_data),
                        };
                        match protocol.handle_text(
                            text.as_str(),
                            observed_at,
                            &expected_request_id,
                        ) {
                            Ok(ProtocolResult::Subscribed) => subscription_confirmed = true,
                            Ok(ProtocolResult::Pong) => awaiting_pong = false,
                            Ok(ProtocolResult::Event(event)) => {
                                if let Err(error) = router.route(event) {
                                    return SessionEnd::failed(error, delivered_data);
                                }
                                delivered_data = true;
                            }
                            Ok(ProtocolResult::Duplicate | ProtocolResult::OutOfOrder) => {}
                            Err(error) => return SessionEnd::failed(error, delivered_data),
                        }
                    }
                    Message::Ping(payload) => {
                        if let Err(error) = writer.send(Message::Pong(payload)).await {
                            return SessionEnd::failed(
                                transport_error("pong write", error),
                                delivered_data,
                            );
                        }
                    }
                    Message::Pong(_) => awaiting_pong = false,
                    Message::Close(_) => {
                        return SessionEnd::failed(
                            BybitMarketStreamError::new(
                                MarketStreamErrorKind::Transport,
                                "Bybit public WebSocket closed the connection",
                            ),
                            delivered_data,
                        );
                    }
                    Message::Binary(_) | Message::Frame(_) => {
                        return SessionEnd::failed(
                            BybitMarketStreamError::new(
                                MarketStreamErrorKind::ContractViolation,
                                "Bybit public WebSocket sent a non-text data message",
                            ),
                            delivered_data,
                        );
                    }
                }
            }
        }
    }
}

fn reconnect_delay(consecutive_failures: u8) -> Duration {
    let exponent = u32::from(consecutive_failures.saturating_sub(1).min(5));
    Duration::from_secs((1_u64 << exponent).min(30))
}

enum SessionEnd {
    Shutdown,
    Failed {
        error: BybitMarketStreamError,
        delivered_data: bool,
    },
}

impl SessionEnd {
    fn failed(error: BybitMarketStreamError, delivered_data: bool) -> Self {
        Self::Failed {
            error,
            delivered_data,
        }
    }
}

struct EventRouter {
    routes: BTreeMap<InstrumentId, BoundedQueueSender<BybitMarketEvent>>,
    correlation_id: CorrelationId,
}

impl EventRouter {
    fn new(
        expected_instruments: &[InstrumentId],
        routes: Vec<(InstrumentId, BoundedQueueSender<BybitMarketEvent>)>,
        correlation_id: CorrelationId,
    ) -> Result<Self, BybitMarketStreamError> {
        let routes: BTreeMap<_, _> = routes.into_iter().collect();
        if routes.len() != expected_instruments.len()
            || expected_instruments
                .iter()
                .any(|instrument_id| !routes.contains_key(instrument_id))
        {
            return Err(BybitMarketStreamError::new(
                MarketStreamErrorKind::InvalidConfiguration,
                "market event routes must exactly match the subscription instruments",
            ));
        }
        Ok(Self {
            routes,
            correlation_id,
        })
    }

    fn route(&self, event: BybitMarketEvent) -> Result<(), BybitMarketStreamError> {
        let sender = self.routes.get(event.instrument_id()).ok_or_else(|| {
            BybitMarketStreamError::new(
                MarketStreamErrorKind::InvalidConfiguration,
                "market event has no bounded route",
            )
        })?;
        sender
            .try_send(RuntimeEvent::new(self.correlation_id, event))
            .map_err(|error| match error {
                QueueSendError::Full(_) => BybitMarketStreamError::new(
                    MarketStreamErrorKind::Backpressure,
                    "bounded market event queue is saturated",
                ),
                QueueSendError::Closed(_) => BybitMarketStreamError::new(
                    MarketStreamErrorKind::Backpressure,
                    "bounded market event queue is closed",
                ),
            })
    }
}

struct ProtocolProcessor {
    plan: SubscriptionPlan,
    freshness: FeedFreshnessRegistry,
    kline_cursors: BTreeMap<TopicSpec, KlineCursor>,
    book_cursors: BTreeMap<InstrumentId, u64>,
}

impl ProtocolProcessor {
    fn new(plan: SubscriptionPlan, freshness: FeedFreshnessRegistry) -> Self {
        Self {
            plan,
            freshness,
            kline_cursors: BTreeMap::new(),
            book_cursors: BTreeMap::new(),
        }
    }

    fn handle_text(
        &mut self,
        text: &str,
        observed_at: UnixMillis,
        expected_request_id: &str,
    ) -> Result<ProtocolResult, BybitMarketStreamError> {
        if text.len() > MAX_MESSAGE_BYTES {
            return Err(contract_error(
                "Bybit public WebSocket text exceeds the bounded message size",
            ));
        }
        let value: Value = serde_json::from_str(text).map_err(|error| {
            contract_error(format!(
                "cannot decode Bybit public WebSocket JSON: {error}"
            ))
        })?;

        if value.get("op").is_some() {
            return decode_command(value, expected_request_id);
        }
        let topic = value
            .get("topic")
            .and_then(Value::as_str)
            .ok_or_else(|| contract_error("Bybit public WebSocket message has no topic or op"))?;
        let spec =
            self.plan.specs.get(topic).cloned().ok_or_else(|| {
                contract_error("Bybit public WebSocket sent an unrequested topic")
            })?;

        match spec.kind {
            TopicKind::Kline(interval) => {
                let update = decode_kline(value, &spec.instrument_id, interval)?;
                let result = self.classify_kline(&spec, &update);
                if !matches!(result, ProtocolResult::OutOfOrder) {
                    self.freshness
                        .record(&spec, update.source_generated_at(), observed_at);
                }
                Ok(result)
            }
            TopicKind::BestBook => {
                let snapshot = decode_best_book(value, &spec.instrument_id)?;
                let result = self.classify_book(&snapshot);
                if !matches!(result, ProtocolResult::OutOfOrder) {
                    self.freshness
                        .record(&spec, snapshot.source_generated_at(), observed_at);
                }
                Ok(result)
            }
        }
    }

    fn classify_kline(&mut self, spec: &TopicSpec, update: &KlineUpdate) -> ProtocolResult {
        let cursor = KlineCursor {
            start_at: update.start_at(),
            last_trade_at: update.last_trade_at(),
            confirmed: update.confirmed(),
        };
        match self.kline_cursors.get(spec).copied() {
            None => {
                self.kline_cursors.insert(spec.clone(), cursor);
                ProtocolResult::Event(BybitMarketEvent::Kline(update.clone()))
            }
            Some(previous) if cursor.start_at > previous.start_at => {
                self.kline_cursors.insert(spec.clone(), cursor);
                ProtocolResult::Event(BybitMarketEvent::Kline(update.clone()))
            }
            Some(previous) if cursor.start_at < previous.start_at => ProtocolResult::OutOfOrder,
            Some(previous)
                if !previous.confirmed
                    && (cursor.last_trade_at > previous.last_trade_at
                        || (cursor.last_trade_at == previous.last_trade_at
                            && cursor.confirmed
                            && !previous.confirmed)) =>
            {
                self.kline_cursors.insert(spec.clone(), cursor);
                ProtocolResult::Event(BybitMarketEvent::Kline(update.clone()))
            }
            Some(previous) if cursor.last_trade_at < previous.last_trade_at => {
                ProtocolResult::OutOfOrder
            }
            Some(_) => ProtocolResult::Duplicate,
        }
    }

    fn classify_book(&mut self, snapshot: &BestBookSnapshot) -> ProtocolResult {
        let instrument_id = snapshot.instrument_id();
        let update_id = snapshot.update_id();
        match self.book_cursors.get(instrument_id).copied() {
            None => {
                self.book_cursors.insert(instrument_id.clone(), update_id);
                ProtocolResult::Event(BybitMarketEvent::BestBook(snapshot.clone()))
            }
            Some(previous) if update_id == 1 && previous != 1 => {
                self.book_cursors.insert(instrument_id.clone(), update_id);
                ProtocolResult::Event(BybitMarketEvent::BestBook(snapshot.clone()))
            }
            Some(previous) if update_id > previous => {
                self.book_cursors.insert(instrument_id.clone(), update_id);
                ProtocolResult::Event(BybitMarketEvent::BestBook(snapshot.clone()))
            }
            Some(previous) if update_id < previous => ProtocolResult::OutOfOrder,
            Some(_) => ProtocolResult::Duplicate,
        }
    }
}

#[derive(Clone, Copy)]
struct KlineCursor {
    start_at: UnixMillis,
    last_trade_at: UnixMillis,
    confirmed: bool,
}

#[derive(Debug)]
enum ProtocolResult {
    Subscribed,
    Pong,
    Event(BybitMarketEvent),
    Duplicate,
    OutOfOrder,
}

fn decode_command(
    value: Value,
    expected_request_id: &str,
) -> Result<ProtocolResult, BybitMarketStreamError> {
    let response: CommandResponse = serde_json::from_value(value)
        .map_err(|error| contract_error(format!("invalid Bybit command response: {error}")))?;
    match response.op.as_ref() {
        "subscribe" => {
            if response.req_id.as_deref() != Some(expected_request_id) {
                return Err(contract_error(
                    "Bybit subscription response request ID does not match",
                ));
            }
            if !response.success {
                return Err(BybitMarketStreamError::new(
                    MarketStreamErrorKind::SubscriptionRejected,
                    format!("Bybit rejected public subscriptions: {}", response.ret_msg),
                ));
            }
            Ok(ProtocolResult::Subscribed)
        }
        "ping" if response.success && response.ret_msg.as_ref() == "pong" => {
            Ok(ProtocolResult::Pong)
        }
        _ => Err(contract_error(
            "unsupported Bybit public WebSocket command response",
        )),
    }
}

fn decode_kline(
    value: Value,
    instrument_id: &InstrumentId,
    expected_interval: KlineInterval,
) -> Result<KlineUpdate, BybitMarketStreamError> {
    let envelope: KlineEnvelope = serde_json::from_value(value)
        .map_err(|error| contract_error(format!("invalid Bybit kline response: {error}")))?;
    if envelope.message_type.as_ref() != "snapshot" || envelope.data.len() != 1 {
        return Err(contract_error(
            "Bybit kline response must contain one snapshot",
        ));
    }
    let data = envelope
        .data
        .into_iter()
        .next()
        .expect("length was checked");
    if data.interval.as_ref() != expected_interval.bybit_value() {
        return Err(contract_error("Bybit kline interval differs from topic"));
    }

    let start_at = unix_millis("kline.start", data.start)?;
    let end_at = unix_millis("kline.end", data.end)?;
    let last_trade_at = unix_millis("kline.timestamp", data.timestamp)?;
    let expected_end = start_at
        .get()
        .checked_add(expected_interval.duration_millis() - 1);
    if expected_end != Some(end_at.get()) || last_trade_at < start_at || last_trade_at > end_at {
        return Err(contract_error("Bybit kline timestamps are inconsistent"));
    }

    let open = positive_decimal("kline.open", &data.open)?;
    let high = positive_decimal("kline.high", &data.high)?;
    let low = positive_decimal("kline.low", &data.low)?;
    let close = positive_decimal("kline.close", &data.close)?;
    if high < open || high < close || low > open || low > close || high < low {
        return Err(contract_error("Bybit kline OHLC values are inconsistent"));
    }
    let volume = non_negative_decimal("kline.volume", &data.volume)?;
    let turnover = non_negative_decimal("kline.turnover", &data.turnover)?;

    Ok(KlineUpdate {
        instrument_id: instrument_id.clone(),
        interval: expected_interval,
        source_generated_at: unix_millis("kline.ts", envelope.ts)?,
        start_at,
        end_at,
        last_trade_at,
        open,
        high,
        low,
        close,
        volume,
        turnover,
        confirmed: data.confirm,
    })
}

fn decode_best_book(
    value: Value,
    instrument_id: &InstrumentId,
) -> Result<BestBookSnapshot, BybitMarketStreamError> {
    let envelope: OrderBookEnvelope = serde_json::from_value(value)
        .map_err(|error| contract_error(format!("invalid Bybit orderbook response: {error}")))?;
    if envelope.message_type.as_ref() != "snapshot"
        || envelope.data.symbol.as_ref() != instrument_id.symbol().as_str()
        || envelope.data.bids.len() != 1
        || envelope.data.asks.len() != 1
    {
        return Err(contract_error(
            "Bybit level-1 orderbook must contain one bid and ask snapshot",
        ));
    }
    let bid = decode_book_level("orderbook.bid", &envelope.data.bids[0])?;
    let ask = decode_book_level("orderbook.ask", &envelope.data.asks[0])?;
    if bid.0 >= ask.0 {
        return Err(contract_error("Bybit level-1 orderbook is crossed"));
    }

    Ok(BestBookSnapshot {
        instrument_id: instrument_id.clone(),
        source_generated_at: unix_millis("orderbook.ts", envelope.ts)?,
        matching_engine_at: unix_millis("orderbook.cts", envelope.cts)?,
        update_id: envelope.data.update_id,
        cross_sequence: envelope.data.cross_sequence(),
        bid_price: bid.0,
        bid_quantity: bid.1,
        ask_price: ask.0,
        ask_quantity: ask.1,
    })
}

fn decode_book_level(
    field: &'static str,
    values: &[Box<str>],
) -> Result<(DomainDecimal, DomainDecimal), BybitMarketStreamError> {
    if values.len() != 2 {
        return Err(contract_error(format!(
            "Bybit {field} must contain price and quantity"
        )));
    }
    Ok((
        positive_decimal(field, &values[0])?,
        positive_decimal(field, &values[1])?,
    ))
}

fn positive_decimal(
    field: &'static str,
    value: &str,
) -> Result<DomainDecimal, BybitMarketStreamError> {
    let value = exact_decimal(field, value)?;
    if value <= DomainDecimal::ZERO {
        return Err(contract_error(format!("Bybit {field} must be positive")));
    }
    Ok(value)
}

fn non_negative_decimal(
    field: &'static str,
    value: &str,
) -> Result<DomainDecimal, BybitMarketStreamError> {
    let value = exact_decimal(field, value)?;
    if value < DomainDecimal::ZERO {
        return Err(contract_error(format!(
            "Bybit {field} must be non-negative"
        )));
    }
    Ok(value)
}

fn exact_decimal(
    field: &'static str,
    value: &str,
) -> Result<DomainDecimal, BybitMarketStreamError> {
    DomainDecimal::from_str(value)
        .map_err(|_| contract_error(format!("Bybit {field} is not an exact base-10 decimal")))
}

fn unix_millis(field: &'static str, value: u64) -> Result<UnixMillis, BybitMarketStreamError> {
    let value = i64::try_from(value)
        .map_err(|_| contract_error(format!("Bybit {field} exceeds i64 milliseconds")))?;
    UnixMillis::new(value)
        .map_err(|_| contract_error(format!("Bybit {field} is a negative timestamp")))
}

fn current_unix_millis() -> Result<UnixMillis, BybitMarketStreamError> {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| {
            BybitMarketStreamError::new(
                MarketStreamErrorKind::Clock,
                "system clock is before the Unix epoch",
            )
        })?;
    let millis = i64::try_from(duration.as_millis()).map_err(|_| {
        BybitMarketStreamError::new(
            MarketStreamErrorKind::Clock,
            "system clock cannot be represented in milliseconds",
        )
    })?;
    UnixMillis::new(millis).map_err(|error| {
        BybitMarketStreamError::new(
            MarketStreamErrorKind::Clock,
            format!("invalid system clock: {error}"),
        )
    })
}

fn timestamp_is_fresh(timestamp: UnixMillis, now: UnixMillis, maximum_age: Duration) -> bool {
    let Ok(maximum_age) = i64::try_from(maximum_age.as_millis()) else {
        return false;
    };
    now.get()
        .checked_sub(timestamp.get())
        .is_some_and(|age| (0..=maximum_age).contains(&age))
}

fn contract_error(message: impl Into<Box<str>>) -> BybitMarketStreamError {
    BybitMarketStreamError::new(MarketStreamErrorKind::ContractViolation, message)
}

fn transport_error(
    action: &str,
    error: tokio_tungstenite::tungstenite::Error,
) -> BybitMarketStreamError {
    BybitMarketStreamError::new(
        MarketStreamErrorKind::Transport,
        format!("Bybit public WebSocket {action} failed: {error}"),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarketStreamErrorKind {
    InvalidConfiguration,
    Clock,
    Connect,
    Timeout,
    HeartbeatTimeout,
    Transport,
    SubscriptionRejected,
    ContractViolation,
    Backpressure,
}

impl MarketStreamErrorKind {
    #[must_use]
    pub const fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Connect
                | Self::Timeout
                | Self::HeartbeatTimeout
                | Self::Transport
                | Self::ContractViolation
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BybitMarketStreamError {
    kind: MarketStreamErrorKind,
    message: Box<str>,
}

impl BybitMarketStreamError {
    fn new(kind: MarketStreamErrorKind, message: impl Into<Box<str>>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> MarketStreamErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        self.kind.is_retryable()
    }
}

impl fmt::Display for BybitMarketStreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for BybitMarketStreamError {}

#[derive(Deserialize)]
struct CommandResponse {
    success: bool,
    ret_msg: Box<str>,
    op: Box<str>,
    #[serde(default)]
    req_id: Option<Box<str>>,
}

#[derive(Deserialize)]
struct KlineEnvelope {
    #[serde(rename = "type")]
    message_type: Box<str>,
    ts: u64,
    data: Vec<KlineData>,
}

#[derive(Deserialize)]
struct KlineData {
    start: u64,
    end: u64,
    interval: Box<str>,
    open: Box<str>,
    close: Box<str>,
    high: Box<str>,
    low: Box<str>,
    volume: Box<str>,
    turnover: Box<str>,
    confirm: bool,
    timestamp: u64,
}

#[derive(Deserialize)]
struct OrderBookEnvelope {
    #[serde(rename = "type")]
    message_type: Box<str>,
    ts: u64,
    data: OrderBookData,
    cts: u64,
}

#[derive(Deserialize)]
struct OrderBookData {
    #[serde(rename = "s")]
    symbol: Box<str>,
    #[serde(rename = "b")]
    bids: Vec<Vec<Box<str>>>,
    #[serde(rename = "a")]
    asks: Vec<Vec<Box<str>>>,
    #[serde(rename = "u")]
    update_id: u64,
    seq: u64,
}

impl OrderBookData {
    const fn cross_sequence(&self) -> u64 {
        self.seq
    }
}

#[cfg(test)]
mod tests {
    use core::str::FromStr;
    use std::time::Duration;

    use futures_util::{SinkExt, StreamExt};
    use ironpilot_application::{
        BoundedQueueSender, DeploymentEnvironment, EnvironmentFingerprint, HealthIssue,
        HealthMonitor, RuntimeEvent, RuntimeSupervisor, StartupIdentity, UnixMillis,
    };
    use ironpilot_domain::{CorrelationId, DomainDecimal, InstrumentId};
    use serde_json::{Value, json};
    use tokio::net::TcpListener;
    use tokio::time::timeout;
    use tokio_tungstenite::accept_async;
    use tokio_tungstenite::tungstenite::Message;

    use crate::parse_and_validate_yaml;

    use super::{
        BYBIT_MAINNET_SPOT_PUBLIC_WEBSOCKET_URL, BybitMarketEvent, BybitPublicWebSocketClient,
        EventRouter, FeedFreshnessRegistry, KlineInterval, MarketStreamErrorKind,
        ProtocolProcessor, ProtocolResult, SubscriptionPlan, reconnect_delay,
    };

    const VALID_YAML: &str = include_str!("../../../config/ironpilot.example.yaml");
    const KLINE_FIXTURE: &str = include_str!("../tests/fixtures/bybit-ws-kline-15-btcusdt.json");
    const ORDERBOOK_FIXTURE: &str =
        include_str!("../tests/fixtures/bybit-ws-orderbook-1-btcusdt.json");

    fn instrument(value: &str) -> InstrumentId {
        InstrumentId::from_str(value).expect("valid instrument ID")
    }

    fn correlation_id() -> CorrelationId {
        CorrelationId::from_str("d8786832-20b6-4978-93dd-e00b1f24b793")
            .expect("valid correlation ID")
    }

    fn timestamp(value: i64) -> UnixMillis {
        UnixMillis::new(value).expect("valid timestamp")
    }

    fn validated_config() -> ironpilot_application::ValidatedRuntimeConfig {
        let identity = StartupIdentity::new(
            DeploymentEnvironment::Development,
            EnvironmentFingerprint::from_str("development-paper-local").expect("valid fingerprint"),
        );
        parse_and_validate_yaml(VALID_YAML, &identity).expect("valid fixture config")
    }

    fn processor() -> (ProtocolProcessor, FeedFreshnessRegistry, InstrumentId) {
        let instrument_id = instrument("bybit:spot:BTCUSDT");
        let plan = SubscriptionPlan::for_spot_instruments(std::slice::from_ref(&instrument_id))
            .expect("valid plan");
        let freshness = FeedFreshnessRegistry::new(plan.instrument_ids());
        (
            ProtocolProcessor::new(plan, freshness.clone()),
            freshness,
            instrument_id,
        )
    }

    #[test]
    fn subscription_set_is_deterministic_and_reused_for_reconnects() {
        let plan = SubscriptionPlan::for_spot_instruments(&[
            instrument("bybit:spot:SOLUSDT"),
            instrument("bybit:spot:BTCUSDT"),
            instrument("bybit:spot:ETHUSDT"),
        ])
        .expect("valid plan");

        assert_eq!(plan.topics().len(), 9);
        assert_eq!(
            plan.topics()
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<&str>>(),
            &[
                "kline.15.BTCUSDT",
                "kline.60.BTCUSDT",
                "orderbook.1.BTCUSDT",
                "kline.15.ETHUSDT",
                "kline.60.ETHUSDT",
                "orderbook.1.ETHUSDT",
                "kline.15.SOLUSDT",
                "kline.60.SOLUSDT",
                "orderbook.1.SOLUSDT",
            ]
        );

        let first: Value =
            serde_json::from_str(&plan.subscription_request(1)).expect("valid request");
        let reconnect: Value =
            serde_json::from_str(&plan.subscription_request(2)).expect("valid request");
        assert_eq!(first["args"], reconnect["args"]);
        assert_ne!(first["req_id"], reconnect["req_id"]);
        assert_eq!(first["op"], "subscribe");
    }

    #[test]
    fn kline_updates_are_exact_deduplicated_and_ordered() {
        let (mut processor, freshness, instrument_id) = processor();

        let first = processor
            .handle_text(KLINE_FIXTURE, timestamp(1_672_324_990_000), "unused")
            .expect("valid kline");
        let ProtocolResult::Event(BybitMarketEvent::Kline(first)) = first else {
            panic!("first kline must be delivered");
        };
        assert_eq!(first.instrument_id(), &instrument_id);
        assert_eq!(first.interval(), KlineInterval::FifteenMinutes);
        assert_eq!(
            first.open(),
            DomainDecimal::from_str("16649.5").expect("exact decimal")
        );
        assert!(!first.confirmed());

        assert!(matches!(
            processor
                .handle_text(KLINE_FIXTURE, timestamp(1_672_324_991_000), "unused")
                .expect("valid duplicate"),
            ProtocolResult::Duplicate
        ));

        let newer = KLINE_FIXTURE.replace("1672324988882", "1672324989999");
        assert!(matches!(
            processor
                .handle_text(&newer, timestamp(1_672_324_992_000), "unused")
                .expect("valid update"),
            ProtocolResult::Event(BybitMarketEvent::Kline(_))
        ));
        assert!(matches!(
            processor
                .handle_text(KLINE_FIXTURE, timestamp(1_672_324_993_000), "unused")
                .expect("valid older message"),
            ProtocolResult::OutOfOrder
        ));

        let snapshot = freshness.snapshot();
        assert_eq!(
            snapshot
                .symbol(&instrument_id)
                .expect("tracked symbol")
                .kline_15()
                .expect("kline freshness")
                .observed_at(),
            timestamp(1_672_324_992_000)
        );
    }

    #[test]
    fn level_one_orderbook_handles_keepalive_restart_and_out_of_order() {
        let (mut processor, freshness, instrument_id) = processor();

        let first = processor
            .handle_text(ORDERBOOK_FIXTURE, timestamp(1_672_304_485_000), "unused")
            .expect("valid orderbook");
        let ProtocolResult::Event(BybitMarketEvent::BestBook(first)) = first else {
            panic!("first orderbook must be delivered");
        };
        assert_eq!(first.update_id(), 18_521_288);
        assert!(first.bid_price() < first.ask_price());

        assert!(matches!(
            processor
                .handle_text(ORDERBOOK_FIXTURE, timestamp(1_672_304_486_000), "unused")
                .expect("valid keepalive"),
            ProtocolResult::Duplicate
        ));
        assert_eq!(
            freshness
                .snapshot()
                .symbol(&instrument_id)
                .expect("tracked symbol")
                .best_book()
                .expect("book freshness")
                .observed_at(),
            timestamp(1_672_304_486_000)
        );

        let older = ORDERBOOK_FIXTURE.replace("18521288", "18521287");
        assert!(matches!(
            processor
                .handle_text(&older, timestamp(1_672_304_487_000), "unused")
                .expect("valid older snapshot"),
            ProtocolResult::OutOfOrder
        ));

        let restart = ORDERBOOK_FIXTURE.replace("18521288", "1");
        assert!(matches!(
            processor
                .handle_text(&restart, timestamp(1_672_304_488_000), "unused")
                .expect("valid service restart"),
            ProtocolResult::Event(BybitMarketEvent::BestBook(_))
        ));
        assert!(matches!(
            processor
                .handle_text(&restart, timestamp(1_672_304_489_000), "unused")
                .expect("valid repeated restart snapshot"),
            ProtocolResult::Duplicate
        ));
    }

    #[test]
    fn every_required_topic_contributes_to_per_symbol_freshness() {
        let (mut processor, freshness, instrument_id) = processor();
        processor
            .handle_text(KLINE_FIXTURE, timestamp(1_672_324_990_000), "unused")
            .expect("valid 15m kline");

        let mut hourly: Value = serde_json::from_str(KLINE_FIXTURE).expect("valid JSON");
        hourly["topic"] = Value::String("kline.60.BTCUSDT".into());
        hourly["data"][0]["interval"] = Value::String("60".into());
        hourly["data"][0]["end"] = Value::from(1_672_328_399_999_u64);
        processor
            .handle_text(&hourly.to_string(), timestamp(1_672_324_990_000), "unused")
            .expect("valid 60m kline");
        processor
            .handle_text(ORDERBOOK_FIXTURE, timestamp(1_672_324_990_000), "unused")
            .expect("valid orderbook");

        let snapshot = freshness.snapshot();
        let symbol = snapshot.symbol(&instrument_id).expect("tracked symbol");
        assert!(symbol.kline_15().is_some());
        assert!(symbol.kline_60().is_some());
        assert!(symbol.best_book().is_some());
        assert!(symbol.is_fresh_at(timestamp(1_672_324_990_999), Duration::from_secs(1)));
        assert!(!symbol.is_fresh_at(timestamp(1_672_324_991_001), Duration::from_secs(1)));
    }

    #[test]
    fn command_contract_confirms_subscription_and_rejects_failures() {
        let (mut processor, _freshness, _instrument_id) = processor();
        let acknowledgement = r#"{
            "success": true,
            "ret_msg": "subscribe",
            "conn_id": "test",
            "req_id": "ironpilot-sub-1",
            "op": "subscribe"
        }"#;
        assert!(matches!(
            processor
                .handle_text(acknowledgement, timestamp(1), "ironpilot-sub-1")
                .expect("valid acknowledgement"),
            ProtocolResult::Subscribed
        ));

        let rejected = acknowledgement.replace("\"success\": true", "\"success\": false");
        let error = processor
            .handle_text(&rejected, timestamp(1), "ironpilot-sub-1")
            .expect_err("rejection must fail");
        assert_eq!(error.kind(), MarketStreamErrorKind::SubscriptionRejected);
        assert!(!error.is_retryable());
    }

    #[test]
    fn bounded_market_queue_saturation_is_visible_and_not_silently_dropped() {
        let config = validated_config();
        let health = HealthMonitor::new(config.runtime_limits());
        let (sender, _receiver) = BoundedQueueSender::market(config.queue_limits(), &health);
        let (mut processor, _freshness, instrument_id) = processor();
        let ProtocolResult::Event(event) = processor
            .handle_text(KLINE_FIXTURE, timestamp(1_672_324_990_000), "unused")
            .expect("valid kline")
        else {
            panic!("first kline must be an event");
        };

        for _ in 0..1_024 {
            sender
                .try_send(RuntimeEvent::new(correlation_id(), event.clone()))
                .expect("queue has configured capacity");
        }
        let router = EventRouter::new(
            std::slice::from_ref(&instrument_id),
            vec![(instrument_id.clone(), sender)],
            correlation_id(),
        )
        .expect("matching route");
        let error = router.route(event).expect_err("saturation must fail");

        assert_eq!(error.kind(), MarketStreamErrorKind::Backpressure);
        assert!(!error.is_retryable());
        assert!(
            health
                .snapshot(timestamp(1_672_324_990_000), Duration::from_secs(1))
                .issues()
                .contains(&HealthIssue::MarketQueueSaturated)
        );
    }

    #[test]
    fn configuration_is_spot_only_route_exact_and_reconnect_backoff_is_capped() {
        assert!(SubscriptionPlan::for_spot_instruments(&[]).is_err());
        assert!(
            SubscriptionPlan::for_spot_instruments(&[instrument("bybit:linear_perpetual:BTCUSDT")])
                .is_err()
        );

        let config = validated_config();
        let health = HealthMonitor::new(config.runtime_limits());
        let (sender, _receiver) = BoundedQueueSender::market(config.queue_limits(), &health);
        let btc = instrument("bybit:spot:BTCUSDT");
        let missing_route = BybitPublicWebSocketClient::mainnet(
            std::slice::from_ref(&btc),
            vec![(instrument("bybit:spot:ETHUSDT"), sender)],
            correlation_id(),
        )
        .err()
        .expect("routes must match");
        assert_eq!(
            missing_route.kind(),
            MarketStreamErrorKind::InvalidConfiguration
        );

        assert_eq!(reconnect_delay(1), Duration::from_secs(1));
        assert_eq!(reconnect_delay(6), Duration::from_secs(30));
        assert_eq!(reconnect_delay(u8::MAX), Duration::from_secs(30));
    }

    #[tokio::test]
    async fn reconnect_resubscribes_the_exact_set_and_delivers_after_recovery() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("local listener");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for connection_number in 1..=2 {
                let (stream, _) = listener.accept().await.expect("local connection");
                let mut websocket = accept_async(stream).await.expect("server handshake");
                let request = websocket
                    .next()
                    .await
                    .expect("subscription frame")
                    .expect("valid subscription frame");
                let Message::Text(request) = request else {
                    panic!("subscription must be text");
                };
                let request: Value =
                    serde_json::from_str(request.as_str()).expect("valid subscription JSON");
                requests.push(request.clone());

                if connection_number == 1 {
                    websocket.close(None).await.expect("close first session");
                    continue;
                }

                websocket
                    .send(Message::text(
                        json!({
                            "success": true,
                            "ret_msg": "subscribe",
                            "conn_id": "local-test",
                            "req_id": request["req_id"],
                            "op": "subscribe",
                        })
                        .to_string(),
                    ))
                    .await
                    .expect("subscription acknowledgement");
                websocket
                    .send(Message::text(KLINE_FIXTURE))
                    .await
                    .expect("market event");
                let _ = timeout(Duration::from_secs(5), websocket.next()).await;
            }
            requests
        });

        let config = validated_config();
        let health = HealthMonitor::new(config.runtime_limits());
        let (sender, mut receiver) = BoundedQueueSender::market(config.queue_limits(), &health);
        let btc = instrument("bybit:spot:BTCUSDT");
        let plan =
            SubscriptionPlan::for_spot_instruments(std::slice::from_ref(&btc)).expect("valid plan");
        let freshness = FeedFreshnessRegistry::new(plan.instrument_ids());
        let router = EventRouter::new(plan.instrument_ids(), vec![(btc, sender)], correlation_id())
            .expect("matching route");
        let client = BybitPublicWebSocketClient {
            endpoint: format!("ws://{address}/v5/public/spot").into(),
            protocol: ProtocolProcessor::new(plan.clone(), freshness.clone()),
            plan,
            router,
            freshness: freshness.clone(),
        };

        let mut supervisor = RuntimeSupervisor::new(
            core::num::NonZeroUsize::new(1).expect("one is non-zero"),
            health,
        );
        let shutdown = supervisor.shutdown_signal();
        let run = client.run(shutdown);
        tokio::pin!(run);
        let event = tokio::select! {
            result = &mut run => panic!("stream ended before recovery: {result:?}"),
            event = timeout(Duration::from_secs(5), receiver.recv()) => {
                event
                    .expect("recovery event timeout")
                    .expect("recovery queue closed")
            }
        };
        assert!(matches!(event.payload(), BybitMarketEvent::Kline(_)));

        let report = supervisor.shutdown(Duration::from_secs(1)).await;
        assert!(report.graceful());
        timeout(Duration::from_secs(1), &mut run)
            .await
            .expect("client shutdown timeout")
            .expect("client shutdown");

        let requests = server.await.expect("server task");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["args"], requests[1]["args"]);
        assert_ne!(requests[0]["req_id"], requests[1]["req_id"]);
        assert!(
            freshness
                .snapshot()
                .symbol(&instrument("bybit:spot:BTCUSDT"))
                .expect("tracked symbol")
                .kline_15()
                .is_some()
        );
    }

    #[tokio::test]
    #[ignore = "requires read-only access to the live Bybit public WebSocket"]
    async fn live_public_websocket_delivers_and_shuts_down_cleanly() {
        let config = validated_config();
        let health = HealthMonitor::new(config.runtime_limits());
        let (sender, mut receiver) = BoundedQueueSender::market(config.queue_limits(), &health);
        let btc = instrument("bybit:spot:BTCUSDT");
        let client = BybitPublicWebSocketClient::mainnet(
            std::slice::from_ref(&btc),
            vec![(btc.clone(), sender)],
            correlation_id(),
        )
        .expect("valid live client");
        assert_eq!(
            BYBIT_MAINNET_SPOT_PUBLIC_WEBSOCKET_URL,
            "wss://stream.bybit.com/v5/public/spot"
        );

        let mut supervisor = RuntimeSupervisor::new(
            core::num::NonZeroUsize::new(1).expect("one is non-zero"),
            health,
        );
        let shutdown = supervisor.shutdown_signal();
        let run = client.run(shutdown);
        tokio::pin!(run);
        let event = tokio::select! {
            result = &mut run => panic!("live stream ended before data: {result:?}"),
            event = timeout(Duration::from_secs(15), receiver.recv()) => {
                event
                    .expect("live event timeout")
                    .expect("live queue closed")
            }
        };
        assert_eq!(
            event.payload().instrument_id().to_string(),
            "bybit:spot:BTCUSDT"
        );

        let report = supervisor.shutdown(Duration::from_secs(5)).await;
        assert!(report.graceful());
        timeout(Duration::from_secs(5), &mut run)
            .await
            .expect("live stream shutdown timeout")
            .expect("live stream must stop cleanly");
    }
}
