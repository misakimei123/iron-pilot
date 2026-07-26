use bybit_sdk::{
    Category, OrderStatus as SdkOrderStatus, OrderType as SdkOrderType, Side as SdkSide,
    http::{Order as SdkOrder, PlaceOrderResponse as SdkPlaceOrderResponse, WalletBalance},
    ws::{
        Config as SdkStreamConfig, Event as SdkEvent, ExecutionMsg, Handle as SdkStreamHandle,
        IncomingMessage, OrderMsg, PrivateMsg, Stream as SdkStream, TopicMessage, WalletMsg,
    },
};
use core::fmt;
use core::str::FromStr;
use core::time::Duration;
use ironpilot_domain::{
    AccountOrderFact, AccountOrderSide, AccountOrderStatus, AiOrderType, AssetCode, DomainDecimal,
    ExchangeAssetBalance, InstrumentId, LocalAssetBalance, PortfolioReconciler, PortfolioSnapshot,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};

use crate::SqliteRepository;

pub const BYBIT_PRIVATE_SYNC_VERSION_V1: &str = "ironpilot-bybit-private-sync-v1";
pub const MAX_BYBIT_PRIVATE_BATCH_RECORDS: usize = 256;
pub const BYBIT_PRIVATE_COMMAND_QUEUE_SIZE: usize = 32;
pub const BYBIT_PRIVATE_EVENT_QUEUE_SIZE: usize = 256;
pub const BYBIT_PRIVATE_MAX_RECONNECT_ATTEMPTS: u32 = 5;

#[must_use]
pub fn bybit_private_sdk_config(url: impl Into<String>) -> SdkStreamConfig {
    SdkStreamConfig::new(url)
        .command_queue_size(BYBIT_PRIVATE_COMMAND_QUEUE_SIZE)
        .event_queue_size(BYBIT_PRIVATE_EVENT_QUEUE_SIZE)
        .max_reconnect_attempts(BYBIT_PRIVATE_MAX_RECONNECT_ATTEMPTS)
        .reconnect_base_delay(Duration::from_millis(500))
        .reconnect_max_delay(Duration::from_secs(8))
        .close_timeout(Duration::from_secs(5))
        .ping_interval(Some(Duration::from_secs(20)))
        .pong_timeout(Duration::from_secs(10))
}

#[must_use]
pub fn start_bybit_private_sdk_stream(
    url: impl Into<String>,
) -> (SdkStreamHandle, tokio::sync::mpsc::Receiver<SdkEvent>) {
    SdkStream::new(bybit_private_sdk_config(url))
}

#[must_use]
pub(crate) fn start_bybit_private_sdk_stream_through_socks5(
    url: impl Into<String>,
    proxy: impl Into<String>,
) -> (SdkStreamHandle, tokio::sync::mpsc::Receiver<SdkEvent>) {
    SdkStream::new(bybit_private_sdk_config(url).socks5_proxy(proxy))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BybitSyncEffect {
    Applied,
    DuplicateNoEffect,
    StaleNoEffect,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BybitPrivateIngestReport {
    applied: usize,
    duplicate: usize,
    stale: usize,
}

impl BybitPrivateIngestReport {
    #[must_use]
    pub const fn applied(self) -> usize {
        self.applied
    }

    #[must_use]
    pub const fn duplicate(self) -> usize {
        self.duplicate
    }

    #[must_use]
    pub const fn stale(self) -> usize {
        self.stale
    }

    fn record(&mut self, effect: BybitSyncEffect) {
        match effect {
            BybitSyncEffect::Applied => self.applied += 1,
            BybitSyncEffect::DuplicateNoEffect => self.duplicate += 1,
            BybitSyncEffect::StaleNoEffect => self.stale += 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BybitOrderFact {
    exchange_order_id: Box<str>,
    order_link_id: Option<Box<str>>,
    instrument_id: InstrumentId,
    side: AccountOrderSide,
    order_type: AiOrderType,
    limit_price: Option<DomainDecimal>,
    original_quantity: DomainDecimal,
    filled_quantity: DomainDecimal,
    status: StoredOrderStatus,
    updated_at_unix_millis: u64,
    payload_json: Box<str>,
    payload_hash: Box<str>,
}

impl BybitOrderFact {
    pub fn from_sdk_stream(value: &OrderMsg) -> Result<Self, BybitPrivateSyncError> {
        if value.category != Category::Spot {
            return Err(BybitPrivateSyncError::NonSpotFact);
        }
        Self::new(
            &value.order_id,
            value.order_link_id.as_deref(),
            &value.symbol,
            sdk_side(value.side),
            sdk_order_type(value.order_type)?,
            value.price.to_string(),
            value.qty.to_string(),
            value.cum_exec_qty.to_string(),
            sdk_order_status(value.order_status),
            value.updated_time,
        )
    }

    pub fn from_rest(value: &SdkOrder, category: Category) -> Result<Self, BybitPrivateSyncError> {
        if category != Category::Spot {
            return Err(BybitPrivateSyncError::NonSpotFact);
        }
        Self::new(
            &value.order_id,
            value.order_link_id.as_deref(),
            &value.symbol,
            sdk_side(value.side),
            sdk_order_type(value.order_type)?,
            value.price.to_string(),
            value.qty.to_string(),
            value.cum_exec_qty.to_string(),
            sdk_order_status(value.order_status),
            value.updated_time,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        exchange_order_id: &str,
        order_link_id: Option<&str>,
        symbol: &str,
        side: AccountOrderSide,
        order_type: AiOrderType,
        price: String,
        quantity: String,
        filled_quantity: String,
        status: StoredOrderStatus,
        updated_at_unix_millis: u64,
    ) -> Result<Self, BybitPrivateSyncError> {
        validate_label("exchange order ID", exchange_order_id)?;
        if let Some(value) = order_link_id {
            validate_label("order link ID", value)?;
        }
        if updated_at_unix_millis == 0 {
            return Err(BybitPrivateSyncError::InvalidTimestamp);
        }
        let instrument_id = instrument_from_symbol(symbol)?;
        let original_quantity = decimal(&quantity)?;
        let filled_quantity = decimal(&filled_quantity)?;
        if original_quantity <= DomainDecimal::ZERO
            || filled_quantity < DomainDecimal::ZERO
            || filled_quantity > original_quantity
        {
            return Err(BybitPrivateSyncError::InvalidQuantity);
        }
        let parsed_price = decimal(&price)?;
        let limit_price = match order_type {
            AiOrderType::Limit if parsed_price > DomainDecimal::ZERO => Some(parsed_price),
            AiOrderType::Market => None,
            AiOrderType::Limit => return Err(BybitPrivateSyncError::InvalidPrice),
        };
        let payload = json!({
            "schema_version": BYBIT_PRIVATE_SYNC_VERSION_V1,
            "exchange_order_id": exchange_order_id,
            "order_link_id": order_link_id,
            "instrument_id": instrument_id.to_string(),
            "side": side_name(side),
            "order_type": order_type_name(order_type),
            "limit_price": limit_price,
            "original_quantity": original_quantity,
            "filled_quantity": filled_quantity,
            "status": status.as_str(),
            "updated_at_unix_millis": updated_at_unix_millis
        });
        let payload_json = serde_json::to_string(&payload)
            .expect("validated Bybit order fact must serialize")
            .into_boxed_str();
        let payload_hash = sha256_hex(payload_json.as_bytes()).into_boxed_str();
        Ok(Self {
            exchange_order_id: exchange_order_id.into(),
            order_link_id: order_link_id.map(Into::into),
            instrument_id,
            side,
            order_type,
            limit_price,
            original_quantity,
            filled_quantity,
            status,
            updated_at_unix_millis,
            payload_json,
            payload_hash,
        })
    }

    fn to_context_fact(&self) -> Result<AccountOrderFact, BybitPrivateSyncError> {
        let status = match self.status {
            StoredOrderStatus::New => AccountOrderStatus::New,
            StoredOrderStatus::PartiallyFilled => AccountOrderStatus::PartiallyFilled,
            StoredOrderStatus::PendingCancel => AccountOrderStatus::PendingCancel,
            _ => return Err(BybitPrivateSyncError::TerminalOrderInContext),
        };
        AccountOrderFact::new(
            self.exchange_order_id.clone(),
            self.order_link_id.clone(),
            self.instrument_id.clone(),
            self.side,
            self.order_type,
            self.limit_price,
            self.original_quantity,
            self.filled_quantity,
            status,
            self.updated_at_unix_millis,
        )
        .map_err(|error| BybitPrivateSyncError::Domain(error.to_string()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BybitWalletFact {
    asset: AssetCode,
    wallet_quantity: DomainDecimal,
    locked_quantity: DomainDecimal,
    observed_at_unix_millis: u64,
    payload_json: Box<str>,
    payload_hash: Box<str>,
}

impl BybitWalletFact {
    pub fn from_sdk_stream(
        asset: &str,
        wallet_quantity: String,
        locked_quantity: String,
        observed_at_unix_millis: u64,
    ) -> Result<Self, BybitPrivateSyncError> {
        let asset = AssetCode::new(asset).map_err(|error| {
            BybitPrivateSyncError::Domain(format!("invalid wallet asset: {error}"))
        })?;
        let wallet_quantity = decimal(&wallet_quantity)?;
        let locked_quantity = decimal(&locked_quantity)?;
        if observed_at_unix_millis == 0
            || wallet_quantity < DomainDecimal::ZERO
            || locked_quantity < DomainDecimal::ZERO
            || locked_quantity > wallet_quantity
        {
            return Err(BybitPrivateSyncError::InvalidWallet);
        }
        let payload = json!({
            "schema_version": BYBIT_PRIVATE_SYNC_VERSION_V1,
            "asset": asset.as_str(),
            "wallet_quantity": wallet_quantity,
            "locked_quantity": locked_quantity,
            "observed_at_unix_millis": observed_at_unix_millis
        });
        let payload_json = serde_json::to_string(&payload)
            .expect("validated Bybit wallet fact must serialize")
            .into_boxed_str();
        let payload_hash = sha256_hex(payload_json.as_bytes()).into_boxed_str();
        Ok(Self {
            asset,
            wallet_quantity,
            locked_quantity,
            observed_at_unix_millis,
            payload_json,
            payload_hash,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BybitExecutionFact {
    execution_id: Box<str>,
    exchange_order_id: Box<str>,
    order_link_id: Option<Box<str>>,
    instrument_id: InstrumentId,
    side: AccountOrderSide,
    quantity: DomainDecimal,
    price: DomainDecimal,
    fee_quantity: DomainDecimal,
    fee_asset: AssetCode,
    occurred_at_unix_millis: u64,
    payload_json: Box<str>,
    payload_hash: Box<str>,
}

impl BybitExecutionFact {
    fn from_stream(value: &ExecutionMsg) -> Result<Self, BybitPrivateSyncError> {
        if value.category != Category::Spot {
            return Err(BybitPrivateSyncError::NonSpotFact);
        }
        validate_label("execution ID", &value.exec_id)?;
        validate_label("exchange order ID", &value.order_id)?;
        let instrument_id = instrument_from_symbol(&value.symbol)?;
        let quantity = decimal(&value.exec_qty.to_string())?;
        let price = decimal(&value.exec_price.to_string())?;
        let fee_quantity = decimal(&value.exec_fee.to_string())?;
        let fee_asset = AssetCode::new(value.fee_currency.as_str())
            .map_err(|error| BybitPrivateSyncError::Domain(error.to_string()))?;
        if value.exec_time == 0
            || quantity <= DomainDecimal::ZERO
            || price <= DomainDecimal::ZERO
            || fee_quantity < DomainDecimal::ZERO
        {
            return Err(BybitPrivateSyncError::InvalidExecution);
        }
        let side = sdk_side(value.side);
        let payload = json!({
            "schema_version": BYBIT_PRIVATE_SYNC_VERSION_V1,
            "execution_id": value.exec_id,
            "exchange_order_id": value.order_id,
            "order_link_id": value.order_link_id,
            "instrument_id": instrument_id.to_string(),
            "side": side_name(side),
            "quantity": quantity,
            "price": price,
            "fee_quantity": fee_quantity,
            "fee_asset": fee_asset.as_str(),
            "occurred_at_unix_millis": value.exec_time
        });
        let payload_json = serde_json::to_string(&payload)
            .expect("validated Bybit execution fact must serialize")
            .into_boxed_str();
        let payload_hash = sha256_hex(payload_json.as_bytes()).into_boxed_str();
        Ok(Self {
            execution_id: value.exec_id.clone().into_boxed_str(),
            exchange_order_id: value.order_id.clone().into_boxed_str(),
            order_link_id: value.order_link_id.clone().map(String::into_boxed_str),
            instrument_id,
            side,
            quantity,
            price,
            fee_quantity,
            fee_asset,
            occurred_at_unix_millis: value.exec_time,
            payload_json,
            payload_hash,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BybitReconciliationSnapshot {
    observed_at_unix_millis: u64,
    orders: Vec<BybitOrderFact>,
    wallets: Vec<BybitWalletFact>,
    snapshot_hash: Box<str>,
}

impl BybitReconciliationSnapshot {
    pub fn new(
        observed_at_unix_millis: u64,
        mut orders: Vec<BybitOrderFact>,
        mut wallets: Vec<BybitWalletFact>,
    ) -> Result<Self, BybitPrivateSyncError> {
        if observed_at_unix_millis == 0
            || wallets.is_empty()
            || orders.len() > MAX_BYBIT_PRIVATE_BATCH_RECORDS
            || wallets.len() > MAX_BYBIT_PRIVATE_BATCH_RECORDS
            || orders
                .iter()
                .any(|order| order.updated_at_unix_millis > observed_at_unix_millis)
            || wallets
                .iter()
                .any(|wallet| wallet.observed_at_unix_millis > observed_at_unix_millis)
        {
            return Err(BybitPrivateSyncError::InvalidSnapshot);
        }
        orders.sort_by(|left, right| left.exchange_order_id.cmp(&right.exchange_order_id));
        wallets.sort_by(|left, right| left.asset.cmp(&right.asset));
        if orders
            .windows(2)
            .any(|pair| pair[0].exchange_order_id == pair[1].exchange_order_id)
            || wallets
                .windows(2)
                .any(|pair| pair[0].asset == pair[1].asset)
        {
            return Err(BybitPrivateSyncError::DuplicateSnapshotFact);
        }
        let payload = json!({
            "schema_version": BYBIT_PRIVATE_SYNC_VERSION_V1,
            "observed_at_unix_millis": observed_at_unix_millis,
            "order_hashes": orders.iter().map(|value| value.payload_hash.as_ref()).collect::<Vec<_>>(),
            "wallet_hashes": wallets.iter().map(|value| value.payload_hash.as_ref()).collect::<Vec<_>>()
        });
        let snapshot_hash = sha256_hex(
            serde_json::to_string(&payload)
                .expect("reconciliation manifest must serialize")
                .as_bytes(),
        )
        .into_boxed_str();
        Ok(Self {
            observed_at_unix_millis,
            orders,
            wallets,
            snapshot_hash,
        })
    }

    pub fn from_sdk(
        observed_at_unix_millis: u64,
        category: Category,
        orders: &[SdkOrder],
        wallet: &WalletBalance,
    ) -> Result<Self, BybitPrivateSyncError> {
        let orders = orders
            .iter()
            .map(|value| BybitOrderFact::from_rest(value, category))
            .collect::<Result<Vec<_>, _>>()?;
        let wallets = wallet
            .coin
            .values()
            .map(|value| {
                BybitWalletFact::from_sdk_stream(
                    &value.coin,
                    value.wallet_balance.to_string(),
                    value.locked.to_string(),
                    observed_at_unix_millis,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(observed_at_unix_millis, orders, wallets)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BybitContextFacts {
    portfolio: PortfolioSnapshot,
    open_orders: Vec<AccountOrderFact>,
    latest_execution_at_unix_millis: Option<u64>,
}

impl BybitContextFacts {
    #[must_use]
    pub const fn portfolio(&self) -> &PortfolioSnapshot {
        &self.portfolio
    }

    #[must_use]
    pub fn open_orders(&self) -> &[AccountOrderFact] {
        &self.open_orders
    }

    #[must_use]
    pub const fn latest_execution_at_unix_millis(&self) -> Option<u64> {
        self.latest_execution_at_unix_millis
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BybitPrivateEvidenceCounts {
    order_acks: u64,
    private_events: u64,
    executions: u64,
}

impl BybitPrivateEvidenceCounts {
    #[must_use]
    pub const fn order_acks(self) -> u64 {
        self.order_acks
    }

    #[must_use]
    pub const fn private_events(self) -> u64 {
        self.private_events
    }

    #[must_use]
    pub const fn executions(self) -> u64 {
        self.executions
    }
}

pub struct SqliteBybitPrivateSync<'a> {
    repository: &'a SqliteRepository,
}

impl<'a> SqliteBybitPrivateSync<'a> {
    #[must_use]
    pub const fn new(repository: &'a SqliteRepository) -> Self {
        Self { repository }
    }

    pub async fn record_sdk_order_ack(
        &self,
        ack: &SdkPlaceOrderResponse,
        acknowledged_at_unix_millis: u64,
    ) -> Result<BybitSyncEffect, BybitPrivateSyncError> {
        validate_label("order link ID", &ack.order_link_id)?;
        validate_label("exchange order ID", &ack.order_id)?;
        if acknowledged_at_unix_millis == 0 {
            return Err(BybitPrivateSyncError::InvalidTimestamp);
        }
        let payload_json = json!({
            "schema_version": BYBIT_PRIVATE_SYNC_VERSION_V1,
            "order_link_id": ack.order_link_id,
            "exchange_order_id": ack.order_id,
            "acknowledged_at_unix_millis": acknowledged_at_unix_millis,
            "semantics": "ACKNOWLEDGED_NOT_FILLED"
        })
        .to_string();
        let payload_hash = sha256_hex(payload_json.as_bytes());
        let _guard = self.repository.write_gate.lock().await;
        let mut transaction = self.repository.pool.begin().await?;
        let inserted = sqlx::query(
            "INSERT INTO bybit_order_acks VALUES (?, ?, ?, ?, ?) ON CONFLICT(order_link_id) DO NOTHING",
        )
        .bind(&ack.order_link_id)
        .bind(&ack.order_id)
        .bind(to_i64(acknowledged_at_unix_millis)?)
        .bind(&payload_hash)
        .bind(&payload_json)
        .execute(&mut *transaction)
        .await?;
        let effect = if inserted.rows_affected() == 1 {
            audit(
                &mut transaction,
                &format!("bybit-ack:{}", ack.order_link_id),
                acknowledged_at_unix_millis,
                "BYBIT_ORDER_ACK_RECORDED",
                &ack.order_link_id,
                &payload_json,
            )
            .await?;
            BybitSyncEffect::Applied
        } else {
            let stored: String = sqlx::query_scalar(
                "SELECT payload_hash FROM bybit_order_acks WHERE order_link_id = ?",
            )
            .bind(&ack.order_link_id)
            .fetch_one(&mut *transaction)
            .await?;
            if stored != payload_hash {
                return Err(BybitPrivateSyncError::IdempotencyConflict);
            }
            BybitSyncEffect::DuplicateNoEffect
        };
        transaction.commit().await?;
        Ok(effect)
    }

    pub async fn ingest_sdk_event(
        &self,
        event: SdkEvent,
        observed_at_unix_millis: u64,
    ) -> Result<BybitPrivateIngestReport, BybitPrivateSyncError> {
        if observed_at_unix_millis == 0 {
            return Err(BybitPrivateSyncError::InvalidTimestamp);
        }
        match event {
            SdkEvent::Connected => {
                self.record_connected(observed_at_unix_millis).await?;
                Ok(BybitPrivateIngestReport::default())
            }
            SdkEvent::Reconnecting { .. } | SdkEvent::Disconnected { .. } => {
                self.record_disconnect(observed_at_unix_millis).await?;
                Ok(BybitPrivateIngestReport::default())
            }
            SdkEvent::ParseError(error) => {
                self.record_disconnect(observed_at_unix_millis).await?;
                Err(BybitPrivateSyncError::SdkParse(error))
            }
            SdkEvent::Message(IncomingMessage::Topic(message)) => self.ingest_topic(message).await,
            SdkEvent::Message(_) => Ok(BybitPrivateIngestReport::default()),
        }
    }

    async fn ingest_topic(
        &self,
        message: TopicMessage,
    ) -> Result<BybitPrivateIngestReport, BybitPrivateSyncError> {
        let _guard = self.repository.write_gate.lock().await;
        let mut transaction = self.repository.pool.begin().await?;
        let mut report = BybitPrivateIngestReport::default();
        match message {
            TopicMessage::Order(message) => {
                ensure_batch(message.data.len())?;
                for value in &message.data {
                    let fact = BybitOrderFact::from_sdk_stream(value)?;
                    report.record(persist_order_event(&mut transaction, &message, &fact).await?);
                }
            }
            TopicMessage::Execution(message) => {
                ensure_batch(message.data.len())?;
                for value in &message.data {
                    let fact = BybitExecutionFact::from_stream(value)?;
                    report
                        .record(persist_execution_event(&mut transaction, &message, &fact).await?);
                }
            }
            TopicMessage::Wallet(message) => {
                ensure_batch(message.data.len())?;
                for value in &message.data {
                    ensure_batch(value.coin.len())?;
                    let mut coins: Vec<_> = value.coin.values().collect();
                    coins.sort_by(|left, right| left.coin.cmp(&right.coin));
                    for coin in coins {
                        let fact = BybitWalletFact::from_sdk_stream(
                            &coin.coin,
                            coin.wallet_balance.to_string(),
                            coin.locked.to_string(),
                            message.creation_time,
                        )?;
                        report
                            .record(persist_wallet_event(&mut transaction, &message, &fact).await?);
                    }
                }
            }
            TopicMessage::Position(_)
            | TopicMessage::FastExecution(_)
            | TopicMessage::Greeks(_)
            | TopicMessage::Dcp(_) => {}
        }
        transaction.commit().await?;
        Ok(report)
    }

    pub async fn record_disconnect(
        &self,
        observed_at_unix_millis: u64,
    ) -> Result<(), BybitPrivateSyncError> {
        if observed_at_unix_millis == 0 {
            return Err(BybitPrivateSyncError::InvalidTimestamp);
        }
        let _guard = self.repository.write_gate.lock().await;
        let mut transaction = self.repository.pool.begin().await?;
        let existing: Option<(String, i64)> = sqlx::query_as(
            "SELECT state, generation FROM bybit_private_sync_state WHERE singleton_id = 1",
        )
        .fetch_optional(&mut *transaction)
        .await?;
        if existing
            .as_ref()
            .is_some_and(|(state, _)| state == "RECOVERY_REQUIRED")
        {
            transaction.rollback().await?;
            return Ok(());
        }
        let generation = existing
            .map_or(Some(1), |(_, value)| value.checked_add(1))
            .ok_or(BybitPrivateSyncError::CorruptStoredFact)?;
        let payload_json = json!({
            "schema_version": BYBIT_PRIVATE_SYNC_VERSION_V1,
            "state": "RECOVERY_REQUIRED",
            "generation": generation,
            "observed_at_unix_millis": observed_at_unix_millis,
            "reason": "private_stream_disconnected"
        })
        .to_string();
        sqlx::query(
            "
            INSERT INTO bybit_private_sync_state VALUES (1, 'RECOVERY_REQUIRED', ?, ?, ?)
            ON CONFLICT(singleton_id) DO UPDATE SET
                state = 'RECOVERY_REQUIRED',
                generation = excluded.generation,
                updated_at = excluded.updated_at,
                payload_json = excluded.payload_json
            ",
        )
        .bind(generation)
        .bind(to_i64(observed_at_unix_millis)?)
        .bind(&payload_json)
        .execute(&mut *transaction)
        .await?;
        audit(
            &mut transaction,
            &format!("bybit-sync:{generation}:recovery-required"),
            observed_at_unix_millis,
            "BYBIT_PRIVATE_RECOVERY_REQUIRED",
            &generation.to_string(),
            &payload_json,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn record_connected(
        &self,
        observed_at_unix_millis: u64,
    ) -> Result<(), BybitPrivateSyncError> {
        if observed_at_unix_millis == 0 {
            return Err(BybitPrivateSyncError::InvalidTimestamp);
        }
        let _guard = self.repository.write_gate.lock().await;
        let mut transaction = self.repository.pool.begin().await?;
        let existing: Option<(String, i64)> = sqlx::query_as(
            "SELECT state, generation FROM bybit_private_sync_state WHERE singleton_id = 1",
        )
        .fetch_optional(&mut *transaction)
        .await?;
        if existing
            .as_ref()
            .is_some_and(|(state, _)| matches!(state.as_str(), "RECOVERY_REQUIRED" | "LIVE"))
        {
            transaction.rollback().await?;
            return Ok(());
        }
        let generation = existing.map_or(0, |(_, value)| value);
        let payload_json = json!({
            "schema_version": BYBIT_PRIVATE_SYNC_VERSION_V1,
            "state": "LIVE",
            "generation": generation,
            "observed_at_unix_millis": observed_at_unix_millis,
            "reason": "private_stream_connected"
        })
        .to_string();
        sqlx::query(
            "
            INSERT INTO bybit_private_sync_state VALUES (1, 'LIVE', ?, ?, ?)
            ON CONFLICT(singleton_id) DO UPDATE SET
                state = 'LIVE',
                generation = excluded.generation,
                updated_at = excluded.updated_at,
                payload_json = excluded.payload_json
            ",
        )
        .bind(generation)
        .bind(to_i64(observed_at_unix_millis)?)
        .bind(&payload_json)
        .execute(&mut *transaction)
        .await?;
        audit(
            &mut transaction,
            &format!("bybit-sync:{generation}:live:{observed_at_unix_millis}"),
            observed_at_unix_millis,
            "BYBIT_PRIVATE_LIVE",
            &generation.to_string(),
            &payload_json,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn apply_reconciliation(
        &self,
        snapshot: &BybitReconciliationSnapshot,
    ) -> Result<BybitSyncEffect, BybitPrivateSyncError> {
        let _guard = self.repository.write_gate.lock().await;
        let mut transaction = self.repository.pool.begin().await?;
        let state: Option<(String, i64)> = sqlx::query_as(
            "SELECT state, generation FROM bybit_private_sync_state WHERE singleton_id = 1",
        )
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((state, generation)) = state else {
            return Err(BybitPrivateSyncError::RecoveryNotRequired);
        };
        let evidence_id = format!("bybit-reconcile:{generation}:{}", snapshot.snapshot_hash);
        let existing: Option<String> =
            sqlx::query_scalar("SELECT payload_json FROM audit_log WHERE audit_entry_id = ?")
                .bind(&evidence_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if existing.is_some() {
            transaction.rollback().await?;
            return Ok(BybitSyncEffect::DuplicateNoEffect);
        }
        if state != "RECOVERY_REQUIRED" {
            return Err(BybitPrivateSyncError::RecoveryNotRequired);
        }
        sqlx::query("DELETE FROM bybit_order_facts")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM bybit_wallet_facts")
            .execute(&mut *transaction)
            .await?;
        for order in &snapshot.orders {
            upsert_order(&mut transaction, order).await?;
        }
        for wallet in &snapshot.wallets {
            upsert_wallet(&mut transaction, wallet).await?;
        }
        let payload_json = json!({
            "schema_version": BYBIT_PRIVATE_SYNC_VERSION_V1,
            "state": "RECONCILED",
            "generation": generation,
            "snapshot_hash": snapshot.snapshot_hash,
            "observed_at_unix_millis": snapshot.observed_at_unix_millis,
            "orders": snapshot.orders.len(),
            "wallets": snapshot.wallets.len()
        })
        .to_string();
        let updated = sqlx::query(
            "UPDATE bybit_private_sync_state SET state = 'RECONCILED', updated_at = ?, payload_json = ? WHERE singleton_id = 1 AND generation = ?",
        )
        .bind(to_i64(snapshot.observed_at_unix_millis)?)
        .bind(&payload_json)
        .bind(generation)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(BybitPrivateSyncError::RecoveryNotRequired);
        }
        audit(
            &mut transaction,
            &evidence_id,
            snapshot.observed_at_unix_millis,
            "BYBIT_PRIVATE_RECONCILED",
            &generation.to_string(),
            &payload_json,
        )
        .await?;
        transaction.commit().await?;
        Ok(BybitSyncEffect::Applied)
    }

    pub async fn load_context_facts(
        &self,
        as_of_unix_millis: u64,
        local_balances: Vec<LocalAssetBalance>,
    ) -> Result<BybitContextFacts, BybitPrivateSyncError> {
        if as_of_unix_millis == 0 {
            return Err(BybitPrivateSyncError::InvalidTimestamp);
        }
        let mut transaction = self.repository.pool.begin().await?;
        let state: Option<String> =
            sqlx::query_scalar("SELECT state FROM bybit_private_sync_state WHERE singleton_id = 1")
                .fetch_optional(&mut *transaction)
                .await?;
        if !matches!(state.as_deref(), Some("LIVE" | "RECONCILED")) {
            return Err(BybitPrivateSyncError::RecoveryRequired);
        }
        let wallet_rows = sqlx::query(
            "SELECT asset, wallet_quantity, locked_quantity, observed_at FROM bybit_wallet_facts ORDER BY asset",
        )
        .fetch_all(&mut *transaction)
        .await?;
        let mut exchange_balances = Vec::with_capacity(wallet_rows.len());
        let mut portfolio_observed_at = 0_u64;
        for row in wallet_rows {
            let observed_at = from_i64(row.try_get("observed_at")?)?;
            if observed_at > as_of_unix_millis {
                return Err(BybitPrivateSyncError::FutureFact);
            }
            portfolio_observed_at = portfolio_observed_at.max(observed_at);
            let wallet_quantity = decimal(row.try_get("wallet_quantity")?)?;
            let locked_quantity = decimal(row.try_get("locked_quantity")?)?;
            let available_quantity = wallet_quantity
                .checked_sub(locked_quantity)
                .ok_or(BybitPrivateSyncError::InvalidWallet)?;
            exchange_balances.push(
                ExchangeAssetBalance::new(
                    AssetCode::new(row.try_get::<String, _>("asset")?)
                        .map_err(|error| BybitPrivateSyncError::Domain(error.to_string()))?,
                    available_quantity,
                    locked_quantity,
                )
                .map_err(|error| BybitPrivateSyncError::Domain(error.to_string()))?,
            );
        }
        if portfolio_observed_at == 0 {
            return Err(BybitPrivateSyncError::MissingWalletFacts);
        }
        let portfolio = PortfolioReconciler::reconcile(
            exchange_balances,
            local_balances,
            portfolio_observed_at,
        )
        .map_err(|error| BybitPrivateSyncError::Domain(error.to_string()))?;
        let order_rows = sqlx::query(
            "
            SELECT exchange_order_id, order_link_id, instrument_id, side, order_type,
                   limit_price, original_quantity, filled_quantity, status, updated_at,
                   payload_json, payload_hash
            FROM bybit_order_facts
            WHERE status IN ('NEW', 'PARTIALLY_FILLED', 'PENDING_CANCEL')
            ORDER BY instrument_id, exchange_order_id
            ",
        )
        .fetch_all(&mut *transaction)
        .await?;
        let mut open_orders = Vec::with_capacity(order_rows.len());
        for row in order_rows {
            let fact = order_from_row(&row)?;
            if fact.updated_at_unix_millis > as_of_unix_millis {
                return Err(BybitPrivateSyncError::FutureFact);
            }
            open_orders.push(fact.to_context_fact()?);
        }
        let latest_execution_at: Option<i64> =
            sqlx::query_scalar("SELECT MAX(occurred_at) FROM bybit_execution_facts")
                .fetch_one(&mut *transaction)
                .await?;
        let latest_execution_at_unix_millis = latest_execution_at.map(from_i64).transpose()?;
        if latest_execution_at_unix_millis.is_some_and(|value| value > as_of_unix_millis) {
            return Err(BybitPrivateSyncError::FutureFact);
        }
        transaction.commit().await?;
        Ok(BybitContextFacts {
            portfolio,
            open_orders,
            latest_execution_at_unix_millis,
        })
    }

    pub async fn evidence_counts(
        &self,
    ) -> Result<BybitPrivateEvidenceCounts, BybitPrivateSyncError> {
        let order_acks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bybit_order_acks")
            .fetch_one(&self.repository.pool)
            .await?;
        let private_events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bybit_private_events")
            .fetch_one(&self.repository.pool)
            .await?;
        let executions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bybit_execution_facts")
            .fetch_one(&self.repository.pool)
            .await?;
        Ok(BybitPrivateEvidenceCounts {
            order_acks: from_i64(order_acks)?,
            private_events: from_i64(private_events)?,
            executions: from_i64(executions)?,
        })
    }
}

async fn persist_order_event(
    transaction: &mut Transaction<'_, Sqlite>,
    message: &PrivateMsg<Vec<OrderMsg>>,
    fact: &BybitOrderFact,
) -> Result<BybitSyncEffect, BybitPrivateSyncError> {
    let event_key = format!(
        "order:{}:{}",
        fact.exchange_order_id, fact.updated_at_unix_millis
    );
    match insert_event(
        transaction,
        &event_key,
        "ORDER",
        &message.id,
        fact.updated_at_unix_millis,
        &fact.payload_hash,
        &fact.payload_json,
    )
    .await?
    {
        BybitSyncEffect::Applied => {}
        effect => return Ok(effect),
    }
    let effect = upsert_order(transaction, fact).await?;
    audit(
        transaction,
        &format!("bybit-event:{event_key}"),
        fact.updated_at_unix_millis,
        "BYBIT_ORDER_FACT",
        &fact.exchange_order_id,
        &fact.payload_json,
    )
    .await?;
    Ok(effect)
}

async fn persist_execution_event(
    transaction: &mut Transaction<'_, Sqlite>,
    message: &PrivateMsg<Vec<ExecutionMsg>>,
    fact: &BybitExecutionFact,
) -> Result<BybitSyncEffect, BybitPrivateSyncError> {
    let event_key = format!("execution:{}", fact.execution_id);
    match insert_event(
        transaction,
        &event_key,
        "EXECUTION",
        &message.id,
        fact.occurred_at_unix_millis,
        &fact.payload_hash,
        &fact.payload_json,
    )
    .await?
    {
        BybitSyncEffect::Applied => {}
        effect => return Ok(effect),
    }
    let inserted = sqlx::query(
        "
        INSERT INTO bybit_execution_facts VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(execution_id) DO NOTHING
        ",
    )
    .bind(&fact.execution_id)
    .bind(&fact.exchange_order_id)
    .bind(fact.order_link_id.as_deref())
    .bind(fact.instrument_id.to_string())
    .bind(side_name(fact.side))
    .bind(fact.quantity.to_string())
    .bind(fact.price.to_string())
    .bind(fact.fee_quantity.to_string())
    .bind(fact.fee_asset.as_str())
    .bind(to_i64(fact.occurred_at_unix_millis)?)
    .bind(&fact.payload_hash)
    .bind(&fact.payload_json)
    .execute(&mut **transaction)
    .await?;
    if inserted.rows_affected() != 1 {
        return Err(BybitPrivateSyncError::IdempotencyConflict);
    }
    audit(
        transaction,
        &format!("bybit-event:{event_key}"),
        fact.occurred_at_unix_millis,
        "BYBIT_EXECUTION_FACT",
        &fact.execution_id,
        &fact.payload_json,
    )
    .await?;
    Ok(BybitSyncEffect::Applied)
}

async fn persist_wallet_event(
    transaction: &mut Transaction<'_, Sqlite>,
    message: &PrivateMsg<Vec<WalletMsg>>,
    fact: &BybitWalletFact,
) -> Result<BybitSyncEffect, BybitPrivateSyncError> {
    let event_key = format!(
        "wallet:{}:{}",
        fact.asset.as_str(),
        fact.observed_at_unix_millis
    );
    match insert_event(
        transaction,
        &event_key,
        "WALLET",
        &message.id,
        fact.observed_at_unix_millis,
        &fact.payload_hash,
        &fact.payload_json,
    )
    .await?
    {
        BybitSyncEffect::Applied => {}
        effect => return Ok(effect),
    }
    let effect = upsert_wallet(transaction, fact).await?;
    audit(
        transaction,
        &format!("bybit-event:{event_key}"),
        fact.observed_at_unix_millis,
        "BYBIT_WALLET_FACT",
        fact.asset.as_str(),
        &fact.payload_json,
    )
    .await?;
    Ok(effect)
}

async fn insert_event(
    transaction: &mut Transaction<'_, Sqlite>,
    event_key: &str,
    event_kind: &str,
    source_message_id: &str,
    occurred_at_unix_millis: u64,
    payload_hash: &str,
    payload_json: &str,
) -> Result<BybitSyncEffect, BybitPrivateSyncError> {
    validate_label("source message ID", source_message_id)?;
    let inserted = sqlx::query(
        "INSERT INTO bybit_private_events VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(event_key) DO NOTHING",
    )
    .bind(event_key)
    .bind(event_kind)
    .bind(source_message_id)
    .bind(to_i64(occurred_at_unix_millis)?)
    .bind(payload_hash)
    .bind(payload_json)
    .execute(&mut **transaction)
    .await?;
    if inserted.rows_affected() == 1 {
        return Ok(BybitSyncEffect::Applied);
    }
    let stored: String =
        sqlx::query_scalar("SELECT payload_hash FROM bybit_private_events WHERE event_key = ?")
            .bind(event_key)
            .fetch_one(&mut **transaction)
            .await?;
    if stored == payload_hash {
        Ok(BybitSyncEffect::DuplicateNoEffect)
    } else {
        Err(BybitPrivateSyncError::IdempotencyConflict)
    }
}

async fn upsert_order(
    transaction: &mut Transaction<'_, Sqlite>,
    fact: &BybitOrderFact,
) -> Result<BybitSyncEffect, BybitPrivateSyncError> {
    let existing: Option<(i64, String)> = sqlx::query_as(
        "SELECT updated_at, payload_hash FROM bybit_order_facts WHERE exchange_order_id = ?",
    )
    .bind(&fact.exchange_order_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some((updated_at, hash)) = existing {
        let updated_at = from_i64(updated_at)?;
        if updated_at > fact.updated_at_unix_millis {
            return Ok(BybitSyncEffect::StaleNoEffect);
        }
        if updated_at == fact.updated_at_unix_millis {
            return if hash == fact.payload_hash.as_ref() {
                Ok(BybitSyncEffect::DuplicateNoEffect)
            } else {
                Err(BybitPrivateSyncError::FactConflict)
            };
        }
    }
    sqlx::query(
        "
        INSERT INTO bybit_order_facts VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(exchange_order_id) DO UPDATE SET
            order_link_id = excluded.order_link_id,
            instrument_id = excluded.instrument_id,
            side = excluded.side,
            order_type = excluded.order_type,
            limit_price = excluded.limit_price,
            original_quantity = excluded.original_quantity,
            filled_quantity = excluded.filled_quantity,
            status = excluded.status,
            updated_at = excluded.updated_at,
            payload_hash = excluded.payload_hash,
            payload_json = excluded.payload_json
        ",
    )
    .bind(&fact.exchange_order_id)
    .bind(fact.order_link_id.as_deref())
    .bind(fact.instrument_id.to_string())
    .bind(side_name(fact.side))
    .bind(order_type_name(fact.order_type))
    .bind(fact.limit_price.map(|value| value.to_string()))
    .bind(fact.original_quantity.to_string())
    .bind(fact.filled_quantity.to_string())
    .bind(fact.status.as_str())
    .bind(to_i64(fact.updated_at_unix_millis)?)
    .bind(&fact.payload_hash)
    .bind(&fact.payload_json)
    .execute(&mut **transaction)
    .await?;
    Ok(BybitSyncEffect::Applied)
}

async fn upsert_wallet(
    transaction: &mut Transaction<'_, Sqlite>,
    fact: &BybitWalletFact,
) -> Result<BybitSyncEffect, BybitPrivateSyncError> {
    let existing: Option<(i64, String)> =
        sqlx::query_as("SELECT observed_at, payload_hash FROM bybit_wallet_facts WHERE asset = ?")
            .bind(fact.asset.as_str())
            .fetch_optional(&mut **transaction)
            .await?;
    if let Some((observed_at, hash)) = existing {
        let observed_at = from_i64(observed_at)?;
        if observed_at > fact.observed_at_unix_millis {
            return Ok(BybitSyncEffect::StaleNoEffect);
        }
        if observed_at == fact.observed_at_unix_millis {
            return if hash == fact.payload_hash.as_ref() {
                Ok(BybitSyncEffect::DuplicateNoEffect)
            } else {
                Err(BybitPrivateSyncError::FactConflict)
            };
        }
    }
    sqlx::query(
        "
        INSERT INTO bybit_wallet_facts VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(asset) DO UPDATE SET
            wallet_quantity = excluded.wallet_quantity,
            locked_quantity = excluded.locked_quantity,
            observed_at = excluded.observed_at,
            payload_hash = excluded.payload_hash,
            payload_json = excluded.payload_json
        ",
    )
    .bind(fact.asset.as_str())
    .bind(fact.wallet_quantity.to_string())
    .bind(fact.locked_quantity.to_string())
    .bind(to_i64(fact.observed_at_unix_millis)?)
    .bind(&fact.payload_hash)
    .bind(&fact.payload_json)
    .execute(&mut **transaction)
    .await?;
    Ok(BybitSyncEffect::Applied)
}

async fn audit(
    transaction: &mut Transaction<'_, Sqlite>,
    audit_entry_id: &str,
    occurred_at_unix_millis: u64,
    category: &str,
    subject_id: &str,
    payload_json: &str,
) -> Result<(), BybitPrivateSyncError> {
    sqlx::query(
        "
        INSERT INTO audit_log(audit_entry_id, occurred_at, category, subject_id, payload_json)
        VALUES (?, ?, ?, ?, ?)
        ",
    )
    .bind(audit_entry_id)
    .bind(to_i64(occurred_at_unix_millis)?)
    .bind(category)
    .bind(subject_id)
    .bind(payload_json)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn order_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<BybitOrderFact, BybitPrivateSyncError> {
    Ok(BybitOrderFact {
        exchange_order_id: row
            .try_get::<String, _>("exchange_order_id")?
            .into_boxed_str(),
        order_link_id: row
            .try_get::<Option<String>, _>("order_link_id")?
            .map(String::into_boxed_str),
        instrument_id: InstrumentId::from_str(row.try_get::<String, _>("instrument_id")?.as_str())
            .map_err(|error| BybitPrivateSyncError::Domain(error.to_string()))?,
        side: match row.try_get::<String, _>("side")?.as_str() {
            "BUY" => AccountOrderSide::Buy,
            "SELL" => AccountOrderSide::Sell,
            _ => return Err(BybitPrivateSyncError::CorruptStoredFact),
        },
        order_type: match row.try_get::<String, _>("order_type")?.as_str() {
            "LIMIT" => AiOrderType::Limit,
            "MARKET" => AiOrderType::Market,
            _ => return Err(BybitPrivateSyncError::CorruptStoredFact),
        },
        limit_price: row
            .try_get::<Option<String>, _>("limit_price")?
            .map(|value| decimal(&value))
            .transpose()?,
        original_quantity: decimal(row.try_get("original_quantity")?)?,
        filled_quantity: decimal(row.try_get("filled_quantity")?)?,
        status: StoredOrderStatus::from_str(row.try_get::<String, _>("status")?.as_str())?,
        updated_at_unix_millis: from_i64(row.try_get("updated_at")?)?,
        payload_json: row.try_get::<String, _>("payload_json")?.into_boxed_str(),
        payload_hash: row.try_get::<String, _>("payload_hash")?.into_boxed_str(),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StoredOrderStatus {
    New,
    PartiallyFilled,
    PendingCancel,
    Filled,
    Cancelled,
    Rejected,
}

impl StoredOrderStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::New => "NEW",
            Self::PartiallyFilled => "PARTIALLY_FILLED",
            Self::PendingCancel => "PENDING_CANCEL",
            Self::Filled => "FILLED",
            Self::Cancelled => "CANCELLED",
            Self::Rejected => "REJECTED",
        }
    }

    fn from_str(value: &str) -> Result<Self, BybitPrivateSyncError> {
        match value {
            "NEW" => Ok(Self::New),
            "PARTIALLY_FILLED" => Ok(Self::PartiallyFilled),
            "PENDING_CANCEL" => Ok(Self::PendingCancel),
            "FILLED" => Ok(Self::Filled),
            "CANCELLED" => Ok(Self::Cancelled),
            "REJECTED" => Ok(Self::Rejected),
            _ => Err(BybitPrivateSyncError::CorruptStoredFact),
        }
    }
}

fn sdk_order_status(value: SdkOrderStatus) -> StoredOrderStatus {
    match value {
        SdkOrderStatus::New | SdkOrderStatus::Untriggered | SdkOrderStatus::Triggered => {
            StoredOrderStatus::New
        }
        SdkOrderStatus::PartiallyFilled => StoredOrderStatus::PartiallyFilled,
        SdkOrderStatus::Filled => StoredOrderStatus::Filled,
        SdkOrderStatus::Cancelled
        | SdkOrderStatus::PartiallyFilledCanceled
        | SdkOrderStatus::Deactivated => StoredOrderStatus::Cancelled,
        SdkOrderStatus::Rejected => StoredOrderStatus::Rejected,
    }
}

const fn sdk_side(value: SdkSide) -> AccountOrderSide {
    match value {
        SdkSide::Buy => AccountOrderSide::Buy,
        SdkSide::Sell => AccountOrderSide::Sell,
    }
}

fn sdk_order_type(value: SdkOrderType) -> Result<AiOrderType, BybitPrivateSyncError> {
    match value {
        SdkOrderType::Limit => Ok(AiOrderType::Limit),
        SdkOrderType::Market => Ok(AiOrderType::Market),
        SdkOrderType::UNKNOWN => Err(BybitPrivateSyncError::InvalidPrice),
    }
}

fn instrument_from_symbol(symbol: &str) -> Result<InstrumentId, BybitPrivateSyncError> {
    validate_label("symbol", symbol)?;
    InstrumentId::from_str(&format!("bybit:spot:{symbol}"))
        .map_err(|error| BybitPrivateSyncError::Domain(error.to_string()))
}

fn decimal(value: &str) -> Result<DomainDecimal, BybitPrivateSyncError> {
    DomainDecimal::from_str(value).map_err(|error| BybitPrivateSyncError::Domain(error.to_string()))
}

fn validate_label(field: &'static str, value: &str) -> Result<(), BybitPrivateSyncError> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(BybitPrivateSyncError::InvalidLabel { field });
    }
    Ok(())
}

fn ensure_batch(count: usize) -> Result<(), BybitPrivateSyncError> {
    if count > MAX_BYBIT_PRIVATE_BATCH_RECORDS {
        Err(BybitPrivateSyncError::BatchCapacityExceeded)
    } else {
        Ok(())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut result = String::with_capacity(64);
    for byte in digest {
        use core::fmt::Write as _;
        write!(result, "{byte:02x}").expect("writing to String cannot fail");
    }
    result
}

const fn side_name(value: AccountOrderSide) -> &'static str {
    match value {
        AccountOrderSide::Buy => "BUY",
        AccountOrderSide::Sell => "SELL",
    }
}

const fn order_type_name(value: AiOrderType) -> &'static str {
    match value {
        AiOrderType::Limit => "LIMIT",
        AiOrderType::Market => "MARKET",
    }
}

fn to_i64(value: u64) -> Result<i64, BybitPrivateSyncError> {
    i64::try_from(value).map_err(|_| BybitPrivateSyncError::InvalidTimestamp)
}

fn from_i64(value: i64) -> Result<u64, BybitPrivateSyncError> {
    u64::try_from(value).map_err(|_| BybitPrivateSyncError::CorruptStoredFact)
}

#[derive(Debug)]
pub enum BybitPrivateSyncError {
    Sqlx(sqlx::Error),
    InvalidLabel { field: &'static str },
    InvalidTimestamp,
    InvalidQuantity,
    InvalidPrice,
    InvalidWallet,
    InvalidExecution,
    InvalidSnapshot,
    DuplicateSnapshotFact,
    BatchCapacityExceeded,
    NonSpotFact,
    IdempotencyConflict,
    FactConflict,
    RecoveryRequired,
    RecoveryNotRequired,
    MissingWalletFacts,
    FutureFact,
    TerminalOrderInContext,
    CorruptStoredFact,
    SdkParse(String),
    Domain(String),
}

impl fmt::Display for BybitPrivateSyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlx(error) => write!(formatter, "{error}"),
            Self::InvalidLabel { field } => write!(formatter, "{field} is invalid"),
            Self::InvalidTimestamp => formatter.write_str("Bybit fact timestamp is invalid"),
            Self::InvalidQuantity => formatter.write_str("Bybit order quantity is invalid"),
            Self::InvalidPrice => formatter.write_str("Bybit order price is invalid"),
            Self::InvalidWallet => formatter.write_str("Bybit wallet fact is invalid"),
            Self::InvalidExecution => formatter.write_str("Bybit execution fact is invalid"),
            Self::InvalidSnapshot => {
                formatter.write_str("Bybit reconciliation snapshot is invalid")
            }
            Self::DuplicateSnapshotFact => {
                formatter.write_str("Bybit reconciliation snapshot contains duplicate facts")
            }
            Self::BatchCapacityExceeded => {
                formatter.write_str("Bybit private batch exceeds the fixed capacity")
            }
            Self::NonSpotFact => formatter.write_str("non-Spot Bybit fact was rejected"),
            Self::IdempotencyConflict => {
                formatter.write_str("Bybit idempotency key was reused with different content")
            }
            Self::FactConflict => {
                formatter.write_str("Bybit fact has conflicting content at the same timestamp")
            }
            Self::RecoveryRequired => {
                formatter.write_str("Bybit private facts require reconciliation")
            }
            Self::RecoveryNotRequired => {
                formatter.write_str("Bybit reconciliation was not requested")
            }
            Self::MissingWalletFacts => formatter.write_str("Bybit wallet facts are missing"),
            Self::FutureFact => formatter.write_str("Bybit private facts contain future data"),
            Self::TerminalOrderInContext => {
                formatter.write_str("terminal Bybit order cannot enter an AI Context")
            }
            Self::CorruptStoredFact => formatter.write_str("stored Bybit private fact is corrupt"),
            Self::SdkParse(error) => write!(formatter, "Bybit SDK parse error: {error}"),
            Self::Domain(error) => formatter.write_str(error),
        }
    }
}

impl std::error::Error for BybitPrivateSyncError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlx(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for BybitPrivateSyncError {
    fn from(error: sqlx::Error) -> Self {
        Self::Sqlx(error)
    }
}
