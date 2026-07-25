use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use crate::{
    AssetCode, DomainDecimal, FillId, InstrumentId, ManagedLotId, OrderId, SpotInstrumentRules,
    TradePlanId,
};

pub const PORTFOLIO_SCHEMA_VERSION_V1: &str = "ironpilot-portfolio-v1";
pub const MAX_PORTFOLIO_ASSETS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PortfolioHash([u8; 32]);

impl PortfolioHash {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for PortfolioHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PortfolioFillSide {
    Buy,
    Sell,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortfolioFill {
    fill_id: FillId,
    order_id: OrderId,
    trade_plan_id: TradePlanId,
    managed_lot_id: Option<ManagedLotId>,
    instrument_id: InstrumentId,
    base_asset: AssetCode,
    quote_asset: AssetCode,
    side: PortfolioFillSide,
    base_quantity: DomainDecimal,
    quote_quantity: DomainDecimal,
    occurred_at_unix_millis: u64,
}

impl PortfolioFill {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fill_id: FillId,
        order_id: OrderId,
        trade_plan_id: TradePlanId,
        managed_lot_id: Option<ManagedLotId>,
        instrument_rules: &SpotInstrumentRules,
        side: PortfolioFillSide,
        base_quantity: DomainDecimal,
        quote_quantity: DomainDecimal,
        occurred_at_unix_millis: u64,
    ) -> Result<Self, PortfolioError> {
        if base_quantity <= DomainDecimal::ZERO || quote_quantity <= DomainDecimal::ZERO {
            return Err(PortfolioError::NonPositiveFillQuantity);
        }
        match (side, managed_lot_id) {
            (PortfolioFillSide::Buy, None) => return Err(PortfolioError::ManagedLotIdRequired),
            (PortfolioFillSide::Sell, Some(_)) => {
                return Err(PortfolioError::ManagedLotIdForbidden);
            }
            _ => {}
        }
        Ok(Self {
            fill_id,
            order_id,
            trade_plan_id,
            managed_lot_id,
            instrument_id: instrument_rules.instrument_id().clone(),
            base_asset: instrument_rules.base_asset().clone(),
            quote_asset: instrument_rules.quote_asset().clone(),
            side,
            base_quantity,
            quote_quantity,
            occurred_at_unix_millis,
        })
    }

    #[must_use]
    pub const fn fill_id(&self) -> FillId {
        self.fill_id
    }

    #[must_use]
    pub const fn order_id(&self) -> OrderId {
        self.order_id
    }

    #[must_use]
    pub const fn trade_plan_id(&self) -> TradePlanId {
        self.trade_plan_id
    }

    #[must_use]
    pub const fn managed_lot_id(&self) -> Option<ManagedLotId> {
        self.managed_lot_id
    }

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
    pub const fn side(&self) -> PortfolioFillSide {
        self.side
    }

    #[must_use]
    pub const fn base_quantity(&self) -> DomainDecimal {
        self.base_quantity
    }

    #[must_use]
    pub const fn quote_quantity(&self) -> DomainDecimal {
        self.quote_quantity
    }

    #[must_use]
    pub const fn occurred_at_unix_millis(&self) -> u64 {
        self.occurred_at_unix_millis
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedPosition {
    instrument_id: InstrumentId,
    base_asset: AssetCode,
    quantity: DomainDecimal,
}

impl ManagedPosition {
    pub fn new(
        instrument_id: InstrumentId,
        base_asset: AssetCode,
        quantity: DomainDecimal,
    ) -> Result<Self, PortfolioError> {
        if quantity < DomainDecimal::ZERO {
            return Err(PortfolioError::NegativeBalance);
        }
        Ok(Self {
            instrument_id,
            base_asset,
            quantity,
        })
    }

    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub fn base_asset(&self) -> &AssetCode {
        &self.base_asset
    }

    #[must_use]
    pub const fn quantity(&self) -> DomainDecimal {
        self.quantity
    }

    pub fn authorize_sell(
        &self,
        requested_quantity: DomainDecimal,
        exchange_available_quantity: DomainDecimal,
    ) -> Result<SellAuthorization, PortfolioError> {
        if requested_quantity <= DomainDecimal::ZERO {
            return Err(PortfolioError::NonPositiveSellQuantity);
        }
        if exchange_available_quantity < DomainDecimal::ZERO {
            return Err(PortfolioError::NegativeBalance);
        }
        if requested_quantity > self.quantity {
            return Err(PortfolioError::SellExceedsManagedQuantity);
        }
        if requested_quantity > exchange_available_quantity {
            return Err(PortfolioError::SellExceedsExchangeAvailable);
        }
        Ok(SellAuthorization {
            instrument_id: self.instrument_id.clone(),
            base_asset: self.base_asset.clone(),
            approved_quantity: requested_quantity,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SellAuthorization {
    instrument_id: InstrumentId,
    base_asset: AssetCode,
    approved_quantity: DomainDecimal,
}

impl SellAuthorization {
    #[must_use]
    pub fn instrument_id(&self) -> &InstrumentId {
        &self.instrument_id
    }

    #[must_use]
    pub fn base_asset(&self) -> &AssetCode {
        &self.base_asset
    }

    #[must_use]
    pub const fn approved_quantity(&self) -> DomainDecimal {
        self.approved_quantity
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExchangeAssetBalance {
    asset: AssetCode,
    available_quantity: DomainDecimal,
    locked_quantity: DomainDecimal,
    total_quantity: DomainDecimal,
}

impl ExchangeAssetBalance {
    pub fn new(
        asset: AssetCode,
        available_quantity: DomainDecimal,
        locked_quantity: DomainDecimal,
    ) -> Result<Self, PortfolioError> {
        if available_quantity < DomainDecimal::ZERO || locked_quantity < DomainDecimal::ZERO {
            return Err(PortfolioError::NegativeBalance);
        }
        let total_quantity = available_quantity
            .checked_add(locked_quantity)
            .ok_or(PortfolioError::ArithmeticOverflow)?;
        Ok(Self {
            asset,
            available_quantity,
            locked_quantity,
            total_quantity,
        })
    }

    #[must_use]
    pub fn asset(&self) -> &AssetCode {
        &self.asset
    }

    #[must_use]
    pub const fn available_quantity(&self) -> DomainDecimal {
        self.available_quantity
    }

    #[must_use]
    pub const fn locked_quantity(&self) -> DomainDecimal {
        self.locked_quantity
    }

    #[must_use]
    pub const fn total_quantity(&self) -> DomainDecimal {
        self.total_quantity
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAssetBalance {
    asset: AssetCode,
    expected_exchange_quantity: DomainDecimal,
    managed_quantity: DomainDecimal,
}

impl LocalAssetBalance {
    pub fn new(
        asset: AssetCode,
        expected_exchange_quantity: DomainDecimal,
        managed_quantity: DomainDecimal,
    ) -> Result<Self, PortfolioError> {
        if expected_exchange_quantity < DomainDecimal::ZERO
            || managed_quantity < DomainDecimal::ZERO
        {
            return Err(PortfolioError::NegativeBalance);
        }
        if managed_quantity > expected_exchange_quantity {
            return Err(PortfolioError::ManagedExceedsExpectedBalance);
        }
        Ok(Self {
            asset,
            expected_exchange_quantity,
            managed_quantity,
        })
    }

    #[must_use]
    pub fn asset(&self) -> &AssetCode {
        &self.asset
    }

    #[must_use]
    pub const fn expected_exchange_quantity(&self) -> DomainDecimal {
        self.expected_exchange_quantity
    }

    #[must_use]
    pub const fn managed_quantity(&self) -> DomainDecimal {
        self.managed_quantity
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetReconciliation {
    asset: AssetCode,
    exchange_available_quantity: DomainDecimal,
    exchange_locked_quantity: DomainDecimal,
    exchange_total_quantity: DomainDecimal,
    local_expected_quantity: DomainDecimal,
    managed_quantity: DomainDecimal,
    unknown_quantity: DomainDecimal,
    shortfall_quantity: DomainDecimal,
}

impl AssetReconciliation {
    #[must_use]
    pub fn asset(&self) -> &AssetCode {
        &self.asset
    }

    #[must_use]
    pub const fn exchange_available_quantity(&self) -> DomainDecimal {
        self.exchange_available_quantity
    }

    #[must_use]
    pub const fn exchange_locked_quantity(&self) -> DomainDecimal {
        self.exchange_locked_quantity
    }

    #[must_use]
    pub const fn exchange_total_quantity(&self) -> DomainDecimal {
        self.exchange_total_quantity
    }

    #[must_use]
    pub const fn local_expected_quantity(&self) -> DomainDecimal {
        self.local_expected_quantity
    }

    #[must_use]
    pub const fn managed_quantity(&self) -> DomainDecimal {
        self.managed_quantity
    }

    #[must_use]
    pub const fn unknown_quantity(&self) -> DomainDecimal {
        self.unknown_quantity
    }

    #[must_use]
    pub const fn shortfall_quantity(&self) -> DomainDecimal {
        self.shortfall_quantity
    }

    #[must_use]
    pub fn is_balanced(&self) -> bool {
        self.unknown_quantity == DomainDecimal::ZERO
            && self.shortfall_quantity == DomainDecimal::ZERO
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PortfolioReconciliationStatus {
    Balanced,
    BalanceDifference,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortfolioSnapshot {
    schema_version: &'static str,
    observed_at_unix_millis: u64,
    status: PortfolioReconciliationStatus,
    assets: Vec<AssetReconciliation>,
    snapshot_hash: PortfolioHash,
}

impl PortfolioSnapshot {
    #[must_use]
    pub const fn schema_version(&self) -> &'static str {
        self.schema_version
    }

    #[must_use]
    pub const fn observed_at_unix_millis(&self) -> u64 {
        self.observed_at_unix_millis
    }

    #[must_use]
    pub const fn status(&self) -> PortfolioReconciliationStatus {
        self.status
    }

    #[must_use]
    pub fn assets(&self) -> &[AssetReconciliation] {
        &self.assets
    }

    #[must_use]
    pub const fn snapshot_hash(&self) -> PortfolioHash {
        self.snapshot_hash
    }

    #[must_use]
    pub fn allows_new_entries(&self) -> bool {
        self.status == PortfolioReconciliationStatus::Balanced
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PortfolioReconciler;

impl PortfolioReconciler {
    pub fn reconcile(
        exchange_balances: Vec<ExchangeAssetBalance>,
        local_balances: Vec<LocalAssetBalance>,
        observed_at_unix_millis: u64,
    ) -> Result<PortfolioSnapshot, PortfolioError> {
        if exchange_balances.len() > MAX_PORTFOLIO_ASSETS
            || local_balances.len() > MAX_PORTFOLIO_ASSETS
        {
            return Err(PortfolioError::AssetCapacityExceeded);
        }
        let exchange = collect_unique_exchange(exchange_balances)?;
        let local = collect_unique_local(local_balances)?;
        let assets: BTreeSet<AssetCode> = exchange.keys().chain(local.keys()).cloned().collect();
        if assets.len() > MAX_PORTFOLIO_ASSETS {
            return Err(PortfolioError::AssetCapacityExceeded);
        }

        let mut reconciliations = Vec::with_capacity(assets.len());
        for asset in assets {
            let exchange_balance = exchange.get(&asset);
            let local_balance = local.get(&asset);
            let exchange_available_quantity = exchange_balance.map_or(
                DomainDecimal::ZERO,
                ExchangeAssetBalance::available_quantity,
            );
            let exchange_locked_quantity =
                exchange_balance.map_or(DomainDecimal::ZERO, ExchangeAssetBalance::locked_quantity);
            let exchange_total_quantity =
                exchange_balance.map_or(DomainDecimal::ZERO, ExchangeAssetBalance::total_quantity);
            let local_expected_quantity = local_balance.map_or(
                DomainDecimal::ZERO,
                LocalAssetBalance::expected_exchange_quantity,
            );
            let managed_quantity =
                local_balance.map_or(DomainDecimal::ZERO, LocalAssetBalance::managed_quantity);
            let (unknown_quantity, shortfall_quantity) =
                if exchange_total_quantity >= local_expected_quantity {
                    (
                        exchange_total_quantity
                            .checked_sub(local_expected_quantity)
                            .ok_or(PortfolioError::ArithmeticOverflow)?,
                        DomainDecimal::ZERO,
                    )
                } else {
                    (
                        DomainDecimal::ZERO,
                        local_expected_quantity
                            .checked_sub(exchange_total_quantity)
                            .ok_or(PortfolioError::ArithmeticOverflow)?,
                    )
                };
            reconciliations.push(AssetReconciliation {
                asset,
                exchange_available_quantity,
                exchange_locked_quantity,
                exchange_total_quantity,
                local_expected_quantity,
                managed_quantity,
                unknown_quantity,
                shortfall_quantity,
            });
        }
        let status = if reconciliations.iter().all(AssetReconciliation::is_balanced) {
            PortfolioReconciliationStatus::Balanced
        } else {
            PortfolioReconciliationStatus::BalanceDifference
        };
        let snapshot_hash =
            hash_portfolio_snapshot(observed_at_unix_millis, status, &reconciliations);
        Ok(PortfolioSnapshot {
            schema_version: PORTFOLIO_SCHEMA_VERSION_V1,
            observed_at_unix_millis,
            status,
            assets: reconciliations,
            snapshot_hash,
        })
    }
}

fn collect_unique_exchange(
    balances: Vec<ExchangeAssetBalance>,
) -> Result<BTreeMap<AssetCode, ExchangeAssetBalance>, PortfolioError> {
    let mut result = BTreeMap::new();
    for balance in balances {
        if result.insert(balance.asset.clone(), balance).is_some() {
            return Err(PortfolioError::DuplicateAsset);
        }
    }
    Ok(result)
}

fn collect_unique_local(
    balances: Vec<LocalAssetBalance>,
) -> Result<BTreeMap<AssetCode, LocalAssetBalance>, PortfolioError> {
    let mut result = BTreeMap::new();
    for balance in balances {
        if result.insert(balance.asset.clone(), balance).is_some() {
            return Err(PortfolioError::DuplicateAsset);
        }
    }
    Ok(result)
}

fn hash_portfolio_snapshot(
    observed_at_unix_millis: u64,
    status: PortfolioReconciliationStatus,
    assets: &[AssetReconciliation],
) -> PortfolioHash {
    let mut hasher = PortfolioHasher::new("portfolio-snapshot-v1");
    hasher.field(PORTFOLIO_SCHEMA_VERSION_V1);
    hasher.u64(observed_at_unix_millis);
    hasher.field(match status {
        PortfolioReconciliationStatus::Balanced => "balanced",
        PortfolioReconciliationStatus::BalanceDifference => "balance-difference",
    });
    hasher.usize(assets.len());
    for asset in assets {
        hasher.field(asset.asset.as_str());
        for value in [
            asset.exchange_available_quantity,
            asset.exchange_locked_quantity,
            asset.exchange_total_quantity,
            asset.local_expected_quantity,
            asset.managed_quantity,
            asset.unknown_quantity,
            asset.shortfall_quantity,
        ] {
            hasher.decimal(value);
        }
    }
    hasher.finish()
}

struct PortfolioHasher(Sha256);

impl PortfolioHasher {
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

    fn finish(self) -> PortfolioHash {
        PortfolioHash(self.0.finalize().into())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortfolioError {
    NonPositiveFillQuantity,
    ManagedLotIdRequired,
    ManagedLotIdForbidden,
    NegativeBalance,
    ManagedExceedsExpectedBalance,
    NonPositiveSellQuantity,
    SellExceedsManagedQuantity,
    SellExceedsExchangeAvailable,
    DuplicateAsset,
    AssetCapacityExceeded,
    ArithmeticOverflow,
}

impl fmt::Display for PortfolioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonPositiveFillQuantity => "portfolio fill quantities must be positive",
            Self::ManagedLotIdRequired => "a managed lot ID is required for a buy fill",
            Self::ManagedLotIdForbidden => "a sell fill must consume existing managed lots",
            Self::NegativeBalance => "portfolio quantities must not be negative",
            Self::ManagedExceedsExpectedBalance => {
                "managed quantity exceeds the local expected exchange balance"
            }
            Self::NonPositiveSellQuantity => "sell quantity must be positive",
            Self::SellExceedsManagedQuantity => {
                "sell quantity exceeds the provable managed quantity"
            }
            Self::SellExceedsExchangeAvailable => {
                "sell quantity exceeds the exchange available quantity"
            }
            Self::DuplicateAsset => "portfolio input contains a duplicate asset",
            Self::AssetCapacityExceeded => "portfolio asset count exceeds the fixed bound",
            Self::ArithmeticOverflow => "portfolio decimal arithmetic overflowed",
        })
    }
}

impl std::error::Error for PortfolioError {}

#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use uuid::Uuid;

    use super::*;
    use crate::{InstrumentTradingStatus, validated_spot_instrument_rules};

    fn decimal(value: &str) -> DomainDecimal {
        DomainDecimal::from_str(value).expect("test decimal must be valid")
    }

    fn asset(value: &str) -> AssetCode {
        AssetCode::new(value).expect("test asset must be valid")
    }

    fn instrument() -> InstrumentId {
        InstrumentId::from_str("bybit:spot:BTCUSDT").expect("test instrument must be valid")
    }

    fn rules() -> SpotInstrumentRules {
        validated_spot_instrument_rules(
            instrument(),
            asset("BTC"),
            asset("USDT"),
            InstrumentTradingStatus::Trading,
            decimal("0.01"),
            decimal("0.00000001"),
            decimal("0.00000001"),
            decimal("5"),
            decimal("10"),
            decimal("10"),
            decimal("10"),
            decimal("0.01"),
            decimal("0.01"),
        )
        .expect("test rules must be valid")
    }

    fn stable<T>(
        value: u128,
        constructor: impl FnOnce(Uuid) -> Result<T, crate::ParseStableIdError>,
    ) -> T {
        constructor(Uuid::from_u128(value)).expect("test ID must be valid")
    }

    #[test]
    fn sell_authorization_never_exceeds_managed_or_exchange_available_quantity() {
        let position =
            ManagedPosition::new(instrument(), asset("BTC"), decimal("1.25")).expect("valid");
        let authorization = position
            .authorize_sell(decimal("1.25"), decimal("2"))
            .expect("managed quantity may be sold");
        assert_eq!(authorization.approved_quantity(), decimal("1.25"));
        assert_eq!(
            position.authorize_sell(decimal("1.25000001"), decimal("2")),
            Err(PortfolioError::SellExceedsManagedQuantity)
        );
        assert_eq!(
            position.authorize_sell(decimal("1"), decimal("0.9")),
            Err(PortfolioError::SellExceedsExchangeAvailable)
        );
    }

    #[test]
    fn any_balance_difference_blocks_new_entries_and_classifies_unknown_assets() {
        let snapshot = PortfolioReconciler::reconcile(
            vec![
                ExchangeAssetBalance::new(asset("BTC"), decimal("1.2"), decimal("0"))
                    .expect("valid"),
                ExchangeAssetBalance::new(asset("DOGE"), decimal("5"), decimal("0"))
                    .expect("valid"),
            ],
            vec![
                LocalAssetBalance::new(asset("BTC"), decimal("1"), decimal("0.8")).expect("valid"),
            ],
            42,
        )
        .expect("reconciliation must succeed");

        assert_eq!(
            snapshot.status(),
            PortfolioReconciliationStatus::BalanceDifference
        );
        assert!(!snapshot.allows_new_entries());
        assert_eq!(snapshot.assets().len(), 2);
        assert_eq!(snapshot.assets()[0].asset().as_str(), "BTC");
        assert_eq!(snapshot.assets()[0].unknown_quantity(), decimal("0.2"));
        assert_eq!(snapshot.assets()[0].managed_quantity(), decimal("0.8"));
        assert_eq!(snapshot.assets()[1].asset().as_str(), "DOGE");
        assert_eq!(snapshot.assets()[1].unknown_quantity(), decimal("5"));
    }

    #[test]
    fn a_local_shortfall_is_explicit_and_blocks_entries() {
        let snapshot = PortfolioReconciler::reconcile(
            vec![
                ExchangeAssetBalance::new(asset("BTC"), decimal("0.9"), decimal("0"))
                    .expect("valid"),
            ],
            vec![
                LocalAssetBalance::new(asset("BTC"), decimal("1"), decimal("0.8")).expect("valid"),
            ],
            42,
        )
        .expect("reconciliation must succeed");

        assert!(!snapshot.allows_new_entries());
        assert_eq!(snapshot.assets()[0].shortfall_quantity(), decimal("0.1"));
        assert_eq!(snapshot.assets()[0].unknown_quantity(), DomainDecimal::ZERO);
    }

    #[test]
    fn exact_balances_are_trusted_and_hash_independent_of_input_order() {
        let exchange = vec![
            ExchangeAssetBalance::new(asset("USDT"), decimal("900"), decimal("100"))
                .expect("valid"),
            ExchangeAssetBalance::new(asset("BTC"), decimal("0.8"), decimal("0.2")).expect("valid"),
        ];
        let local = vec![
            LocalAssetBalance::new(asset("BTC"), decimal("1"), decimal("0.7")).expect("valid"),
            LocalAssetBalance::new(asset("USDT"), decimal("1000"), decimal("0")).expect("valid"),
        ];
        let first =
            PortfolioReconciler::reconcile(exchange.clone(), local.clone(), 42).expect("valid");
        let second = PortfolioReconciler::reconcile(
            exchange.into_iter().rev().collect(),
            local.into_iter().rev().collect(),
            42,
        )
        .expect("valid");

        assert!(first.allows_new_entries());
        assert_eq!(first.status(), PortfolioReconciliationStatus::Balanced);
        assert_eq!(first.snapshot_hash(), second.snapshot_hash());
        assert_eq!(first, second);
    }

    #[test]
    fn fill_contract_requires_a_buy_lot_and_forbids_a_sell_lot() {
        let fill_id = stable(1, FillId::new);
        let order_id = stable(2, OrderId::new);
        let trade_plan_id = stable(3, TradePlanId::new);
        let managed_lot_id = stable(4, ManagedLotId::new);

        assert_eq!(
            PortfolioFill::new(
                fill_id,
                order_id,
                trade_plan_id,
                None,
                &rules(),
                PortfolioFillSide::Buy,
                decimal("1"),
                decimal("100"),
                1,
            ),
            Err(PortfolioError::ManagedLotIdRequired)
        );
        assert_eq!(
            PortfolioFill::new(
                fill_id,
                order_id,
                trade_plan_id,
                Some(managed_lot_id),
                &rules(),
                PortfolioFillSide::Sell,
                decimal("1"),
                decimal("100"),
                1,
            ),
            Err(PortfolioError::ManagedLotIdForbidden)
        );
    }

    #[test]
    fn fill_contract_derives_instrument_and_assets_from_validated_rules() {
        let fill = PortfolioFill::new(
            stable(11, FillId::new),
            stable(12, OrderId::new),
            stable(13, TradePlanId::new),
            Some(stable(14, ManagedLotId::new)),
            &rules(),
            PortfolioFillSide::Buy,
            decimal("0.5"),
            decimal("50"),
            1,
        )
        .expect("fill must be valid");

        assert_eq!(fill.instrument_id(), &instrument());
        assert_eq!(fill.base_asset(), &asset("BTC"));
        assert_eq!(fill.quote_asset(), &asset("USDT"));
    }

    #[test]
    fn duplicate_assets_and_invalid_managed_balances_fail_closed() {
        let duplicate =
            ExchangeAssetBalance::new(asset("BTC"), decimal("1"), decimal("0")).expect("valid");
        assert_eq!(
            PortfolioReconciler::reconcile(vec![duplicate.clone(), duplicate], Vec::new(), 1),
            Err(PortfolioError::DuplicateAsset)
        );
        assert_eq!(
            LocalAssetBalance::new(asset("BTC"), decimal("1"), decimal("2")),
            Err(PortfolioError::ManagedExceedsExpectedBalance)
        );
    }
}
