use core::fmt;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ironpilot_domain::{
    AssetCode, DomainDecimal, Exchange, ExchangeServerTime, InstrumentId, InstrumentRulesSnapshot,
    InstrumentTradingStatus, InstrumentType, RulesHash, SpotInstrumentRules,
    validated_spot_instrument_rules,
};
use reqwest::{Client, StatusCode, Url, redirect};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const BYBIT_MAINNET_PUBLIC_REST_URL: &str = "https://api.bybit.com/";
pub const DEFAULT_INSTRUMENT_RULES_TTL: Duration = Duration::from_secs(6 * 60 * 60);
pub const MAX_INSTRUMENT_RULES_TTL: Duration = Duration::from_secs(24 * 60 * 60);
pub const INSTRUMENT_RULES_HASH_SCHEMA_V1: &str = "ironpilot-bybit-spot-rules-v1";

const MIN_INSTRUMENT_RULES_TTL: Duration = Duration::from_secs(1);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug)]
pub struct BybitPublicRestClient {
    client: Client,
    base_url: Url,
}

impl BybitPublicRestClient {
    pub fn mainnet() -> Result<Self, BybitPublicRestError> {
        Self::with_base_url(BYBIT_MAINNET_PUBLIC_REST_URL)
    }

    pub fn with_base_url(base_url: &str) -> Result<Self, BybitPublicRestError> {
        let base_url = Url::parse(base_url).map_err(|error| {
            BybitPublicRestError::new(
                PublicRestErrorKind::InvalidConfiguration,
                format!("invalid Bybit public REST base URL: {error}"),
            )
        })?;
        if base_url.scheme() != "https"
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.path() != "/"
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(BybitPublicRestError::new(
                PublicRestErrorKind::InvalidConfiguration,
                "Bybit public REST base URL must be an HTTPS origin",
            ));
        }

        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(redirect::Policy::none())
            .user_agent(concat!("ironpilot/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| {
                BybitPublicRestError::new(
                    PublicRestErrorKind::InvalidConfiguration,
                    format!("cannot construct Bybit public REST client: {error}"),
                )
            })?;

        Ok(Self { client, base_url })
    }

    pub async fn fetch_server_time(&self) -> Result<ExchangeServerTime, BybitPublicRestError> {
        let body = self.get("/v5/market/time", &[]).await?;
        decode_server_time(&body)
    }

    pub async fn fetch_spot_instrument_rules(
        &self,
        instrument_ids: &[InstrumentId],
    ) -> Result<InstrumentRulesSnapshot, BybitPublicRestError> {
        self.fetch_spot_instrument_rules_with_ttl(instrument_ids, DEFAULT_INSTRUMENT_RULES_TTL)
            .await
    }

    pub async fn fetch_spot_instrument_rules_with_ttl(
        &self,
        instrument_ids: &[InstrumentId],
        ttl: Duration,
    ) -> Result<InstrumentRulesSnapshot, BybitPublicRestError> {
        validate_request(instrument_ids, ttl)?;

        let server_time = self.fetch_server_time().await?;
        let mut rules = Vec::with_capacity(instrument_ids.len());
        for instrument_id in instrument_ids {
            let body = self
                .get(
                    "/v5/market/instruments-info",
                    &[
                        ("category", "spot"),
                        ("symbol", instrument_id.symbol().as_str()),
                    ],
                )
                .await?;
            rules.push(decode_spot_instrument(&body, instrument_id)?);
        }

        let observed_at_unix_millis = current_unix_millis()?;
        build_rules_snapshot(rules, server_time, observed_at_unix_millis, ttl)
    }

    async fn get(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<Vec<u8>, BybitPublicRestError> {
        let url = self.base_url.join(path).map_err(|error| {
            BybitPublicRestError::new(
                PublicRestErrorKind::InvalidConfiguration,
                format!("cannot construct Bybit public REST endpoint: {error}"),
            )
        })?;
        let mut response = self
            .client
            .get(url)
            .query(query)
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        let status = response.status();

        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(BybitPublicRestError::with_http_status(
                PublicRestErrorKind::ContractViolation,
                status,
                "Bybit public REST response exceeds the bounded body limit",
            ));
        }

        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(classify_reqwest_error)? {
            if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(BybitPublicRestError::with_http_status(
                    PublicRestErrorKind::ContractViolation,
                    status,
                    "Bybit public REST response exceeds the bounded body limit",
                ));
            }
            body.extend_from_slice(&chunk);
        }

        if !status.is_success() {
            return Err(classify_http_status(status));
        }
        Ok(body)
    }
}

fn build_rules_snapshot(
    rules: Vec<SpotInstrumentRules>,
    server_time: ExchangeServerTime,
    observed_at_unix_millis: u64,
    ttl: Duration,
) -> Result<InstrumentRulesSnapshot, BybitPublicRestError> {
    let ttl_millis = u64::try_from(ttl.as_millis()).map_err(|_| {
        BybitPublicRestError::new(
            PublicRestErrorKind::InvalidRequest,
            "instrument rules TTL cannot be represented in milliseconds",
        )
    })?;
    let valid_until_unix_millis =
        observed_at_unix_millis
            .checked_add(ttl_millis)
            .ok_or_else(|| {
                BybitPublicRestError::new(
                    PublicRestErrorKind::Clock,
                    "instrument rules validity timestamp overflowed",
                )
            })?;
    let rules_hash = hash_rules(&rules);

    InstrumentRulesSnapshot::new(
        rules,
        server_time,
        observed_at_unix_millis,
        valid_until_unix_millis,
        rules_hash,
    )
    .map_err(contract_error)
}

fn validate_request(
    instrument_ids: &[InstrumentId],
    ttl: Duration,
) -> Result<(), BybitPublicRestError> {
    if !(1..=3).contains(&instrument_ids.len()) {
        return Err(BybitPublicRestError::new(
            PublicRestErrorKind::InvalidRequest,
            "Bybit Spot metadata request must contain 1..=3 instruments",
        ));
    }
    if !(MIN_INSTRUMENT_RULES_TTL..=MAX_INSTRUMENT_RULES_TTL).contains(&ttl) {
        return Err(BybitPublicRestError::new(
            PublicRestErrorKind::InvalidRequest,
            "instrument rules TTL must be within 1 second and 24 hours",
        ));
    }
    for instrument_id in instrument_ids {
        if instrument_id.exchange() != Exchange::Bybit
            || instrument_id.instrument_type() != InstrumentType::Spot
        {
            return Err(BybitPublicRestError::new(
                PublicRestErrorKind::InvalidRequest,
                format!("unsupported public metadata instrument {instrument_id}"),
            ));
        }
    }
    let mut sorted = instrument_ids.to_vec();
    sorted.sort();
    if sorted.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(BybitPublicRestError::new(
            PublicRestErrorKind::InvalidRequest,
            "Bybit Spot metadata request contains a duplicate instrument",
        ));
    }
    Ok(())
}

fn current_unix_millis() -> Result<u64, BybitPublicRestError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| {
            BybitPublicRestError::new(
                PublicRestErrorKind::Clock,
                "system clock is before the Unix epoch",
            )
        })?
        .as_millis();
    u64::try_from(millis).map_err(|_| {
        BybitPublicRestError::new(
            PublicRestErrorKind::Clock,
            "system clock cannot be represented in milliseconds",
        )
    })
}

fn decode_server_time(body: &[u8]) -> Result<ExchangeServerTime, BybitPublicRestError> {
    let envelope: SuccessfulEnvelope<ServerTimeResult> = decode_success(body)?;
    let unix_seconds = parse_u64("result.timeSecond", &envelope.result.time_second)?;
    let unix_nanos = parse_u64("result.timeNano", &envelope.result.time_nano)?;
    ExchangeServerTime::new(unix_seconds, unix_nanos, envelope.time).map_err(contract_error)
}

fn decode_spot_instrument(
    body: &[u8],
    expected_instrument_id: &InstrumentId,
) -> Result<SpotInstrumentRules, BybitPublicRestError> {
    let envelope: SuccessfulEnvelope<SpotInstrumentResult> = decode_success(body)?;
    let result = envelope.result;
    if result.category.as_ref() != "spot" {
        return Err(contract_message("Bybit instrument category is not Spot"));
    }
    if !result.next_page_cursor.is_empty() {
        return Err(contract_message(
            "Bybit Spot instruments response unexpectedly contains a pagination cursor",
        ));
    }
    if result.list.len() != 1 {
        return Err(contract_message(
            "Bybit Spot symbol query must return exactly one instrument",
        ));
    }

    let dto = result
        .list
        .into_iter()
        .next()
        .expect("length was checked above");
    if dto.symbol.as_ref() != expected_instrument_id.symbol().as_str() {
        return Err(contract_message(
            "Bybit Spot response symbol differs from the requested instrument",
        ));
    }
    let trading_status = match dto.status.as_ref() {
        "Trading" => InstrumentTradingStatus::Trading,
        _ => {
            return Err(contract_message(
                "Bybit Spot response contains an unsupported trading status",
            ));
        }
    };

    validated_spot_instrument_rules(
        expected_instrument_id.clone(),
        AssetCode::new(dto.base_coin).map_err(contract_error)?,
        AssetCode::new(dto.quote_coin).map_err(contract_error)?,
        trading_status,
        parse_decimal("priceFilter.tickSize", &dto.price_filter.tick_size)?,
        parse_decimal(
            "lotSizeFilter.basePrecision",
            &dto.lot_size_filter.base_precision,
        )?,
        parse_decimal(
            "lotSizeFilter.quotePrecision",
            &dto.lot_size_filter.quote_precision,
        )?,
        parse_decimal(
            "lotSizeFilter.minOrderAmt",
            &dto.lot_size_filter.minimum_order_amount,
        )?,
        parse_decimal(
            "lotSizeFilter.maxLimitOrderQty",
            &dto.lot_size_filter.maximum_limit_order_quantity,
        )?,
        parse_decimal(
            "lotSizeFilter.maxMarketOrderQty",
            &dto.lot_size_filter.maximum_market_order_quantity,
        )?,
        parse_decimal(
            "lotSizeFilter.postOnlyMaxLimitOrderSize",
            &dto.lot_size_filter.maximum_post_only_order_quantity,
        )?,
        parse_decimal(
            "riskParameters.priceLimitRatioX",
            &dto.risk_parameters.price_limit_ratio_x,
        )?,
        parse_decimal(
            "riskParameters.priceLimitRatioY",
            &dto.risk_parameters.price_limit_ratio_y,
        )?,
    )
    .map_err(contract_error)
}

fn decode_success<T>(body: &[u8]) -> Result<SuccessfulEnvelope<T>, BybitPublicRestError>
where
    T: for<'de> Deserialize<'de>,
{
    let envelope: WireEnvelope = serde_json::from_slice(body).map_err(|error| {
        BybitPublicRestError::new(
            PublicRestErrorKind::InvalidResponse,
            format!("cannot decode Bybit response envelope: {error}"),
        )
    })?;
    if envelope.ret_code != 0 {
        return Err(classify_ret_code(envelope.ret_code, &envelope.ret_msg));
    }
    let result = serde_json::from_value(envelope.result).map_err(|error| {
        BybitPublicRestError::new(
            PublicRestErrorKind::ContractViolation,
            format!("cannot decode successful Bybit result: {error}"),
        )
    })?;
    Ok(SuccessfulEnvelope {
        result,
        time: envelope.time,
    })
}

fn parse_u64(field: &'static str, value: &str) -> Result<u64, BybitPublicRestError> {
    value
        .parse()
        .map_err(|_| contract_message(format!("Bybit {field} is not an unsigned decimal integer")))
}

fn parse_decimal(field: &'static str, value: &str) -> Result<DomainDecimal, BybitPublicRestError> {
    DomainDecimal::from_str(value).map_err(|_| {
        contract_message(format!(
            "Bybit {field} is not an exact base-10 decimal string"
        ))
    })
}

fn hash_rules(rules: &[SpotInstrumentRules]) -> RulesHash {
    let mut ordered = rules.to_vec();
    ordered.sort_by(|left, right| left.instrument_id().cmp(right.instrument_id()));

    let mut hasher = Sha256::new();
    hash_text(&mut hasher, INSTRUMENT_RULES_HASH_SCHEMA_V1);
    for rule in ordered {
        hash_text(&mut hasher, &rule.instrument_id().to_string());
        hash_text(&mut hasher, rule.base_asset().as_str());
        hash_text(&mut hasher, rule.quote_asset().as_str());
        hash_text(
            &mut hasher,
            match rule.trading_status() {
                InstrumentTradingStatus::Trading => "trading",
            },
        );
        for value in [
            rule.price_tick(),
            rule.base_precision(),
            rule.quote_precision(),
            rule.minimum_order_amount(),
            rule.maximum_limit_order_quantity(),
            rule.maximum_market_order_quantity(),
            rule.maximum_post_only_order_quantity(),
            rule.price_limit_ratio_x(),
            rule.price_limit_ratio_y(),
        ] {
            hash_text(&mut hasher, &value.as_decimal().normalize().to_string());
        }
    }

    RulesHash::from_sha256(hasher.finalize().into())
}

fn hash_text(hasher: &mut Sha256, value: &str) {
    let length = u64::try_from(value.len()).expect("string length must fit u64");
    hasher.update(length.to_be_bytes());
    hasher.update(value.as_bytes());
}

fn classify_reqwest_error(error: reqwest::Error) -> BybitPublicRestError {
    let kind = if error.is_timeout() {
        PublicRestErrorKind::Timeout
    } else {
        PublicRestErrorKind::Transport
    };
    BybitPublicRestError::new(kind, format!("Bybit public REST request failed: {error}"))
}

fn classify_http_status(status: StatusCode) -> BybitPublicRestError {
    let kind = match status.as_u16() {
        408 => PublicRestErrorKind::Timeout,
        429 => PublicRestErrorKind::RateLimited,
        500..=599 => PublicRestErrorKind::RemoteUnavailable,
        401 | 403 => PublicRestErrorKind::AccessDenied,
        _ => PublicRestErrorKind::RemoteRejected,
    };
    BybitPublicRestError::with_http_status(
        kind,
        status,
        format!("Bybit public REST returned HTTP {}", status.as_u16()),
    )
}

fn classify_ret_code(ret_code: i64, ret_message: &str) -> BybitPublicRestError {
    let kind = match ret_code {
        429 | 10_006 => PublicRestErrorKind::RateLimited,
        10_000 | 10_016 => PublicRestErrorKind::RemoteUnavailable,
        10_009 | 10_024 => PublicRestErrorKind::AccessDenied,
        _ => PublicRestErrorKind::RemoteRejected,
    };
    BybitPublicRestError::with_ret_code(
        kind,
        ret_code,
        format!("Bybit public REST rejected the request: {ret_message}"),
    )
}

fn contract_error(error: impl fmt::Display) -> BybitPublicRestError {
    contract_message(format!("Bybit public REST contract violation: {error}"))
}

fn contract_message(message: impl Into<Box<str>>) -> BybitPublicRestError {
    BybitPublicRestError::new(PublicRestErrorKind::ContractViolation, message)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicRestErrorKind {
    InvalidConfiguration,
    InvalidRequest,
    Clock,
    Timeout,
    Transport,
    RateLimited,
    RemoteUnavailable,
    AccessDenied,
    RemoteRejected,
    InvalidResponse,
    ContractViolation,
}

impl PublicRestErrorKind {
    #[must_use]
    pub const fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Timeout | Self::Transport | Self::RateLimited | Self::RemoteUnavailable
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BybitPublicRestError {
    kind: PublicRestErrorKind,
    message: Box<str>,
    http_status: Option<u16>,
    ret_code: Option<i64>,
}

impl BybitPublicRestError {
    fn new(kind: PublicRestErrorKind, message: impl Into<Box<str>>) -> Self {
        Self {
            kind,
            message: message.into(),
            http_status: None,
            ret_code: None,
        }
    }

    fn with_http_status(
        kind: PublicRestErrorKind,
        status: StatusCode,
        message: impl Into<Box<str>>,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            http_status: Some(status.as_u16()),
            ret_code: None,
        }
    }

    fn with_ret_code(
        kind: PublicRestErrorKind,
        ret_code: i64,
        message: impl Into<Box<str>>,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            http_status: None,
            ret_code: Some(ret_code),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> PublicRestErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        self.kind.is_retryable()
    }

    #[must_use]
    pub const fn http_status(&self) -> Option<u16> {
        self.http_status
    }

    #[must_use]
    pub const fn ret_code(&self) -> Option<i64> {
        self.ret_code
    }
}

impl fmt::Display for BybitPublicRestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for BybitPublicRestError {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireEnvelope {
    ret_code: i64,
    ret_msg: Box<str>,
    result: Value,
    time: u64,
}

#[derive(Debug)]
struct SuccessfulEnvelope<T> {
    result: T,
    time: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerTimeResult {
    time_second: Box<str>,
    time_nano: Box<str>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotInstrumentResult {
    category: Box<str>,
    #[serde(default)]
    next_page_cursor: Box<str>,
    list: Vec<SpotInstrumentDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotInstrumentDto {
    symbol: Box<str>,
    base_coin: Box<str>,
    quote_coin: Box<str>,
    status: Box<str>,
    lot_size_filter: SpotLotSizeFilterDto,
    price_filter: SpotPriceFilterDto,
    risk_parameters: SpotRiskParametersDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotLotSizeFilterDto {
    base_precision: Box<str>,
    quote_precision: Box<str>,
    #[serde(rename = "minOrderAmt")]
    minimum_order_amount: Box<str>,
    #[serde(rename = "maxLimitOrderQty")]
    maximum_limit_order_quantity: Box<str>,
    #[serde(rename = "maxMarketOrderQty")]
    maximum_market_order_quantity: Box<str>,
    #[serde(rename = "postOnlyMaxLimitOrderSize")]
    maximum_post_only_order_quantity: Box<str>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotPriceFilterDto {
    tick_size: Box<str>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotRiskParametersDto {
    price_limit_ratio_x: Box<str>,
    price_limit_ratio_y: Box<str>,
}

#[cfg(test)]
mod tests {
    use core::str::FromStr;
    use std::time::Duration;

    use ironpilot_domain::{DomainDecimal, InstrumentId, InstrumentTradingStatus, RulesHash};

    use super::{
        BybitPublicRestClient, PublicRestErrorKind, build_rules_snapshot, decode_server_time,
        decode_spot_instrument, decode_success, hash_rules, validate_request,
    };

    const SERVER_TIME_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/bybit-server-time.json");
    const BTC_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/bybit-spot-btcusdt.json");
    const ETH_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/bybit-spot-ethusdt.json");
    const CURSOR_FIXTURE: &[u8] =
        include_bytes!("../tests/fixtures/bybit-spot-unexpected-cursor.json");
    const RATE_LIMIT_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/bybit-rate-limit.json");

    #[test]
    fn server_time_fixture_preserves_all_integer_precisions() {
        let server_time = decode_server_time(SERVER_TIME_FIXTURE).expect("valid fixture");

        assert_eq!(server_time.unix_seconds(), 1_688_639_403);
        assert_eq!(server_time.unix_nanos(), 1_688_639_403_423_213_947);
        assert_eq!(server_time.response_unix_millis(), 1_688_639_403_423);
    }

    #[test]
    fn spot_fixture_maps_to_domain_rules_without_floating_point() {
        let instrument_id = InstrumentId::from_str("bybit:spot:BTCUSDT").expect("valid ID");
        let rules = decode_spot_instrument(BTC_FIXTURE, &instrument_id).expect("valid fixture");

        assert_eq!(rules.instrument_id(), &instrument_id);
        assert_eq!(rules.base_asset().as_str(), "BTC");
        assert_eq!(rules.quote_asset().as_str(), "USDT");
        assert_eq!(rules.trading_status(), InstrumentTradingStatus::Trading);
        assert_eq!(
            rules.price_tick(),
            DomainDecimal::from_str("0.10000000").expect("exact decimal")
        );
        assert_eq!(
            rules.base_precision(),
            DomainDecimal::from_str("0.000001").expect("exact decimal")
        );
        assert_eq!(
            rules.quote_precision(),
            DomainDecimal::from_str("0.0000001").expect("exact decimal")
        );
        assert_eq!(
            rules.maximum_limit_order_quantity(),
            DomainDecimal::from_str("83.000000").expect("exact decimal")
        );
        assert_eq!(
            rules.maximum_market_order_quantity(),
            DomainDecimal::from_str("41.500000").expect("exact decimal")
        );
    }

    #[test]
    fn spot_pagination_cursor_is_a_contract_violation() {
        let instrument_id = InstrumentId::from_str("bybit:spot:BTCUSDT").expect("valid ID");
        let error =
            decode_spot_instrument(CURSOR_FIXTURE, &instrument_id).expect_err("cursor must fail");

        assert_eq!(error.kind(), PublicRestErrorKind::ContractViolation);
        assert!(error.to_string().contains("pagination cursor"));
    }

    #[test]
    fn rules_hash_is_order_independent_and_schema_stable() {
        let btc_id = InstrumentId::from_str("bybit:spot:BTCUSDT").expect("valid ID");
        let eth_id = InstrumentId::from_str("bybit:spot:ETHUSDT").expect("valid ID");
        let btc = decode_spot_instrument(BTC_FIXTURE, &btc_id).expect("valid fixture");
        let eth = decode_spot_instrument(ETH_FIXTURE, &eth_id).expect("valid fixture");

        let forward = hash_rules(&[btc.clone(), eth.clone()]);
        let reverse = hash_rules(&[eth, btc]);

        assert_eq!(forward, reverse);
        assert_eq!(
            forward.to_string(),
            "386b5dadc634e68d3b79efedb6bcd95b75fc867fdf2f0c57f78dd6f458e46ba7"
        );
        assert_ne!(forward, RulesHash::from_sha256([0; 32]));

        let normalized_fixture = String::from_utf8(BTC_FIXTURE.to_vec())
            .expect("fixture is UTF-8")
            .replace("0.10000000", "0.1")
            .replace("83.000000", "83")
            .replace("41.500000", "41.5")
            .replace("60000.000000", "60000")
            .replace("0.005000", "0.005")
            .replace("0.010000", "0.01");
        let normalized_btc =
            decode_spot_instrument(normalized_fixture.as_bytes(), &btc_id).expect("valid fixture");
        let scaled_btc =
            decode_spot_instrument(BTC_FIXTURE, &btc_id).expect("valid scaled fixture");
        assert_eq!(hash_rules(&[normalized_btc]), hash_rules(&[scaled_btc]));
    }

    #[test]
    fn rules_snapshot_ttl_has_an_explicit_expiry_boundary() {
        let instrument_id = InstrumentId::from_str("bybit:spot:BTCUSDT").expect("valid ID");
        let rules = decode_spot_instrument(BTC_FIXTURE, &instrument_id).expect("valid fixture");
        let server_time = decode_server_time(SERVER_TIME_FIXTURE).expect("valid fixture");

        let snapshot = build_rules_snapshot(
            vec![rules],
            server_time,
            1_700_000_000_000,
            Duration::from_secs(60),
        )
        .expect("valid snapshot");

        assert_eq!(snapshot.observed_at_unix_millis(), 1_700_000_000_000);
        assert_eq!(snapshot.valid_until_unix_millis(), 1_700_000_060_000);
        assert!(!snapshot.is_expired_at(1_700_000_059_999));
        assert!(snapshot.is_expired_at(1_700_000_060_000));
    }

    #[test]
    fn bybit_rate_limit_is_explicitly_retryable() {
        let error = decode_success::<serde_json::Value>(RATE_LIMIT_FIXTURE)
            .expect_err("rate limit fixture must fail");

        assert_eq!(error.kind(), PublicRestErrorKind::RateLimited);
        assert_eq!(error.ret_code(), Some(10_006));
        assert!(error.is_retryable());
    }

    #[test]
    fn request_contract_rejects_duplicates_and_out_of_range_ttl() {
        let instrument_id = InstrumentId::from_str("bybit:spot:BTCUSDT").expect("valid ID");

        let duplicate = validate_request(
            &[instrument_id.clone(), instrument_id.clone()],
            Duration::from_secs(60),
        )
        .expect_err("duplicates must fail");
        assert_eq!(duplicate.kind(), PublicRestErrorKind::InvalidRequest);

        let excessive_ttl =
            validate_request(&[instrument_id], Duration::from_secs(24 * 60 * 60 + 1))
                .expect_err("excessive TTL must fail");
        assert_eq!(excessive_ttl.kind(), PublicRestErrorKind::InvalidRequest);
    }

    #[test]
    fn base_url_must_be_an_https_origin() {
        let error = BybitPublicRestClient::with_base_url("http://api.bybit.com/")
            .expect_err("plaintext HTTP must fail");

        assert_eq!(error.kind(), PublicRestErrorKind::InvalidConfiguration);
    }
}
