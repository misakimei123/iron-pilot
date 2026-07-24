use core::fmt;
use core::str::FromStr;

use rust_decimal::Decimal;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Maximum scale supported by [`rust_decimal::Decimal`].
pub const MAX_DECIMAL_SCALE: u32 = 28;

/// Exact base-10 domain value.
///
/// The only wire representation is a string. JSON floating-point numbers are
/// deliberately rejected so amounts can never enter through binary floating point.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DomainDecimal(Decimal);

impl DomainDecimal {
    pub const ZERO: Self = Self(Decimal::ZERO);

    pub fn from_mantissa_scale(
        mantissa: i128,
        scale: u32,
    ) -> Result<Self, ParseDomainDecimalError> {
        if scale > MAX_DECIMAL_SCALE {
            return Err(ParseDomainDecimalError::ScaleOutOfRange { scale });
        }

        Decimal::try_from_i128_with_scale(mantissa, scale)
            .map(Self)
            .map_err(|_| ParseDomainDecimalError::MagnitudeOutOfRange)
    }

    #[must_use]
    pub const fn as_decimal(self) -> Decimal {
        self.0
    }
}

impl fmt::Display for DomainDecimal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for DomainDecimal {
    type Err = ParseDomainDecimalError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Decimal::from_str_exact(value)
            .map(Self)
            .map_err(|_| ParseDomainDecimalError::InvalidSyntax)
    }
}

impl Serialize for DomainDecimal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for DomainDecimal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseDomainDecimalError {
    InvalidSyntax,
    MagnitudeOutOfRange,
    ScaleOutOfRange { scale: u32 },
}

impl fmt::Display for ParseDomainDecimalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSyntax => formatter.write_str("invalid exact decimal syntax"),
            Self::MagnitudeOutOfRange => {
                formatter.write_str("decimal magnitude exceeds the domain representation")
            }
            Self::ScaleOutOfRange { scale } => {
                write!(
                    formatter,
                    "decimal scale {scale} exceeds {MAX_DECIMAL_SCALE}"
                )
            }
        }
    }
}

impl std::error::Error for ParseDomainDecimalError {}
