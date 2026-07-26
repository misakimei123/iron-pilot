use core::fmt;
use core::str::FromStr;

use ironpilot_application::{
    AuditEntry, ExecutionEffect, ExecutionReceipt, ExecutionVenue, PaperExecutionError,
    PaperExecutionPolicy, PaperMarketObservation, PaperMatchingEngine, PaperOpenOrder,
    PaperOrderEvaluation, PlannedSpotOrder, SpotExecutionPort, SpotExecutionRequest, UnixMillis,
};
use ironpilot_domain::{
    AccountOrderSide, AiOrderType, AiTimeInForce, AuditEntryId, DomainDecimal, FillId,
    ManagedLotId, OrderId, OrderIntentId, PortfolioFill, PortfolioFillSide, RuntimeInstanceId,
    SnapshotId, SpotInstrumentRules, TradePlanActionId, TradePlanId,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use crate::persistence::{
    StorageError, consume_managed_lots, domain_timestamp, ensure_instance_lease, insert_audit,
    insert_managed_lot, normalized_decimal,
};
use crate::{PersistenceEffect, SqliteRepository};

pub struct SqlitePaperExecutionPort<'a> {
    repository: &'a SqliteRepository,
    owner_id: RuntimeInstanceId,
    policy: PaperExecutionPolicy,
}

impl<'a> SqlitePaperExecutionPort<'a> {
    #[must_use]
    pub const fn new(
        repository: &'a SqliteRepository,
        owner_id: RuntimeInstanceId,
        policy: PaperExecutionPolicy,
    ) -> Self {
        Self {
            repository,
            owner_id,
            policy,
        }
    }

    pub async fn process_observation(
        &self,
        observation: &PaperMarketObservation,
        rules: &SpotInstrumentRules,
    ) -> Result<PaperExecutionReport, PaperExecutionAdapterError> {
        if observation.instrument_id() != rules.instrument_id() {
            return Err(PaperExecutionError::InstrumentMismatch.into());
        }
        let observed_at = domain_timestamp(observation.observed_at_unix_millis())?;
        let payload_json = observation.payload_json();
        let observation_hash = hash_text(&payload_json);
        let _write_guard = self.repository.write_gate.lock().await;
        let mut transaction = self.repository.pool.begin().await?;
        ensure_instance_lease(&mut transaction, self.owner_id, observed_at).await?;

        let insert = sqlx::query(
            "
            INSERT INTO paper_market_observations(
                observation_id, instrument_id, source_generated_at, observed_at,
                observation_hash, payload_json, effect_json
            )
            VALUES (?, ?, ?, ?, ?, ?, '{}')
            ON CONFLICT(observation_id) DO NOTHING
            ",
        )
        .bind(observation.observation_id().to_string())
        .bind(observation.instrument_id().to_string())
        .bind(domain_timestamp(
            observation.source_generated_at_unix_millis(),
        )?)
        .bind(observed_at)
        .bind(&observation_hash)
        .bind(&payload_json)
        .execute(&mut *transaction)
        .await?;
        if insert.rows_affected() == 0 {
            let existing: (String, i64, i64, String, String, String) = sqlx::query_as(
                "
                SELECT instrument_id, source_generated_at, observed_at,
                       observation_hash, payload_json, effect_json
                FROM paper_market_observations
                WHERE observation_id = ?
                ",
            )
            .bind(observation.observation_id().to_string())
            .fetch_one(&mut *transaction)
            .await?;
            let expected = (
                observation.instrument_id().to_string(),
                domain_timestamp(observation.source_generated_at_unix_millis())?,
                observed_at,
                observation_hash,
                payload_json,
            );
            if existing.0 != expected.0
                || existing.1 != expected.1
                || existing.2 != expected.2
                || existing.3 != expected.3
                || existing.4 != expected.4
            {
                return Err(StorageError::IdempotencyConflict.into());
            }
            let fill_ids = parse_effect_fill_ids(&existing.5)?;
            transaction.commit().await?;
            return Ok(PaperExecutionReport::new(
                PersistenceEffect::DuplicateNoEffect,
                fill_ids,
            ));
        }

        let rows = sqlx::query(
            "
            SELECT
                specs.order_id, orders.order_intent_id, specs.action_id,
                specs.trade_plan_id, specs.role, specs.take_profit_index,
                specs.side, specs.order_type, specs.quantity, specs.limit_price,
                specs.trigger_price, specs.time_in_force, specs.expires_at,
                specs.max_slippage_quote, specs.decision_as_of, specs.submitted_at,
                specs.filled_quantity
            FROM paper_order_specs AS specs
            JOIN paper_orders AS orders ON orders.order_id = specs.order_id
            WHERE specs.instrument_id = ?
              AND orders.state IN ('OPEN', 'PARTIALLY_FILLED', 'ACTIVE')
            ORDER BY
                CASE specs.role
                    WHEN 'PROTECTIVE_STOP' THEN 0
                    WHEN 'ENTRY' THEN 1
                    WHEN 'REDUCTION' THEN 2
                    WHEN 'EXIT' THEN 3
                    ELSE 4
                END,
                specs.take_profit_index,
                specs.order_id
            ",
        )
        .bind(observation.instrument_id().to_string())
        .fetch_all(&mut *transaction)
        .await?;

        let mut available_liquidity = observation.available_base_liquidity();
        let mut fill_ids = Vec::new();
        let mut protection_matched = false;
        let mut expired_order_ids = Vec::new();
        for row in rows {
            let stored = StoredPaperOrder::from_row(&row)?;
            if protection_matched && stored.role.is_take_profit() {
                continue;
            }
            let managed_quantity =
                managed_quantity(&mut transaction, rules.instrument_id()).await?;
            let remaining_quantity = match stored.quantity {
                Some(quantity) => {
                    let unfilled = quantity
                        .checked_sub(stored.filled_quantity)
                        .ok_or(PaperExecutionAdapterError::InvalidStoredOrder)?;
                    if stored.side == AccountOrderSide::Sell {
                        unfilled.min(managed_quantity)
                    } else {
                        unfilled
                    }
                }
                None => managed_quantity,
            };
            if remaining_quantity <= DomainDecimal::ZERO {
                cancel_order(&mut transaction, stored.order_id, observed_at).await?;
                continue;
            }
            let order = stored.to_planned_order()?;
            let open_order = PaperOpenOrder::new(
                order,
                rules.instrument_id().clone(),
                stored.decision_as_of,
                stored.submitted_at,
                remaining_quantity,
            )?;
            match PaperMatchingEngine::evaluate(
                &open_order,
                observation,
                available_liquidity,
                self.policy,
            ) {
                Ok(PaperOrderEvaluation::NoFill) => {}
                Ok(PaperOrderEvaluation::Expired) => {
                    expire_order(&mut transaction, &stored, observed_at, remaining_quantity)
                        .await?;
                    expired_order_ids.push(stored.order_id.to_string());
                }
                Ok(PaperOrderEvaluation::Fill(matched)) => {
                    let fill_id = stable_fill_id(stored.order_id, observation.observation_id())?;
                    let managed_lot_id = if matched.side() == AccountOrderSide::Buy {
                        Some(stable_managed_lot_id(
                            stored.order_id,
                            observation.observation_id(),
                        )?)
                    } else {
                        None
                    };
                    let side = match matched.side() {
                        AccountOrderSide::Buy => PortfolioFillSide::Buy,
                        AccountOrderSide::Sell => PortfolioFillSide::Sell,
                    };
                    let fill = PortfolioFill::new(
                        fill_id,
                        stored.order_id,
                        stored.trade_plan_id,
                        managed_lot_id,
                        rules,
                        side,
                        matched.base_quantity(),
                        matched.quote_quantity(),
                        matched.occurred_at_unix_millis(),
                    )
                    .map_err(StorageError::Portfolio)?;
                    let fill_payload = json!({
                        "schema_version": "ironpilot-paper-fill-v1",
                        "observation_id": observation.observation_id().to_string(),
                        "action_id": stored.action_id.to_string(),
                        "trade_plan_id": stored.trade_plan_id.to_string(),
                        "order_id": stored.order_id.to_string(),
                        "role": stored.role.as_str(),
                        "side": side_name(stored.side),
                        "base_quantity": normalized_decimal(matched.base_quantity()),
                        "execution_price": normalized_decimal(matched.execution_price()),
                        "quote_quantity": normalized_decimal(matched.quote_quantity()),
                        "fee_quote": normalized_decimal(matched.fee_quote())
                    });
                    sqlx::query(
                        "
                        INSERT INTO fills(fill_id, order_id, occurred_at, payload_json)
                        VALUES (?, ?, ?, ?)
                        ",
                    )
                    .bind(fill_id.to_string())
                    .bind(stored.order_id.to_string())
                    .bind(observed_at)
                    .bind(fill_payload.to_string())
                    .execute(&mut *transaction)
                    .await?;
                    match side {
                        PortfolioFillSide::Buy => {
                            insert_managed_lot(&mut transaction, &fill, observed_at).await?;
                        }
                        PortfolioFillSide::Sell => {
                            consume_managed_lots(&mut transaction, &fill, observed_at).await?;
                        }
                    }
                    let total_filled = stored
                        .filled_quantity
                        .checked_add(matched.base_quantity())
                        .ok_or(PaperExecutionAdapterError::ArithmeticFailure)?;
                    let is_fully_filled = stored
                        .quantity
                        .is_none_or(|quantity| total_filled >= quantity)
                        || (side == PortfolioFillSide::Sell
                            && managed_quantity <= matched.base_quantity());
                    sqlx::query(
                        "
                        UPDATE paper_order_specs
                        SET filled_quantity = ?,
                            accumulated_quote = ?,
                            accumulated_fee_quote = ?
                        WHERE order_id = ?
                        ",
                    )
                    .bind(normalized_decimal(total_filled))
                    .bind(normalized_decimal(
                        stored
                            .accumulated_quote(&mut transaction)
                            .await?
                            .checked_add(matched.quote_quantity())
                            .ok_or(PaperExecutionAdapterError::ArithmeticFailure)?,
                    ))
                    .bind(normalized_decimal(
                        stored
                            .accumulated_fee(&mut transaction)
                            .await?
                            .checked_add(matched.fee_quote())
                            .ok_or(PaperExecutionAdapterError::ArithmeticFailure)?,
                    ))
                    .bind(stored.order_id.to_string())
                    .execute(&mut *transaction)
                    .await?;
                    sqlx::query(
                        "
                        UPDATE paper_orders
                        SET state = ?, updated_at = ?
                        WHERE order_id = ?
                        ",
                    )
                    .bind(if is_fully_filled {
                        "FILLED"
                    } else {
                        "PARTIALLY_FILLED"
                    })
                    .bind(observed_at)
                    .bind(stored.order_id.to_string())
                    .execute(&mut *transaction)
                    .await?;
                    apply_fill_state(&mut transaction, &stored, observed_at, is_fully_filled)
                        .await?;
                    available_liquidity = available_liquidity
                        .checked_sub(matched.base_quantity())
                        .ok_or(PaperExecutionAdapterError::ArithmeticFailure)?;
                    if stored.role.is_protection() {
                        protection_matched = true;
                    }
                    fill_ids.push(fill_id);
                }
                Ok(PaperOrderEvaluation::SlippageLimitExceeded) => {}
                Err(PaperExecutionError::SlippageLimitExceeded) => {}
                Err(error) => return Err(error.into()),
            }
        }

        let effect = json!({
            "schema_version": "ironpilot-paper-observation-effect-v1",
            "fill_ids": fill_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "expired_order_ids": expired_order_ids
        });
        sqlx::query(
            "
            UPDATE paper_market_observations
            SET effect_json = ?
            WHERE observation_id = ?
            ",
        )
        .bind(effect.to_string())
        .bind(observation.observation_id().to_string())
        .execute(&mut *transaction)
        .await?;
        let audit = observation_audit(observation, &effect)?;
        insert_audit(&mut transaction, &audit).await?;
        transaction.commit().await?;
        Ok(PaperExecutionReport::new(
            PersistenceEffect::Applied,
            fill_ids,
        ))
    }
}

impl SpotExecutionPort for SqlitePaperExecutionPort<'_> {
    type Error = PaperExecutionAdapterError;

    fn submit<'a>(
        &'a self,
        request: &'a SpotExecutionRequest,
    ) -> ironpilot_application::ExecutionFuture<'a, ExecutionReceipt, Self::Error> {
        Box::pin(async move {
            let effect = self.submit_request(request).await?;
            Ok(ExecutionReceipt::new(
                ExecutionVenue::Paper,
                request.action_id(),
                match effect {
                    PersistenceEffect::Applied => ExecutionEffect::Applied,
                    PersistenceEffect::DuplicateNoEffect => ExecutionEffect::DuplicateNoEffect,
                },
            ))
        })
    }
}

impl SqlitePaperExecutionPort<'_> {
    async fn submit_request(
        &self,
        request: &SpotExecutionRequest,
    ) -> Result<PersistenceEffect, PaperExecutionAdapterError> {
        let created_at = domain_timestamp(request.created_at_unix_millis())?;
        let _write_guard = self.repository.write_gate.lock().await;
        let mut transaction = self.repository.pool.begin().await?;
        ensure_instance_lease(&mut transaction, self.owner_id, created_at).await?;

        let validation_matches: i64 = sqlx::query_scalar(
            "
            SELECT EXISTS(
                SELECT 1
                FROM execution_validations
                WHERE action_id = ?
                  AND trade_plan_id = ?
                  AND outcome = 'ACCEPT'
                  AND context_hash = ?
                  AND plan_hash = ?
                  AND validation_hash = ?
            )
            ",
        )
        .bind(request.action_id().to_string())
        .bind(request.trade_plan_id().to_string())
        .bind(request.context_hash())
        .bind(request.source_plan_hash())
        .bind(request.validation_hash())
        .fetch_one(&mut *transaction)
        .await?;
        if validation_matches != 1 {
            return Err(PaperExecutionAdapterError::ValidationEvidenceMismatch);
        }

        let insert = sqlx::query(
            "
            INSERT INTO paper_execution_submissions(
                action_id, trade_plan_id, venue, command, validation_hash,
                source_plan_hash, request_hash, created_at, payload_json
            )
            VALUES (?, ?, 'PAPER', ?, ?, ?, ?, ?, ?)
            ON CONFLICT(action_id) DO NOTHING
            ",
        )
        .bind(request.action_id().to_string())
        .bind(request.trade_plan_id().to_string())
        .bind(request.command().as_str())
        .bind(request.validation_hash())
        .bind(request.source_plan_hash())
        .bind(request.request_hash().to_string())
        .bind(created_at)
        .bind(request.payload_json())
        .execute(&mut *transaction)
        .await?;
        if insert.rows_affected() == 0 {
            let matches: i64 = sqlx::query_scalar(
                "
                SELECT EXISTS(
                    SELECT 1
                    FROM paper_execution_submissions
                    WHERE action_id = ?
                      AND trade_plan_id = ?
                      AND venue = 'PAPER'
                      AND command = ?
                      AND validation_hash = ?
                      AND source_plan_hash = ?
                      AND request_hash = ?
                      AND created_at = ?
                      AND payload_json = ?
                )
                ",
            )
            .bind(request.action_id().to_string())
            .bind(request.trade_plan_id().to_string())
            .bind(request.command().as_str())
            .bind(request.validation_hash())
            .bind(request.source_plan_hash())
            .bind(request.request_hash().to_string())
            .bind(created_at)
            .bind(request.payload_json())
            .fetch_one(&mut *transaction)
            .await?;
            if matches != 1 {
                return Err(StorageError::IdempotencyConflict.into());
            }
            transaction.commit().await?;
            return Ok(PersistenceEffect::DuplicateNoEffect);
        }

        use ironpilot_application::ExecutionCommandKind;
        match request.command() {
            ExecutionCommandKind::CancelEntry => {
                cancel_roles(
                    &mut transaction,
                    request.trade_plan_id(),
                    &["ENTRY"],
                    created_at,
                )
                .await?;
                update_action(&mut transaction, request.action_id(), "EXECUTED").await?;
                update_plan(
                    &mut transaction,
                    request.trade_plan_id(),
                    "CANCELLED",
                    created_at,
                )
                .await?;
            }
            ExecutionCommandKind::ModifyProtection => {
                cancel_roles(
                    &mut transaction,
                    request.trade_plan_id(),
                    &["PROTECTIVE_STOP", "TAKE_PROFIT"],
                    created_at,
                )
                .await?;
                insert_orders(&mut transaction, request, "ACTIVE").await?;
                update_action(&mut transaction, request.action_id(), "EXECUTED").await?;
            }
            ExecutionCommandKind::OpenLong => {
                insert_orders(&mut transaction, request, "OPEN").await?;
                update_action(&mut transaction, request.action_id(), "EXECUTION_PENDING").await?;
                update_plan(
                    &mut transaction,
                    request.trade_plan_id(),
                    "ENTRY_PENDING",
                    created_at,
                )
                .await?;
            }
            ExecutionCommandKind::Reduce => {
                insert_orders(&mut transaction, request, "OPEN").await?;
                update_action(&mut transaction, request.action_id(), "EXECUTION_PENDING").await?;
            }
            ExecutionCommandKind::Exit => {
                insert_orders(&mut transaction, request, "OPEN").await?;
                update_action(&mut transaction, request.action_id(), "EXECUTION_PENDING").await?;
                update_plan(
                    &mut transaction,
                    request.trade_plan_id(),
                    "EXIT_PENDING",
                    created_at,
                )
                .await?;
            }
        }
        let audit = submission_audit(request)?;
        insert_audit(&mut transaction, &audit).await?;
        transaction.commit().await?;
        Ok(PersistenceEffect::Applied)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperExecutionReport {
    effect: PersistenceEffect,
    fill_ids: Vec<FillId>,
}

impl PaperExecutionReport {
    const fn new(effect: PersistenceEffect, fill_ids: Vec<FillId>) -> Self {
        Self { effect, fill_ids }
    }

    #[must_use]
    pub const fn effect(&self) -> PersistenceEffect {
        self.effect
    }

    #[must_use]
    pub fn fill_ids(&self) -> &[FillId] {
        &self.fill_ids
    }
}

#[derive(Debug)]
pub enum PaperExecutionAdapterError {
    Storage(StorageError),
    Paper(PaperExecutionError),
    ValidationEvidenceMismatch,
    InvalidStoredOrder,
    InvalidStoredEffect,
    InvalidAudit,
    InvalidStableId,
    ArithmeticFailure,
}

impl fmt::Display for PaperExecutionAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "{error}"),
            Self::Paper(error) => write!(formatter, "{error}"),
            Self::ValidationEvidenceMismatch => {
                formatter.write_str("paper submission does not match accepted validation evidence")
            }
            Self::InvalidStoredOrder => formatter.write_str("stored paper order is invalid"),
            Self::InvalidStoredEffect => {
                formatter.write_str("stored paper observation effect is invalid")
            }
            Self::InvalidAudit => formatter.write_str("paper audit evidence is invalid"),
            Self::InvalidStableId => formatter.write_str("deterministic paper ID is invalid"),
            Self::ArithmeticFailure => formatter.write_str("paper persistence arithmetic failed"),
        }
    }
}

impl std::error::Error for PaperExecutionAdapterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Paper(error) => Some(error),
            _ => None,
        }
    }
}

impl From<StorageError> for PaperExecutionAdapterError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<sqlx::Error> for PaperExecutionAdapterError {
    fn from(value: sqlx::Error) -> Self {
        Self::Storage(StorageError::Sqlx(value))
    }
}

impl From<PaperExecutionError> for PaperExecutionAdapterError {
    fn from(value: PaperExecutionError) -> Self {
        Self::Paper(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StoredRole {
    Entry,
    ProtectiveStop,
    TakeProfit(u8),
    Reduction,
    Exit,
}

impl StoredRole {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Entry => "ENTRY",
            Self::ProtectiveStop => "PROTECTIVE_STOP",
            Self::TakeProfit(_) => "TAKE_PROFIT",
            Self::Reduction => "REDUCTION",
            Self::Exit => "EXIT",
        }
    }

    const fn is_take_profit(self) -> bool {
        matches!(self, Self::TakeProfit(_))
    }

    const fn is_protection(self) -> bool {
        matches!(self, Self::ProtectiveStop | Self::TakeProfit(_))
    }
}

struct StoredPaperOrder {
    order_id: OrderId,
    order_intent_id: OrderIntentId,
    action_id: TradePlanActionId,
    trade_plan_id: TradePlanId,
    role: StoredRole,
    side: AccountOrderSide,
    order_type: AiOrderType,
    quantity: Option<DomainDecimal>,
    limit_price: Option<DomainDecimal>,
    trigger_price: Option<DomainDecimal>,
    time_in_force: Option<AiTimeInForce>,
    expires_at: u64,
    max_slippage_quote: DomainDecimal,
    decision_as_of: u64,
    submitted_at: u64,
    filled_quantity: DomainDecimal,
}

impl StoredPaperOrder {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, PaperExecutionAdapterError> {
        let role: String = row.try_get("role")?;
        let take_profit_index: Option<i64> = row.try_get("take_profit_index")?;
        let role = match role.as_str() {
            "ENTRY" => StoredRole::Entry,
            "PROTECTIVE_STOP" => StoredRole::ProtectiveStop,
            "TAKE_PROFIT" => StoredRole::TakeProfit(
                u8::try_from(
                    take_profit_index.ok_or(PaperExecutionAdapterError::InvalidStoredOrder)?,
                )
                .map_err(|_| PaperExecutionAdapterError::InvalidStoredOrder)?,
            ),
            "REDUCTION" => StoredRole::Reduction,
            "EXIT" => StoredRole::Exit,
            _ => return Err(PaperExecutionAdapterError::InvalidStoredOrder),
        };
        Ok(Self {
            order_id: parse_id(row.try_get::<String, _>("order_id")?)?,
            order_intent_id: parse_id(row.try_get::<String, _>("order_intent_id")?)?,
            action_id: parse_id(row.try_get::<String, _>("action_id")?)?,
            trade_plan_id: parse_id(row.try_get::<String, _>("trade_plan_id")?)?,
            role,
            side: match row.try_get::<String, _>("side")?.as_str() {
                "BUY" => AccountOrderSide::Buy,
                "SELL" => AccountOrderSide::Sell,
                _ => return Err(PaperExecutionAdapterError::InvalidStoredOrder),
            },
            order_type: match row.try_get::<String, _>("order_type")?.as_str() {
                "LIMIT" => AiOrderType::Limit,
                "MARKET" => AiOrderType::Market,
                _ => return Err(PaperExecutionAdapterError::InvalidStoredOrder),
            },
            quantity: parse_optional_decimal(row.try_get("quantity")?)?,
            limit_price: parse_optional_decimal(row.try_get("limit_price")?)?,
            trigger_price: parse_optional_decimal(row.try_get("trigger_price")?)?,
            time_in_force: match row
                .try_get::<Option<String>, _>("time_in_force")?
                .as_deref()
            {
                Some("GTC") => Some(AiTimeInForce::Gtc),
                Some("IOC") => Some(AiTimeInForce::Ioc),
                Some("FOK") => Some(AiTimeInForce::Fok),
                None => None,
                _ => return Err(PaperExecutionAdapterError::InvalidStoredOrder),
            },
            expires_at: parse_timestamp(row.try_get("expires_at")?)?,
            max_slippage_quote: parse_decimal(row.try_get("max_slippage_quote")?)?,
            decision_as_of: parse_timestamp(row.try_get("decision_as_of")?)?,
            submitted_at: parse_timestamp(row.try_get("submitted_at")?)?,
            filled_quantity: parse_decimal(row.try_get("filled_quantity")?)?,
        })
    }

    fn to_planned_order(&self) -> Result<PlannedSpotOrder, PaperExecutionAdapterError> {
        let role = match self.role {
            StoredRole::Entry => ironpilot_application::ExecutionOrderRole::Entry,
            StoredRole::ProtectiveStop => ironpilot_application::ExecutionOrderRole::ProtectiveStop,
            StoredRole::TakeProfit(index) => {
                ironpilot_application::ExecutionOrderRole::TakeProfit { index }
            }
            StoredRole::Reduction => ironpilot_application::ExecutionOrderRole::Reduction,
            StoredRole::Exit => ironpilot_application::ExecutionOrderRole::Exit,
        };
        PlannedSpotOrder::from_persisted(
            ironpilot_application::ExecutionOrderIds::new(self.order_intent_id, self.order_id),
            role,
            self.side,
            self.order_type,
            self.quantity,
            self.limit_price,
            self.trigger_price,
            self.time_in_force,
            self.expires_at,
            self.max_slippage_quote,
        )
        .map_err(Into::into)
    }

    async fn accumulated_quote(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<DomainDecimal, PaperExecutionAdapterError> {
        let value: String = sqlx::query_scalar(
            "SELECT accumulated_quote FROM paper_order_specs WHERE order_id = ?",
        )
        .bind(self.order_id.to_string())
        .fetch_one(&mut **transaction)
        .await?;
        parse_decimal(value)
    }

    async fn accumulated_fee(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<DomainDecimal, PaperExecutionAdapterError> {
        let value: String = sqlx::query_scalar(
            "SELECT accumulated_fee_quote FROM paper_order_specs WHERE order_id = ?",
        )
        .bind(self.order_id.to_string())
        .fetch_one(&mut **transaction)
        .await?;
        parse_decimal(value)
    }
}

async fn insert_orders(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    request: &SpotExecutionRequest,
    live_state: &str,
) -> Result<(), PaperExecutionAdapterError> {
    for order in request.orders() {
        let state = if request.command() == ironpilot_application::ExecutionCommandKind::OpenLong
            && order.role() != ironpilot_application::ExecutionOrderRole::Entry
        {
            "STAGED"
        } else {
            live_state
        };
        sqlx::query(
            "
            INSERT INTO order_intents(
                order_intent_id, action_id, state, created_at, payload_json
            )
            VALUES (?, ?, ?, ?, ?)
            ",
        )
        .bind(order.ids().order_intent_id().to_string())
        .bind(request.action_id().to_string())
        .bind(state)
        .bind(domain_timestamp(request.created_at_unix_millis())?)
        .bind(order.payload_json())
        .execute(&mut **transaction)
        .await?;
        sqlx::query(
            "
            INSERT INTO paper_orders(
                order_id, order_intent_id, state, created_at, updated_at, payload_json
            )
            VALUES (?, ?, ?, ?, ?, ?)
            ",
        )
        .bind(order.ids().order_id().to_string())
        .bind(order.ids().order_intent_id().to_string())
        .bind(state)
        .bind(domain_timestamp(request.created_at_unix_millis())?)
        .bind(domain_timestamp(request.created_at_unix_millis())?)
        .bind(order.payload_json())
        .execute(&mut **transaction)
        .await?;
        sqlx::query(
            "
            INSERT INTO paper_order_specs(
                order_id, action_id, trade_plan_id, instrument_id, role,
                take_profit_index, side, order_type, quantity, limit_price,
                trigger_price, time_in_force, expires_at, max_slippage_quote,
                decision_as_of, submitted_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ",
        )
        .bind(order.ids().order_id().to_string())
        .bind(request.action_id().to_string())
        .bind(request.trade_plan_id().to_string())
        .bind(request.instrument_id().to_string())
        .bind(order.role().as_str())
        .bind(order.role().take_profit_index().map(i64::from))
        .bind(side_name(order.side()))
        .bind(order_type_name(order.order_type()))
        .bind(order.quantity().map(normalized_decimal))
        .bind(order.limit_price().map(normalized_decimal))
        .bind(order.trigger_price().map(normalized_decimal))
        .bind(order.time_in_force().map(time_in_force_name))
        .bind(domain_timestamp(order.expires_at_unix_millis())?)
        .bind(normalized_decimal(order.max_slippage_quote()))
        .bind(domain_timestamp(request.context_as_of_unix_millis())?)
        .bind(domain_timestamp(request.created_at_unix_millis())?)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn update_action(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    action_id: TradePlanActionId,
    state: &str,
) -> Result<(), PaperExecutionAdapterError> {
    let update = sqlx::query(
        "
        UPDATE trade_plan_actions
        SET state = ?
        WHERE action_id = ? AND state = 'VALIDATION_ACCEPTED'
        ",
    )
    .bind(state)
    .bind(action_id.to_string())
    .execute(&mut **transaction)
    .await?;
    if update.rows_affected() != 1 {
        return Err(PaperExecutionAdapterError::ValidationEvidenceMismatch);
    }
    Ok(())
}

async fn update_plan(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    trade_plan_id: TradePlanId,
    state: &str,
    updated_at: i64,
) -> Result<(), PaperExecutionAdapterError> {
    let update = sqlx::query(
        "
        UPDATE trade_plans
        SET state = ?, updated_at = ?
        WHERE trade_plan_id = ?
          AND state NOT IN ('REJECTED', 'CANCELLED', 'CLOSED')
        ",
    )
    .bind(state)
    .bind(updated_at)
    .bind(trade_plan_id.to_string())
    .execute(&mut **transaction)
    .await?;
    if update.rows_affected() != 1 {
        return Err(PaperExecutionAdapterError::InvalidStoredOrder);
    }
    Ok(())
}

async fn cancel_roles(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    trade_plan_id: TradePlanId,
    roles: &[&str],
    updated_at: i64,
) -> Result<(), PaperExecutionAdapterError> {
    for role in roles {
        sqlx::query(
            "
            UPDATE paper_orders
            SET state = 'CANCELLED', updated_at = ?
            WHERE order_id IN (
                SELECT order_id
                FROM paper_order_specs
                WHERE trade_plan_id = ? AND role = ?
            )
              AND state IN ('STAGED', 'OPEN', 'ACTIVE', 'PARTIALLY_FILLED')
            ",
        )
        .bind(updated_at)
        .bind(trade_plan_id.to_string())
        .bind(role)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn cancel_order(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    order_id: OrderId,
    updated_at: i64,
) -> Result<(), PaperExecutionAdapterError> {
    sqlx::query(
        "
        UPDATE paper_orders
        SET state = 'CANCELLED', updated_at = ?
        WHERE order_id = ?
          AND state IN ('OPEN', 'ACTIVE', 'PARTIALLY_FILLED')
        ",
    )
    .bind(updated_at)
    .bind(order_id.to_string())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn expire_order(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    order: &StoredPaperOrder,
    updated_at: i64,
    remaining_quantity: DomainDecimal,
) -> Result<(), PaperExecutionAdapterError> {
    sqlx::query("UPDATE paper_orders SET state = 'EXPIRED', updated_at = ? WHERE order_id = ?")
        .bind(updated_at)
        .bind(order.order_id.to_string())
        .execute(&mut **transaction)
        .await?;
    match order.role {
        StoredRole::Entry if order.filled_quantity == DomainDecimal::ZERO => {
            update_plan(transaction, order.trade_plan_id, "CANCELLED", updated_at).await?;
            sqlx::query("UPDATE trade_plan_actions SET state = 'EXPIRED' WHERE action_id = ?")
                .bind(order.action_id.to_string())
                .execute(&mut **transaction)
                .await?;
        }
        StoredRole::Reduction => {
            sqlx::query("UPDATE trade_plan_actions SET state = 'EXPIRED' WHERE action_id = ?")
                .bind(order.action_id.to_string())
                .execute(&mut **transaction)
                .await?;
        }
        _ => {
            sqlx::query(
                "UPDATE trade_plan_actions SET state = 'RECOVERY_REQUIRED' WHERE action_id = ?",
            )
            .bind(order.action_id.to_string())
            .execute(&mut **transaction)
            .await?;
            update_plan(
                transaction,
                order.trade_plan_id,
                "RECOVERY_REQUIRED",
                updated_at,
            )
            .await?;
        }
    }
    let _ = remaining_quantity;
    Ok(())
}

async fn apply_fill_state(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    order: &StoredPaperOrder,
    updated_at: i64,
    fully_filled: bool,
) -> Result<(), PaperExecutionAdapterError> {
    match order.role {
        StoredRole::Entry if fully_filled => {
            sqlx::query(
                "
                UPDATE paper_orders
                SET state = 'ACTIVE', updated_at = ?
                WHERE order_id IN (
                    SELECT order_id FROM paper_order_specs
                    WHERE trade_plan_id = ?
                      AND role IN ('PROTECTIVE_STOP', 'TAKE_PROFIT')
                )
                  AND state = 'STAGED'
                ",
            )
            .bind(updated_at)
            .bind(order.trade_plan_id.to_string())
            .execute(&mut **transaction)
            .await?;
            sqlx::query("UPDATE trade_plan_actions SET state = 'EXECUTED' WHERE action_id = ?")
                .bind(order.action_id.to_string())
                .execute(&mut **transaction)
                .await?;
            update_plan(transaction, order.trade_plan_id, "ACTIVE", updated_at).await?;
        }
        StoredRole::Reduction | StoredRole::Exit if fully_filled => {
            sqlx::query("UPDATE trade_plan_actions SET state = 'EXECUTED' WHERE action_id = ?")
                .bind(order.action_id.to_string())
                .execute(&mut **transaction)
                .await?;
            if order.role == StoredRole::Exit {
                close_trade_plan(transaction, order.trade_plan_id, updated_at).await?;
            }
        }
        StoredRole::ProtectiveStop | StoredRole::TakeProfit(_) => {
            let remaining = managed_quantity_by_plan(transaction, order.trade_plan_id).await?;
            if remaining == DomainDecimal::ZERO {
                close_trade_plan(transaction, order.trade_plan_id, updated_at).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn close_trade_plan(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    trade_plan_id: TradePlanId,
    updated_at: i64,
) -> Result<(), PaperExecutionAdapterError> {
    cancel_roles(
        transaction,
        trade_plan_id,
        &["PROTECTIVE_STOP", "TAKE_PROFIT"],
        updated_at,
    )
    .await?;
    update_plan(transaction, trade_plan_id, "CLOSED", updated_at).await
}

async fn managed_quantity(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    instrument_id: &ironpilot_domain::InstrumentId,
) -> Result<DomainDecimal, PaperExecutionAdapterError> {
    let values: Vec<String> = sqlx::query_scalar(
        "
        SELECT json_extract(payload_json, '$.remaining_quantity')
        FROM managed_lots
        WHERE instrument_id = ? AND closed_at IS NULL
        ",
    )
    .bind(instrument_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    sum_decimals(values)
}

async fn managed_quantity_by_plan(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    trade_plan_id: TradePlanId,
) -> Result<DomainDecimal, PaperExecutionAdapterError> {
    let values: Vec<String> = sqlx::query_scalar(
        "
        SELECT json_extract(payload_json, '$.remaining_quantity')
        FROM managed_lots
        WHERE trade_plan_id = ? AND closed_at IS NULL
        ",
    )
    .bind(trade_plan_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    sum_decimals(values)
}

fn sum_decimals(values: Vec<String>) -> Result<DomainDecimal, PaperExecutionAdapterError> {
    values
        .into_iter()
        .try_fold(DomainDecimal::ZERO, |total, value| {
            total
                .checked_add(parse_decimal(value)?)
                .ok_or(PaperExecutionAdapterError::ArithmeticFailure)
        })
}

fn submission_audit(
    request: &SpotExecutionRequest,
) -> Result<AuditEntry, PaperExecutionAdapterError> {
    let id = stable_audit_id("paper-submission", &request.action_id().to_string())?;
    AuditEntry::new(
        id,
        UnixMillis::new(domain_timestamp(request.created_at_unix_millis())?)
            .map_err(|_| PaperExecutionAdapterError::InvalidAudit)?,
        "paper_execution_submission",
        Some(request.action_id().to_string()),
        json!({
            "request_hash": request.request_hash().to_string(),
            "validation_hash": request.validation_hash(),
            "source_plan_hash": request.source_plan_hash(),
            "effect": "APPLIED"
        }),
    )
    .map_err(|_| PaperExecutionAdapterError::InvalidAudit)
}

fn observation_audit(
    observation: &PaperMarketObservation,
    effect: &Value,
) -> Result<AuditEntry, PaperExecutionAdapterError> {
    let id = stable_audit_id(
        "paper-observation",
        &observation.observation_id().to_string(),
    )?;
    AuditEntry::new(
        id,
        UnixMillis::new(domain_timestamp(observation.observed_at_unix_millis())?)
            .map_err(|_| PaperExecutionAdapterError::InvalidAudit)?,
        "paper_market_observation",
        Some(observation.observation_id().to_string()),
        effect.clone(),
    )
    .map_err(|_| PaperExecutionAdapterError::InvalidAudit)
}

fn stable_fill_id(
    order_id: OrderId,
    observation_id: SnapshotId,
) -> Result<FillId, PaperExecutionAdapterError> {
    FillId::new(stable_uuid(
        "paper-fill",
        &format!("{order_id}:{observation_id}"),
    ))
    .map_err(|_| PaperExecutionAdapterError::InvalidStableId)
}

fn stable_managed_lot_id(
    order_id: OrderId,
    observation_id: SnapshotId,
) -> Result<ManagedLotId, PaperExecutionAdapterError> {
    ManagedLotId::new(stable_uuid(
        "paper-managed-lot",
        &format!("{order_id}:{observation_id}"),
    ))
    .map_err(|_| PaperExecutionAdapterError::InvalidStableId)
}

fn stable_audit_id(
    namespace: &str,
    value: &str,
) -> Result<AuditEntryId, PaperExecutionAdapterError> {
    AuditEntryId::new(stable_uuid(namespace, value))
        .map_err(|_| PaperExecutionAdapterError::InvalidStableId)
}

fn stable_uuid(namespace: &str, value: &str) -> Uuid {
    let digest = Sha256::digest(format!("{namespace}:{value}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn hash_text(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_effect_fill_ids(value: &str) -> Result<Vec<FillId>, PaperExecutionAdapterError> {
    let value: Value =
        serde_json::from_str(value).map_err(|_| PaperExecutionAdapterError::InvalidStoredEffect)?;
    value
        .get("fill_ids")
        .and_then(Value::as_array)
        .ok_or(PaperExecutionAdapterError::InvalidStoredEffect)?
        .iter()
        .map(|value| {
            parse_id(
                value
                    .as_str()
                    .ok_or(PaperExecutionAdapterError::InvalidStoredEffect)?
                    .to_owned(),
            )
        })
        .collect()
}

fn parse_id<T: FromStr>(value: String) -> Result<T, PaperExecutionAdapterError> {
    value
        .parse()
        .map_err(|_| PaperExecutionAdapterError::InvalidStoredOrder)
}

fn parse_optional_decimal(
    value: Option<String>,
) -> Result<Option<DomainDecimal>, PaperExecutionAdapterError> {
    value.map(parse_decimal).transpose()
}

fn parse_decimal(value: String) -> Result<DomainDecimal, PaperExecutionAdapterError> {
    value
        .parse()
        .map_err(|_| PaperExecutionAdapterError::InvalidStoredOrder)
}

fn parse_timestamp(value: i64) -> Result<u64, PaperExecutionAdapterError> {
    u64::try_from(value).map_err(|_| PaperExecutionAdapterError::InvalidStoredOrder)
}

const fn side_name(side: AccountOrderSide) -> &'static str {
    match side {
        AccountOrderSide::Buy => "BUY",
        AccountOrderSide::Sell => "SELL",
    }
}

const fn order_type_name(order_type: AiOrderType) -> &'static str {
    match order_type {
        AiOrderType::Limit => "LIMIT",
        AiOrderType::Market => "MARKET",
    }
}

const fn time_in_force_name(time_in_force: AiTimeInForce) -> &'static str {
    match time_in_force {
        AiTimeInForce::Gtc => "GTC",
        AiTimeInForce::Ioc => "IOC",
        AiTimeInForce::Fok => "FOK",
    }
}
