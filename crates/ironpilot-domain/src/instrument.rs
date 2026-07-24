use core::fmt;
use core::str::FromStr;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MIN_SYMBOL_LENGTH: usize = 3;
const MAX_SYMBOL_LENGTH: usize = 32;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Exchange {
    Bybit,
}

impl Exchange {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Bybit => "bybit",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum InstrumentType {
    Spot,
    LinearPerpetual,
}

impl InstrumentType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Spot => "spot",
            Self::LinearPerpetual => "linear_perpetual",
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Symbol(Box<str>);

impl Symbol {
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, ValidationError> {
        let value = value.into();
        let length = value.len();

        if !(MIN_SYMBOL_LENGTH..=MAX_SYMBOL_LENGTH).contains(&length) {
            return Err(ValidationError::SymbolLength { length });
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            return Err(ValidationError::SymbolCharacters);
        }

        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InstrumentId {
    exchange: Exchange,
    instrument_type: InstrumentType,
    symbol: Symbol,
}

impl InstrumentId {
    #[must_use]
    pub const fn exchange(&self) -> Exchange {
        self.exchange
    }

    #[must_use]
    pub const fn instrument_type(&self) -> InstrumentType {
        self.instrument_type
    }

    #[must_use]
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }
}

impl fmt::Display for InstrumentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}:{}",
            self.exchange.as_str(),
            self.instrument_type.as_str(),
            self.symbol
        )
    }
}

impl FromStr for InstrumentId {
    type Err = ParseInstrumentIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split(':');
        let exchange = match parts.next() {
            Some("bybit") => Exchange::Bybit,
            Some(_) => return Err(ParseInstrumentIdError::UnknownExchange),
            None => return Err(ParseInstrumentIdError::InvalidShape),
        };
        let instrument_type = match parts.next() {
            Some("spot") => InstrumentType::Spot,
            Some("linear_perpetual") => InstrumentType::LinearPerpetual,
            Some(_) => return Err(ParseInstrumentIdError::UnknownInstrumentType),
            None => return Err(ParseInstrumentIdError::InvalidShape),
        };
        let symbol = parts.next().ok_or(ParseInstrumentIdError::InvalidShape)?;
        if parts.next().is_some() {
            return Err(ParseInstrumentIdError::InvalidShape);
        }

        Ok(Self {
            exchange,
            instrument_type,
            symbol: Symbol::new(symbol).map_err(ParseInstrumentIdError::InvalidSymbol)?,
        })
    }
}

impl Serialize for InstrumentId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for InstrumentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationError {
    SymbolLength { length: usize },
    SymbolCharacters,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SymbolLength { length } => write!(
                formatter,
                "symbol length {length} is outside {MIN_SYMBOL_LENGTH}..={MAX_SYMBOL_LENGTH}"
            ),
            Self::SymbolCharacters => {
                formatter.write_str("symbol must contain only uppercase ASCII letters and digits")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseInstrumentIdError {
    InvalidShape,
    UnknownExchange,
    UnknownInstrumentType,
    InvalidSymbol(ValidationError),
}

impl fmt::Display for ParseInstrumentIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidShape => formatter.write_str("instrument ID must be exchange:type:symbol"),
            Self::UnknownExchange => formatter.write_str("unknown exchange"),
            Self::UnknownInstrumentType => formatter.write_str("unknown instrument type"),
            Self::InvalidSymbol(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ParseInstrumentIdError {}
