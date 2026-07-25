use core::fmt;

use crate::{DomainDecimal, InstrumentId};

const MAX_ASSET_CODE_LENGTH: usize = 32;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AssetCode(Box<str>);

impl AssetCode {
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, MarketMetadataValidationError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_ASSET_CODE_LENGTH
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            return Err(MarketMetadataValidationError::InvalidAssetCode);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AssetCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum InstrumentTradingStatus {
    Trading,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotInstrumentRules {
    instrument_id: InstrumentId,
    base_asset: AssetCode,
    quote_asset: AssetCode,
    trading_status: InstrumentTradingStatus,
    price_tick: DomainDecimal,
    base_precision: DomainDecimal,
    quote_precision: DomainDecimal,
    minimum_order_amount: DomainDecimal,
    maximum_limit_order_quantity: DomainDecimal,
    maximum_market_order_quantity: DomainDecimal,
    maximum_post_only_order_quantity: DomainDecimal,
    price_limit_ratio_x: DomainDecimal,
    price_limit_ratio_y: DomainDecimal,
}

#[allow(clippy::too_many_arguments)]
pub fn validated_spot_instrument_rules(
    instrument_id: InstrumentId,
    base_asset: AssetCode,
    quote_asset: AssetCode,
    trading_status: InstrumentTradingStatus,
    price_tick: DomainDecimal,
    base_precision: DomainDecimal,
    quote_precision: DomainDecimal,
    minimum_order_amount: DomainDecimal,
    maximum_limit_order_quantity: DomainDecimal,
    maximum_market_order_quantity: DomainDecimal,
    maximum_post_only_order_quantity: DomainDecimal,
    price_limit_ratio_x: DomainDecimal,
    price_limit_ratio_y: DomainDecimal,
) -> Result<SpotInstrumentRules, MarketMetadataValidationError> {
    if base_asset == quote_asset {
        return Err(MarketMetadataValidationError::IdenticalAssets);
    }
    for (field, value) in [
        ("price_tick", price_tick),
        ("base_precision", base_precision),
        ("quote_precision", quote_precision),
        ("minimum_order_amount", minimum_order_amount),
        ("maximum_limit_order_quantity", maximum_limit_order_quantity),
        (
            "maximum_market_order_quantity",
            maximum_market_order_quantity,
        ),
        (
            "maximum_post_only_order_quantity",
            maximum_post_only_order_quantity,
        ),
        ("price_limit_ratio_x", price_limit_ratio_x),
        ("price_limit_ratio_y", price_limit_ratio_y),
    ] {
        if value <= DomainDecimal::ZERO {
            return Err(MarketMetadataValidationError::NonPositiveDecimal { field });
        }
    }

    Ok(SpotInstrumentRules {
        instrument_id,
        base_asset,
        quote_asset,
        trading_status,
        price_tick,
        base_precision,
        quote_precision,
        minimum_order_amount,
        maximum_limit_order_quantity,
        maximum_market_order_quantity,
        maximum_post_only_order_quantity,
        price_limit_ratio_x,
        price_limit_ratio_y,
    })
}

impl SpotInstrumentRules {
    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub fn base_asset(&self) -> &AssetCode {
        &self.base_asset
    }

    #[must_use]
    pub fn quote_asset(&self) -> &AssetCode {
        &self.quote_asset
    }

    #[must_use]
    pub const fn trading_status(&self) -> InstrumentTradingStatus {
        self.trading_status
    }

    #[must_use]
    pub const fn price_tick(&self) -> DomainDecimal {
        self.price_tick
    }

    #[must_use]
    pub const fn base_precision(&self) -> DomainDecimal {
        self.base_precision
    }

    #[must_use]
    pub const fn quote_precision(&self) -> DomainDecimal {
        self.quote_precision
    }

    #[must_use]
    pub const fn minimum_order_amount(&self) -> DomainDecimal {
        self.minimum_order_amount
    }

    #[must_use]
    pub const fn maximum_limit_order_quantity(&self) -> DomainDecimal {
        self.maximum_limit_order_quantity
    }

    #[must_use]
    pub const fn maximum_market_order_quantity(&self) -> DomainDecimal {
        self.maximum_market_order_quantity
    }

    #[must_use]
    pub const fn maximum_post_only_order_quantity(&self) -> DomainDecimal {
        self.maximum_post_only_order_quantity
    }

    #[must_use]
    pub const fn price_limit_ratio_x(&self) -> DomainDecimal {
        self.price_limit_ratio_x
    }

    #[must_use]
    pub const fn price_limit_ratio_y(&self) -> DomainDecimal {
        self.price_limit_ratio_y
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RulesHash([u8; 32]);

impl RulesHash {
    #[must_use]
    pub const fn from_sha256(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for RulesHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExchangeServerTime {
    unix_seconds: u64,
    unix_nanos: u64,
    response_unix_millis: u64,
}

impl ExchangeServerTime {
    pub fn new(
        unix_seconds: u64,
        unix_nanos: u64,
        response_unix_millis: u64,
    ) -> Result<Self, MarketMetadataValidationError> {
        if unix_nanos / 1_000_000_000 != unix_seconds
            || response_unix_millis / 1_000 != unix_seconds
        {
            return Err(MarketMetadataValidationError::InconsistentServerTime);
        }
        Ok(Self {
            unix_seconds,
            unix_nanos,
            response_unix_millis,
        })
    }

    #[must_use]
    pub const fn unix_seconds(self) -> u64 {
        self.unix_seconds
    }

    #[must_use]
    pub const fn unix_nanos(self) -> u64 {
        self.unix_nanos
    }

    #[must_use]
    pub const fn response_unix_millis(self) -> u64 {
        self.response_unix_millis
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstrumentRulesSnapshot {
    rules: Vec<SpotInstrumentRules>,
    server_time: ExchangeServerTime,
    observed_at_unix_millis: u64,
    valid_until_unix_millis: u64,
    rules_hash: RulesHash,
}

impl InstrumentRulesSnapshot {
    pub fn new(
        mut rules: Vec<SpotInstrumentRules>,
        server_time: ExchangeServerTime,
        observed_at_unix_millis: u64,
        valid_until_unix_millis: u64,
        rules_hash: RulesHash,
    ) -> Result<Self, MarketMetadataValidationError> {
        if rules.is_empty() {
            return Err(MarketMetadataValidationError::EmptyRules);
        }
        if valid_until_unix_millis <= observed_at_unix_millis {
            return Err(MarketMetadataValidationError::InvalidValidityWindow);
        }

        rules.sort_by(|left, right| left.instrument_id.cmp(&right.instrument_id));
        if rules
            .windows(2)
            .any(|pair| pair[0].instrument_id == pair[1].instrument_id)
        {
            return Err(MarketMetadataValidationError::DuplicateInstrument);
        }

        Ok(Self {
            rules,
            server_time,
            observed_at_unix_millis,
            valid_until_unix_millis,
            rules_hash,
        })
    }

    #[must_use]
    pub fn rules(&self) -> &[SpotInstrumentRules] {
        &self.rules
    }

    #[must_use]
    pub const fn server_time(&self) -> ExchangeServerTime {
        self.server_time
    }

    #[must_use]
    pub const fn observed_at_unix_millis(&self) -> u64 {
        self.observed_at_unix_millis
    }

    #[must_use]
    pub const fn valid_until_unix_millis(&self) -> u64 {
        self.valid_until_unix_millis
    }

    #[must_use]
    pub const fn rules_hash(&self) -> RulesHash {
        self.rules_hash
    }

    #[must_use]
    pub const fn is_expired_at(&self, unix_millis: u64) -> bool {
        unix_millis >= self.valid_until_unix_millis
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarketMetadataValidationError {
    InvalidAssetCode,
    IdenticalAssets,
    NonPositiveDecimal { field: &'static str },
    InconsistentServerTime,
    EmptyRules,
    InvalidValidityWindow,
    DuplicateInstrument,
}

impl fmt::Display for MarketMetadataValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAssetCode => formatter.write_str("invalid asset code"),
            Self::IdenticalAssets => formatter.write_str("base and quote assets must differ"),
            Self::NonPositiveDecimal { field } => {
                write!(formatter, "{field} must be positive")
            }
            Self::InconsistentServerTime => {
                formatter.write_str("server timestamp fields are inconsistent")
            }
            Self::EmptyRules => formatter.write_str("instrument rules cannot be empty"),
            Self::InvalidValidityWindow => {
                formatter.write_str("instrument rules validity window must be positive")
            }
            Self::DuplicateInstrument => {
                formatter.write_str("instrument rules contain a duplicate instrument")
            }
        }
    }
}

impl std::error::Error for MarketMetadataValidationError {}
