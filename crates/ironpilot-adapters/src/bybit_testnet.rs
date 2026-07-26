use bybit_sdk::{
    AccountType, BASE_URL_API_TESTNET, BASE_URL_STREAM_TESTNET, Category, OrderStatus, OrderType,
    Path as SdkPath, SensitiveString, Side, TimeInForce, Topic,
    http::{
        CancelOrderRequest, Client as SdkClient, Config as SdkHttpConfig, GetExecutionListParams,
        GetInstrumentsInfoParams, GetOpenClosedOrdersParams, GetOrderHistoryParams,
        GetTickersParams, GetWalletBalanceParams, InstrumentsInfo, PlaceOrderRequest,
        SpotInstrumentsInfo, Ticker,
    },
    ws::{
        CommandMsg, Event as SdkEvent, IncomingMessage, OutgoingMessage,
        create_outgoing_message_auth,
    },
};
use core::{fmt, str::FromStr};
use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ironpilot_application::{
    AuthorizedEmergencyCommand, EmergencyCommandKind, ExecutionOrderIds, ExecutionOrderRole,
    PlannedSpotOrder,
};
use ironpilot_domain::{
    AccountOrderSide, AiOrderType, AiTimeInForce, DomainDecimal, EmergencyActionId, OrderId,
    OrderIntentId,
};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use tokio::{sync::mpsc::Receiver, time};
use uuid::Uuid;

use crate::bybit_private::start_bybit_private_sdk_stream_through_socks5;
use crate::{
    BybitReconciliationSnapshot, BybitSyncEffect, SqliteBybitPrivateSync, SqliteRepository,
};

pub const BYBIT_TESTNET_PROTOCOL_SMOKE_VERSION_V1: &str =
    "ironpilot-bybit-testnet-protocol-smoke-v1";
pub const BYBIT_TESTNET_WRITE_AUTHORIZATION_ENV: &str =
    "IRONPILOT_BYBIT_TESTNET_WRITE_AUTHORIZATION";
pub const BYBIT_TESTNET_WRITE_AUTHORIZATION_VALUE: &str = "P4-02A:BYBIT-TESTNET:SPOT:WRITE";
pub const BYBIT_TESTNET_API_KEY_ENV: &str = "IRONPILOT_BYBIT_TESTNET_API_KEY";
pub const BYBIT_TESTNET_API_SECRET_ENV: &str = "IRONPILOT_BYBIT_TESTNET_API_SECRET";
pub const BYBIT_TESTNET_SOCKS5_PROXY_ENV: &str = "IRONPILOT_BYBIT_TESTNET_SOCKS5_PROXY";
pub const BYBIT_TESTNET_SYMBOL: &str = "BTCUSDT";
pub const BYBIT_TESTNET_ORDER_LINK_PREFIX: &str = "ip4-";
pub const BYBIT_TESTNET_MAX_ORDER_QUOTE: Decimal = Decimal::TEN;
const SDK_CALL_TIMEOUT: Duration = Duration::from_secs(20);
const PRIVATE_EVENT_WINDOW: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestnetSubmissionEffect {
    Submitted,
    DuplicateNoEffect,
}

#[derive(Debug)]
pub struct TestnetOrderMapping {
    source_payload_hash: Box<str>,
    sdk_payload_hash: Box<str>,
    sdk_payload_json: Box<str>,
    request: PlaceOrderRequest,
}

impl TestnetOrderMapping {
    #[must_use]
    pub fn source_payload_hash(&self) -> &str {
        &self.source_payload_hash
    }

    #[must_use]
    pub fn sdk_payload_hash(&self) -> &str {
        &self.sdk_payload_hash
    }

    #[must_use]
    pub fn sdk_payload_json(&self) -> &str {
        &self.sdk_payload_json
    }

    #[must_use]
    pub const fn request(&self) -> &PlaceOrderRequest {
        &self.request
    }
}

pub fn map_planned_spot_order_to_testnet(
    symbol: &str,
    order_link_id: &str,
    order: &PlannedSpotOrder,
) -> Result<TestnetOrderMapping, BybitTestnetSmokeError> {
    if symbol != BYBIT_TESTNET_SYMBOL
        || !valid_owned_order_link_id(order_link_id)
        || order.role() == ExecutionOrderRole::ProtectiveStop
        || order.trigger_price().is_some()
    {
        return Err(BybitTestnetSmokeError::UnsafeOrder);
    }
    let quantity = decimal(
        order
            .quantity()
            .ok_or(BybitTestnetSmokeError::UnsafeOrder)?,
    )?;
    let side = match order.side() {
        AccountOrderSide::Buy => Side::Buy,
        AccountOrderSide::Sell => Side::Sell,
    };
    let order_type = match order.order_type() {
        AiOrderType::Limit => OrderType::Limit,
        AiOrderType::Market => OrderType::Market,
    };
    let mut request = PlaceOrderRequest::new(
        Category::Spot,
        symbol.to_owned(),
        side,
        order_type,
        quantity,
    )
    .with_is_leverage(0)
    .with_market_unit("baseCoin".to_owned())
    .with_order_link_id(order_link_id.to_owned());
    if let Some(price) = order.limit_price() {
        request = request.with_price(decimal(price)?);
    }
    if let Some(value) = order.time_in_force() {
        request = request.with_time_in_force(match value {
            AiTimeInForce::Gtc => TimeInForce::GTC,
            AiTimeInForce::Ioc => TimeInForce::IOC,
            AiTimeInForce::Fok => TimeInForce::FOK,
        });
    }
    let sdk_payload_json =
        serde_json::to_string(&request).map_err(BybitTestnetSmokeError::Serialize)?;
    Ok(TestnetOrderMapping {
        source_payload_hash: sha256_hex(order.payload_json().as_bytes()).into_boxed_str(),
        sdk_payload_hash: sha256_hex(sdk_payload_json.as_bytes()).into_boxed_str(),
        sdk_payload_json: sdk_payload_json.into_boxed_str(),
        request,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BybitTestnetSmokeReport {
    pub schema_version: &'static str,
    pub run_id: String,
    pub symbol: &'static str,
    pub cancel_probe_order_link_id: String,
    pub fill_probe_order_link_id: String,
    pub emergency_exit_order_link_id: String,
    pub duplicate_effect: &'static str,
    pub rest_ack_count: u64,
    pub private_event_count: u64,
    pub private_execution_count: u64,
    pub restart_reconciled: bool,
    pub emergency_converged: bool,
    pub max_order_quote: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BybitTestnetRecoveryOrder {
    pub order_link_id: String,
    pub order_status: String,
    pub cumulative_executed_quantity: String,
    pub cumulative_executed_value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BybitTestnetRecoveryReport {
    pub cancelled_open_orders: u64,
    pub owned_orders: Vec<BybitTestnetRecoveryOrder>,
    pub owned_execution_count: u64,
    pub owned_buy_execution_quantity: String,
    pub owned_sell_execution_quantity: String,
}

pub async fn run_bybit_testnet_protocol_smoke(
    repository_path: &Path,
    api_key: String,
    api_secret: String,
    write_authorization: &str,
    socks5_proxy: &str,
) -> Result<BybitTestnetSmokeReport, BybitTestnetSmokeError> {
    authorize(&api_key, &api_secret, write_authorization, socks5_proxy)?;
    let run_id = Uuid::new_v4().simple().to_string();
    let cancel_link = owned_link(&run_id, 'c');
    let fill_link = owned_link(&run_id, 'f');
    let emergency_link = owned_link(&run_id, 'e');
    let client = SdkClient::new(SdkHttpConfig {
        base_url: BASE_URL_API_TESTNET.to_owned(),
        api_key: Some(SensitiveString::from(api_key.as_str())),
        api_secret: Some(SensitiveString::from(api_secret.as_str())),
        recv_window: 5_000,
        referer: None,
    })
    .map_err(BybitTestnetSmokeError::Sdk)?;

    let result = async {
        preflight(&client).await?;
        let (instrument, bid, ask) = market_contract(&client).await?;
        let repository = SqliteRepository::connect(repository_path, 1).await?;
        let sync = SqliteBybitPrivateSync::new(&repository);
        let (handle, mut events) = start_bybit_private_sdk_stream_through_socks5(
            format!("{BASE_URL_STREAM_TESTNET}{}", SdkPath::Private),
            socks5_proxy,
        );
        handle.connect().await?;
        authenticate_private_stream(&handle, &mut events, &sync, &api_key, &api_secret).await?;

        let cancel_price =
            quantize_down(bid * Decimal::new(99, 2), instrument.price_filter.tick_size)?;
        let cancel_quantity = safe_base_quantity(
            &instrument,
            cancel_price,
            instrument.lot_size_filter.min_order_amt,
        )?;
        let cancel_order = planned_order(
            AccountOrderSide::Buy,
            AiOrderType::Limit,
            cancel_quantity,
            Some(cancel_price),
            Some(AiTimeInForce::Gtc),
        )?;
        let cancel_mapping =
            map_planned_spot_order_to_testnet(BYBIT_TESTNET_SYMBOL, &cancel_link, &cancel_order)?;
        enforce_quote_cap(cancel_mapping.request(), cancel_price)?;
        submit_once(
            &repository,
            &client,
            &sync,
            &run_id,
            "CANCEL_PROBE",
            &cancel_link,
            &cancel_mapping,
        )
        .await?;
        let duplicate = submit_once(
            &repository,
            &client,
            &sync,
            &run_id,
            "CANCEL_PROBE",
            &cancel_link,
            &cancel_mapping,
        )
        .await?;
        if duplicate != TestnetSubmissionEffect::DuplicateNoEffect {
            return Err(BybitTestnetSmokeError::IdempotencyFailed);
        }
        query_order(&client, &cancel_link).await?;
        sdk_timeout(
            client.cancel_order(
                &CancelOrderRequest::new(Category::Spot, BYBIT_TESTNET_SYMBOL.to_owned())
                    .with_order_link_id(cancel_link.clone()),
            ),
        )
        .await?;
        pump_private_events(&mut events, &sync, PRIVATE_EVENT_WINDOW).await?;

        let fill_quantity = safe_base_quantity(
            &instrument,
            ask,
            instrument.lot_size_filter.min_order_amt * Decimal::new(105, 2),
        )?;
        let fill_order = planned_order(
            AccountOrderSide::Buy,
            AiOrderType::Market,
            fill_quantity,
            None,
            None,
        )?;
        let fill_mapping =
            map_planned_spot_order_to_testnet(BYBIT_TESTNET_SYMBOL, &fill_link, &fill_order)?;
        enforce_quote_cap(fill_mapping.request(), ask)?;
        submit_once(
            &repository,
            &client,
            &sync,
            &run_id,
            "FILL_PROBE",
            &fill_link,
            &fill_mapping,
        )
        .await?;
        pump_private_events(&mut events, &sync, PRIVATE_EVENT_WINDOW).await?;
        let managed_quantity = quantize_down(
            managed_bought_quantity(&repository, &fill_link).await?,
            instrument.lot_size_filter.base_precision,
        )?;

        let issued_at = now_millis()?;
        let emergency_command = AuthorizedEmergencyCommand::new(
            EmergencyActionId::new(Uuid::new_v4())
                .map_err(|_| BybitTestnetSmokeError::Emergency)?,
            EmergencyCommandKind::CloseAllManagedExposure,
            "P4-02A Bybit Testnet protocol smoke",
            Sha256::digest(b"P4-02A user-authorized Bybit Testnet Emergency").into(),
            Sha256::digest(Uuid::new_v4().as_bytes()).into(),
            issued_at,
            issued_at + 60_000,
        )
        .map_err(|_| BybitTestnetSmokeError::Emergency)?;
        emergency_close(
            &repository,
            &client,
            &sync,
            &run_id,
            &emergency_link,
            managed_quantity,
            bid,
            &emergency_command,
        )
        .await?;
        pump_private_events(&mut events, &sync, PRIVATE_EVENT_WINDOW).await?;
        let counts = sync.evidence_counts().await?;
        if counts.executions() < 2 || counts.private_events() < 2 {
            return Err(BybitTestnetSmokeError::MissingPrivateEvidence);
        }

        handle.disconnect().await?;
        sync.record_disconnect(now_millis()?).await?;
        repository.close().await;

        let restarted = SqliteRepository::connect(repository_path, 1).await?;
        let restarted_sync = SqliteBybitPrivateSync::new(&restarted);
        let orders = sdk_timeout(
            client.get_open_closed_orders_all(
                &GetOpenClosedOrdersParams::new(Category::Spot)
                    .with_symbol(BYBIT_TESTNET_SYMBOL.to_owned())
                    .with_open_only(0)
                    .with_limit(50),
            ),
        )
        .await?;
        let wallet = wallet(&client).await?;
        let snapshot =
            BybitReconciliationSnapshot::from_sdk(now_millis()?, Category::Spot, &orders, &wallet)?;
        let reconcile_effect = restarted_sync.apply_reconciliation(&snapshot).await?;
        let open_owned = orders.iter().any(|order| {
            order.order_status.is_open()
                && order
                    .order_link_id
                    .as_deref()
                    .is_some_and(valid_owned_order_link_id)
        });
        if open_owned || reconcile_effect != BybitSyncEffect::Applied {
            return Err(BybitTestnetSmokeError::ReconciliationFailed);
        }
        let counts = restarted_sync.evidence_counts().await?;
        let report = BybitTestnetSmokeReport {
            schema_version: BYBIT_TESTNET_PROTOCOL_SMOKE_VERSION_V1,
            run_id: run_id.clone(),
            symbol: BYBIT_TESTNET_SYMBOL,
            cancel_probe_order_link_id: cancel_link,
            fill_probe_order_link_id: fill_link,
            emergency_exit_order_link_id: emergency_link,
            duplicate_effect: "DUPLICATE_NO_EFFECT",
            rest_ack_count: counts.order_acks(),
            private_event_count: counts.private_events(),
            private_execution_count: counts.executions(),
            restart_reconciled: true,
            emergency_converged: true,
            max_order_quote: BYBIT_TESTNET_MAX_ORDER_QUOTE.to_string(),
        };
        persist_evidence(
            &restarted,
            &run_id,
            "PROTOCOL_SMOKE_COMPLETED",
            &serde_json::to_value(&report).map_err(BybitTestnetSmokeError::Serialize)?,
        )
        .await?;
        restarted.close().await;
        Ok(report)
    }
    .await;
    match result {
        Ok(report) => Ok(report),
        Err(primary) => match cleanup_owned_testnet_state(&client).await {
            Ok(_) => Err(primary),
            Err(cleanup) => Err(BybitTestnetSmokeError::CleanupFailed {
                primary: primary.to_string().into_boxed_str(),
                cleanup: cleanup.to_string().into_boxed_str(),
            }),
        },
    }
}

pub async fn recover_bybit_testnet_owned_orders(
    api_key: String,
    api_secret: String,
    write_authorization: &str,
) -> Result<BybitTestnetRecoveryReport, BybitTestnetSmokeError> {
    if api_key.trim().is_empty()
        || api_secret.trim().is_empty()
        || write_authorization != BYBIT_TESTNET_WRITE_AUTHORIZATION_VALUE
    {
        return Err(BybitTestnetSmokeError::MissingAuthorization);
    }
    let client = SdkClient::new(SdkHttpConfig {
        base_url: BASE_URL_API_TESTNET.to_owned(),
        api_key: Some(SensitiveString::from(api_key.as_str())),
        api_secret: Some(SensitiveString::from(api_secret.as_str())),
        recv_window: 5_000,
        referer: None,
    })
    .map_err(BybitTestnetSmokeError::Sdk)?;
    let key = sdk_timeout(client.get_api_key_information()).await?.result;
    if key.read_only != 0
        || !key
            .permissions
            .spot
            .iter()
            .any(|value| value == "SpotTrade")
    {
        return Err(BybitTestnetSmokeError::InsufficientKeyPermission);
    }
    let cancelled = cancel_owned_open_orders(&client).await?;
    let history = sdk_timeout(
        client.get_order_history_all(
            &GetOrderHistoryParams::new(Category::Spot)
                .with_symbol(BYBIT_TESTNET_SYMBOL.to_owned())
                .with_limit(50),
        ),
    )
    .await?;
    let owned_orders = history
        .into_iter()
        .filter(|order| {
            order
                .order_link_id
                .as_deref()
                .is_some_and(valid_owned_order_link_id)
        })
        .map(|order| BybitTestnetRecoveryOrder {
            order_link_id: order.order_link_id.unwrap_or_default(),
            order_status: format!("{:?}", order.order_status),
            cumulative_executed_quantity: order.cum_exec_qty.to_string(),
            cumulative_executed_value: order.cum_exec_value.to_string(),
        })
        .collect();
    let executions = sdk_timeout(
        client.get_execution_list_all(
            &GetExecutionListParams::new(Category::Spot)
                .with_symbol(BYBIT_TESTNET_SYMBOL.to_owned())
                .with_limit(100),
        ),
    )
    .await?;
    let owned_executions: Vec<_> = executions
        .into_iter()
        .filter(|execution| {
            execution
                .order_link_id
                .as_deref()
                .is_some_and(valid_owned_order_link_id)
        })
        .collect();
    let owned_buy_execution_quantity = owned_executions
        .iter()
        .filter(|execution| execution.side == Side::Buy)
        .map(|execution| execution.exec_qty)
        .sum::<Decimal>();
    let owned_sell_execution_quantity = owned_executions
        .iter()
        .filter(|execution| execution.side == Side::Sell)
        .map(|execution| execution.exec_qty)
        .sum::<Decimal>();
    Ok(BybitTestnetRecoveryReport {
        cancelled_open_orders: cancelled,
        owned_orders,
        owned_execution_count: u64::try_from(owned_executions.len())
            .map_err(|_| BybitTestnetSmokeError::UnsafeOrder)?,
        owned_buy_execution_quantity: owned_buy_execution_quantity.to_string(),
        owned_sell_execution_quantity: owned_sell_execution_quantity.to_string(),
    })
}

async fn cancel_owned_open_orders(client: &SdkClient) -> Result<u64, BybitTestnetSmokeError> {
    let orders = sdk_timeout(
        client.get_open_closed_orders_all(
            &GetOpenClosedOrdersParams::new(Category::Spot)
                .with_symbol(BYBIT_TESTNET_SYMBOL.to_owned())
                .with_open_only(0)
                .with_limit(50),
        ),
    )
    .await?;
    let mut cancelled = 0_u64;
    for order in orders.iter().filter(|order| {
        order.order_status.is_open()
            && order
                .order_link_id
                .as_deref()
                .is_some_and(valid_owned_order_link_id)
    }) {
        sdk_timeout(
            client.cancel_order(
                &CancelOrderRequest::new(Category::Spot, BYBIT_TESTNET_SYMBOL.to_owned())
                    .with_order_id(order.order_id.clone()),
            ),
        )
        .await?;
        cancelled += 1;
    }
    Ok(cancelled)
}

async fn cleanup_owned_testnet_state(client: &SdkClient) -> Result<(), BybitTestnetSmokeError> {
    cancel_owned_open_orders(client).await?;
    let executions = sdk_timeout(
        client.get_execution_list_all(
            &GetExecutionListParams::new(Category::Spot)
                .with_symbol(BYBIT_TESTNET_SYMBOL.to_owned())
                .with_limit(100),
        ),
    )
    .await?;
    let (bought, sold) = executions
        .iter()
        .filter(|execution| {
            execution
                .order_link_id
                .as_deref()
                .is_some_and(valid_owned_order_link_id)
        })
        .fold(
            (Decimal::ZERO, Decimal::ZERO),
            |(bought, sold), execution| match execution.side {
                Side::Buy => (
                    bought + (execution.exec_qty - execution.exec_fee).max(Decimal::ZERO),
                    sold,
                ),
                Side::Sell => (bought, sold + execution.exec_qty),
            },
        );
    let net_managed = bought - sold;
    if net_managed <= Decimal::ZERO {
        return Ok(());
    }
    let (instrument, bid, _) = market_contract(client).await?;
    let quantity = quantize_down(net_managed, instrument.lot_size_filter.base_precision)?;
    if quantity < instrument.lot_size_filter.min_order_qty {
        return Ok(());
    }
    let recovery_link = owned_link(&Uuid::new_v4().simple().to_string(), 'r');
    let recovery_order = planned_order(
        AccountOrderSide::Sell,
        AiOrderType::Market,
        quantity,
        None,
        None,
    )?;
    let mapping =
        map_planned_spot_order_to_testnet(BYBIT_TESTNET_SYMBOL, &recovery_link, &recovery_order)?;
    enforce_quote_cap(mapping.request(), bid)?;
    sdk_timeout(client.place_order(mapping.request())).await?;
    Ok(())
}

fn authorize(
    api_key: &str,
    api_secret: &str,
    authorization: &str,
    socks5_proxy: &str,
) -> Result<(), BybitTestnetSmokeError> {
    let proxy = std::net::SocketAddr::from_str(socks5_proxy)
        .map_err(|_| BybitTestnetSmokeError::UnsafeProxy)?;
    if api_key.trim().is_empty()
        || api_secret.trim().is_empty()
        || authorization != BYBIT_TESTNET_WRITE_AUTHORIZATION_VALUE
    {
        Err(BybitTestnetSmokeError::MissingAuthorization)
    } else if !proxy.ip().is_loopback() {
        Err(BybitTestnetSmokeError::UnsafeProxy)
    } else {
        Ok(())
    }
}

async fn preflight(client: &SdkClient) -> Result<(), BybitTestnetSmokeError> {
    sdk_timeout(client.get_server_time()).await?;
    let key = sdk_timeout(client.get_api_key_information()).await?.result;
    if key.read_only != 0
        || !key
            .permissions
            .spot
            .iter()
            .any(|value| value == "SpotTrade")
    {
        return Err(BybitTestnetSmokeError::InsufficientKeyPermission);
    }
    let wallet = wallet(client).await?;
    let usdt = wallet.coin.get("USDT");
    let wallet_quantity = usdt.map_or(Decimal::ZERO, |coin| coin.wallet_balance);
    let locked_quantity = usdt.map_or(Decimal::ZERO, |coin| coin.locked);
    let available = wallet_quantity - locked_quantity;
    if available < BYBIT_TESTNET_MAX_ORDER_QUOTE {
        let mut assets: Vec<String> = wallet
            .coin
            .values()
            .filter(|coin| coin.wallet_balance != Decimal::ZERO || coin.locked != Decimal::ZERO)
            .map(|coin| {
                format!(
                    "{}:wallet={},locked={}",
                    coin.coin, coin.wallet_balance, coin.locked
                )
            })
            .collect();
        assets.sort();
        return Err(BybitTestnetSmokeError::InsufficientTestFunds {
            uta: key.uta,
            total_wallet_balance: wallet.total_wallet_balance,
            wallet_quantity,
            locked_quantity,
            assets,
        });
    }
    Ok(())
}

async fn wallet(
    client: &SdkClient,
) -> Result<bybit_sdk::http::WalletBalance, BybitTestnetSmokeError> {
    sdk_timeout(client.get_wallet_balance(&GetWalletBalanceParams {
        account_type: AccountType::UNIFIED,
        coin: None,
    }))
    .await?
    .result
    .list
    .into_iter()
    .next()
    .ok_or(BybitTestnetSmokeError::MissingMarketFact)
}

async fn market_contract(
    client: &SdkClient,
) -> Result<(SpotInstrumentsInfo, Decimal, Decimal), BybitTestnetSmokeError> {
    let response = sdk_timeout(client.get_instruments_info(&GetInstrumentsInfoParams {
        category: Category::Spot,
        symbol: Some(BYBIT_TESTNET_SYMBOL.to_owned()),
        status: None,
        base_coin: None,
        limit: Some(1),
        cursor: None,
    }))
    .await?;
    let InstrumentsInfo::Spot { mut list, .. } = response.result else {
        return Err(BybitTestnetSmokeError::MissingMarketFact);
    };
    let instrument = list
        .pop()
        .ok_or(BybitTestnetSmokeError::MissingMarketFact)?;
    let response = sdk_timeout(client.get_tickers(&GetTickersParams {
        category: Category::Spot,
        symbol: Some(BYBIT_TESTNET_SYMBOL.to_owned()),
        base_coin: None,
        exp_date: None,
    }))
    .await?;
    let Ticker::Spot { mut list } = response.result else {
        return Err(BybitTestnetSmokeError::MissingMarketFact);
    };
    let ticker = list
        .pop()
        .ok_or(BybitTestnetSmokeError::MissingMarketFact)?;
    if ticker.bid1_price <= Decimal::ZERO || ticker.ask1_price <= ticker.bid1_price {
        return Err(BybitTestnetSmokeError::MissingMarketFact);
    }
    Ok((instrument, ticker.bid1_price, ticker.ask1_price))
}

async fn authenticate_private_stream(
    handle: &bybit_sdk::ws::Handle,
    events: &mut Receiver<SdkEvent>,
    sync: &SqliteBybitPrivateSync<'_>,
    api_key: &str,
    api_secret: &str,
) -> Result<(), BybitTestnetSmokeError> {
    let deadline = time::Instant::now() + SDK_CALL_TIMEOUT;
    let mut sent = false;
    let mut authenticated = false;
    let mut subscribed = false;
    while time::Instant::now() < deadline && !(authenticated && subscribed) {
        let event = time::timeout(
            deadline.saturating_duration_since(time::Instant::now()),
            events.recv(),
        )
        .await
        .map_err(|_| BybitTestnetSmokeError::PrivateStreamTimeout)?
        .ok_or(BybitTestnetSmokeError::PrivateStreamClosed)?;
        if matches!(event, SdkEvent::Connected) && !sent {
            handle
                .send_command(create_outgoing_message_auth(
                    SensitiveString::from(api_key),
                    SensitiveString::from(api_secret),
                    Some("ip4-auth".to_owned()),
                    5_000,
                ))
                .await?;
            handle
                .send_command(OutgoingMessage::Subscribe {
                    req_id: Some("ip4-private".to_owned()),
                    args: vec![
                        Topic::OrderAllCategory,
                        Topic::ExecutionAllCategory,
                        Topic::Wallet,
                    ],
                })
                .await?;
            sent = true;
        }
        match &event {
            SdkEvent::Message(IncomingMessage::Command(CommandMsg::Auth {
                success: true, ..
            })) => authenticated = true,
            SdkEvent::Message(IncomingMessage::Command(CommandMsg::Subscribe {
                success: true,
                ..
            })) => subscribed = true,
            _ => {}
        }
        sync.ingest_sdk_event(event, now_millis()?).await?;
    }
    if authenticated && subscribed {
        Ok(())
    } else {
        Err(BybitTestnetSmokeError::PrivateStreamTimeout)
    }
}

async fn pump_private_events(
    events: &mut Receiver<SdkEvent>,
    sync: &SqliteBybitPrivateSync<'_>,
    window: Duration,
) -> Result<(), BybitTestnetSmokeError> {
    let deadline = time::Instant::now() + window;
    loop {
        let remaining = deadline.saturating_duration_since(time::Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        match time::timeout(remaining.min(Duration::from_millis(500)), events.recv()).await {
            Ok(Some(event)) => {
                sync.ingest_sdk_event(event, now_millis()?).await?;
            }
            Ok(None) => return Err(BybitTestnetSmokeError::PrivateStreamClosed),
            Err(_) if time::Instant::now() >= deadline => return Ok(()),
            Err(_) => {}
        }
    }
}

async fn submit_once(
    repository: &SqliteRepository,
    client: &SdkClient,
    sync: &SqliteBybitPrivateSync<'_>,
    run_id: &str,
    purpose: &str,
    order_link_id: &str,
    mapping: &TestnetOrderMapping,
) -> Result<TestnetSubmissionEffect, BybitTestnetSmokeError> {
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT sdk_request_payload_hash FROM bybit_testnet_order_intents WHERE order_link_id = ?",
    )
    .bind(order_link_id)
    .fetch_optional(&repository.pool)
    .await?;
    if let Some(existing) = existing {
        return if existing == mapping.sdk_payload_hash() {
            Ok(TestnetSubmissionEffect::DuplicateNoEffect)
        } else {
            Err(BybitTestnetSmokeError::IdempotencyConflict)
        };
    }
    let acknowledged_at = now_millis()?;
    let ack = sdk_timeout(client.place_order(mapping.request()))
        .await?
        .result;
    sync.record_sdk_order_ack(&ack, acknowledged_at).await?;
    let _guard = repository.write_gate.lock().await;
    sqlx::query("INSERT INTO bybit_testnet_order_intents VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(order_link_id)
        .bind(run_id)
        .bind(purpose)
        .bind(mapping.source_payload_hash())
        .bind(mapping.sdk_payload_hash())
        .bind(mapping.sdk_payload_json())
        .bind(&ack.order_id)
        .bind(i64::try_from(acknowledged_at).map_err(|_| BybitTestnetSmokeError::Clock)?)
        .execute(&repository.pool)
        .await?;
    Ok(TestnetSubmissionEffect::Submitted)
}

async fn query_order(
    client: &SdkClient,
    order_link_id: &str,
) -> Result<(), BybitTestnetSmokeError> {
    let response = sdk_timeout(
        client.get_open_closed_orders(
            &GetOpenClosedOrdersParams::new(Category::Spot)
                .with_symbol(BYBIT_TESTNET_SYMBOL.to_owned())
                .with_order_link_id(order_link_id.to_owned())
                .with_limit(1),
        ),
    )
    .await?;
    if response.result.list.iter().any(|order| {
        order.order_link_id.as_deref() == Some(order_link_id)
            && order.order_status != OrderStatus::Rejected
    }) {
        Ok(())
    } else {
        Err(BybitTestnetSmokeError::OrderQueryFailed)
    }
}

async fn managed_bought_quantity(
    repository: &SqliteRepository,
    order_link_id: &str,
) -> Result<Decimal, BybitTestnetSmokeError> {
    let rows = sqlx::query(
        "SELECT quantity, fee_quantity, fee_asset FROM bybit_execution_facts \
         WHERE order_link_id = ? AND side = 'BUY'",
    )
    .bind(order_link_id)
    .fetch_all(&repository.pool)
    .await?;
    let mut quantity = Decimal::ZERO;
    for row in rows {
        let fill = Decimal::from_str(&row.try_get::<String, _>("quantity")?)
            .map_err(|_| BybitTestnetSmokeError::CorruptEvidence)?;
        let fee = Decimal::from_str(&row.try_get::<String, _>("fee_quantity")?)
            .map_err(|_| BybitTestnetSmokeError::CorruptEvidence)?;
        quantity += if row.try_get::<String, _>("fee_asset")? == "BTC" {
            fill - fee
        } else {
            fill
        };
    }
    if quantity > Decimal::ZERO {
        Ok(quantity)
    } else {
        Err(BybitTestnetSmokeError::MissingPrivateEvidence)
    }
}

#[allow(clippy::too_many_arguments)]
async fn emergency_close(
    repository: &SqliteRepository,
    client: &SdkClient,
    sync: &SqliteBybitPrivateSync<'_>,
    run_id: &str,
    order_link_id: &str,
    managed_quantity: Decimal,
    bid: Decimal,
    command: &AuthorizedEmergencyCommand,
) -> Result<(), BybitTestnetSmokeError> {
    if now_millis()? >= command.expires_at_unix_millis()
        || command.kind() != EmergencyCommandKind::CloseAllManagedExposure
    {
        return Err(BybitTestnetSmokeError::Emergency);
    }
    let orders = sdk_timeout(
        client.get_open_closed_orders_all(
            &GetOpenClosedOrdersParams::new(Category::Spot)
                .with_symbol(BYBIT_TESTNET_SYMBOL.to_owned())
                .with_open_only(0)
                .with_limit(50),
        ),
    )
    .await?;
    for order in orders.iter().filter(|order| {
        order.order_status.is_open()
            && order
                .order_link_id
                .as_deref()
                .is_some_and(valid_owned_order_link_id)
    }) {
        sdk_timeout(
            client.cancel_order(
                &CancelOrderRequest::new(Category::Spot, BYBIT_TESTNET_SYMBOL.to_owned())
                    .with_order_id(order.order_id.clone()),
            ),
        )
        .await?;
    }
    let exit = planned_order(
        AccountOrderSide::Sell,
        AiOrderType::Market,
        managed_quantity,
        None,
        None,
    )?;
    let mapping = map_planned_spot_order_to_testnet(BYBIT_TESTNET_SYMBOL, order_link_id, &exit)?;
    enforce_quote_cap(mapping.request(), bid)?;
    submit_once(
        repository,
        client,
        sync,
        run_id,
        "EMERGENCY_EXIT",
        order_link_id,
        &mapping,
    )
    .await?;
    persist_evidence(
        repository,
        run_id,
        "EMERGENCY_COMMAND_ACCEPTED",
        &json!({
            "schema_version": BYBIT_TESTNET_PROTOCOL_SMOKE_VERSION_V1,
            "command_hash": command.command_hash().to_string(),
            "authorization_evidence_hash": hex(command.authorization_evidence_hash()),
            "managed_quantity": managed_quantity.to_string(),
            "order_link_id": order_link_id
        }),
    )
    .await
}

async fn persist_evidence(
    repository: &SqliteRepository,
    run_id: &str,
    kind: &str,
    payload: &Value,
) -> Result<(), BybitTestnetSmokeError> {
    let observed_at = now_millis()?;
    let payload_json = serde_json::to_string(payload).map_err(BybitTestnetSmokeError::Serialize)?;
    let evidence_id = format!("{run_id}:{kind}");
    let _guard = repository.write_gate.lock().await;
    sqlx::query("INSERT INTO bybit_testnet_smoke_evidence VALUES (?, ?, ?, ?, ?, ?)")
        .bind(evidence_id)
        .bind(run_id)
        .bind(kind)
        .bind(i64::try_from(observed_at).map_err(|_| BybitTestnetSmokeError::Clock)?)
        .bind(sha256_hex(payload_json.as_bytes()))
        .bind(payload_json)
        .execute(&repository.pool)
        .await?;
    Ok(())
}

fn planned_order(
    side: AccountOrderSide,
    order_type: AiOrderType,
    quantity: Decimal,
    limit_price: Option<Decimal>,
    time_in_force: Option<AiTimeInForce>,
) -> Result<PlannedSpotOrder, BybitTestnetSmokeError> {
    let now = now_millis()?;
    PlannedSpotOrder::from_persisted(
        ExecutionOrderIds::new(
            OrderIntentId::new(Uuid::new_v4()).map_err(|_| BybitTestnetSmokeError::UnsafeOrder)?,
            OrderId::new(Uuid::new_v4()).map_err(|_| BybitTestnetSmokeError::UnsafeOrder)?,
        ),
        if side == AccountOrderSide::Buy {
            ExecutionOrderRole::Entry
        } else {
            ExecutionOrderRole::Exit
        },
        side,
        order_type,
        Some(domain_decimal(quantity)?),
        limit_price.map(domain_decimal).transpose()?,
        None,
        time_in_force,
        now + 60_000,
        DomainDecimal::ZERO,
    )
    .map_err(|_| BybitTestnetSmokeError::UnsafeOrder)
}

fn safe_base_quantity(
    instrument: &SpotInstrumentsInfo,
    reference_price: Decimal,
    target_quote: Decimal,
) -> Result<Decimal, BybitTestnetSmokeError> {
    if reference_price <= Decimal::ZERO || target_quote <= Decimal::ZERO {
        return Err(BybitTestnetSmokeError::UnsafeOrder);
    }
    let mut quantity = (target_quote / reference_price).round_dp_with_strategy(
        instrument
            .lot_size_filter
            .base_precision
            .normalize()
            .scale(),
        RoundingStrategy::AwayFromZero,
    );
    quantity = quantity.max(instrument.lot_size_filter.min_order_qty);
    if quantity * reference_price > BYBIT_TESTNET_MAX_ORDER_QUOTE {
        return Err(BybitTestnetSmokeError::QuoteCapExceeded);
    }
    Ok(quantity)
}

fn enforce_quote_cap(
    request: &PlaceOrderRequest,
    reference_price: Decimal,
) -> Result<(), BybitTestnetSmokeError> {
    if request.category != Category::Spot
        || request.symbol != BYBIT_TESTNET_SYMBOL
        || request.is_leverage != Some(0)
        || request
            .order_link_id
            .as_deref()
            .is_none_or(|value| !valid_owned_order_link_id(value))
        || request.qty * reference_price > BYBIT_TESTNET_MAX_ORDER_QUOTE
    {
        Err(BybitTestnetSmokeError::QuoteCapExceeded)
    } else {
        Ok(())
    }
}

fn quantize_down(value: Decimal, step: Decimal) -> Result<Decimal, BybitTestnetSmokeError> {
    if value <= Decimal::ZERO || step <= Decimal::ZERO {
        Err(BybitTestnetSmokeError::MissingMarketFact)
    } else {
        Ok((value / step).floor() * step)
    }
}

fn decimal(value: DomainDecimal) -> Result<Decimal, BybitTestnetSmokeError> {
    Decimal::from_str(&value.to_string()).map_err(|_| BybitTestnetSmokeError::UnsafeOrder)
}

fn domain_decimal(value: Decimal) -> Result<DomainDecimal, BybitTestnetSmokeError> {
    DomainDecimal::from_str(&value.to_string()).map_err(|_| BybitTestnetSmokeError::UnsafeOrder)
}

fn owned_link(run_id: &str, purpose: char) -> String {
    format!(
        "{BYBIT_TESTNET_ORDER_LINK_PREFIX}{purpose}-{}",
        &run_id[..30]
    )
}

fn valid_owned_order_link_id(value: &str) -> bool {
    value.starts_with(BYBIT_TESTNET_ORDER_LINK_PREFIX)
        && value.len() <= 36
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn now_millis() -> Result<u64, BybitTestnetSmokeError> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| BybitTestnetSmokeError::Clock)?
            .as_millis(),
    )
    .map_err(|_| BybitTestnetSmokeError::Clock)
}

async fn sdk_timeout<T>(
    future: impl Future<Output = Result<T, bybit_sdk::Error>>,
) -> Result<T, BybitTestnetSmokeError> {
    time::timeout(SDK_CALL_TIMEOUT, future)
        .await
        .map_err(|_| BybitTestnetSmokeError::SdkTimeout)?
        .map_err(BybitTestnetSmokeError::Sdk)
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes).into())
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug)]
pub enum BybitTestnetSmokeError {
    MissingAuthorization,
    UnsafeProxy,
    InsufficientKeyPermission,
    InsufficientTestFunds {
        uta: i64,
        total_wallet_balance: Decimal,
        wallet_quantity: Decimal,
        locked_quantity: Decimal,
        assets: Vec<String>,
    },
    MissingMarketFact,
    UnsafeOrder,
    QuoteCapExceeded,
    IdempotencyConflict,
    IdempotencyFailed,
    OrderQueryFailed,
    MissingPrivateEvidence,
    PrivateStreamTimeout,
    PrivateStreamClosed,
    ReconciliationFailed,
    Emergency,
    CorruptEvidence,
    Clock,
    SdkTimeout,
    Sdk(bybit_sdk::Error),
    WebSocket(bybit_sdk::ws::Error),
    Storage(crate::StorageError),
    PrivateSync(crate::BybitPrivateSyncError),
    Sqlx(sqlx::Error),
    Serialize(serde_json::Error),
    CleanupFailed {
        primary: Box<str>,
        cleanup: Box<str>,
    },
}

impl fmt::Display for BybitTestnetSmokeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::MissingAuthorization => "explicit P4-02A Testnet write authorization is missing",
            Self::UnsafeProxy => "private WebSocket proxy must be an explicit loopback socket",
            Self::InsufficientKeyPermission => "API key is not read-write SpotTrade",
            Self::InsufficientTestFunds {
                uta,
                total_wallet_balance,
                wallet_quantity,
                locked_quantity,
                assets,
            } => {
                return write!(
                    formatter,
                    "Testnet API wallet has insufficient unlocked USDT \
                     (uta={uta}, total_wallet_balance={total_wallet_balance}, \
                     USDT wallet={wallet_quantity}, locked={locked_quantity}, \
                     nonzero_assets={assets:?})"
                );
            }
            Self::MissingMarketFact => "required Testnet market fact is missing",
            Self::UnsafeOrder => "order violates the bounded Testnet Spot contract",
            Self::QuoteCapExceeded => "order exceeds the 10 USDT hard cap",
            Self::IdempotencyConflict => "orderLinkId was reused with different fields",
            Self::IdempotencyFailed => "duplicate order produced an effect",
            Self::OrderQueryFailed => "placed order was not queryable",
            Self::MissingPrivateEvidence => "private order/execution evidence did not arrive",
            Self::PrivateStreamTimeout => "private stream authentication/subscription timed out",
            Self::PrivateStreamClosed => "private stream closed",
            Self::ReconciliationFailed => "restart reconciliation did not converge",
            Self::Emergency => "Emergency authorization or convergence failed",
            Self::CorruptEvidence => "stored Testnet evidence is corrupt",
            Self::Clock => "system clock is invalid",
            Self::SdkTimeout => "Bybit SDK call timed out",
            Self::Sdk(error) => return write!(formatter, "Bybit SDK error: {error}"),
            Self::WebSocket(error) => {
                return write!(formatter, "Bybit SDK WebSocket error: {error}");
            }
            Self::Storage(error) => return write!(formatter, "storage error: {error}"),
            Self::PrivateSync(error) => return write!(formatter, "private sync error: {error}"),
            Self::Sqlx(error) => return write!(formatter, "database error: {error}"),
            Self::Serialize(error) => return write!(formatter, "serialization error: {error}"),
            Self::CleanupFailed { primary, cleanup } => {
                return write!(
                    formatter,
                    "Testnet smoke failed ({primary}) and owned-order cleanup failed ({cleanup})"
                );
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for BybitTestnetSmokeError {}

impl From<crate::StorageError> for BybitTestnetSmokeError {
    fn from(value: crate::StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<crate::BybitPrivateSyncError> for BybitTestnetSmokeError {
    fn from(value: crate::BybitPrivateSyncError) -> Self {
        Self::PrivateSync(value)
    }
}

impl From<sqlx::Error> for BybitTestnetSmokeError {
    fn from(value: sqlx::Error) -> Self {
        Self::Sqlx(value)
    }
}

impl From<bybit_sdk::ws::Error> for BybitTestnetSmokeError {
    fn from(value: bybit_sdk::ws::Error) -> Self {
        Self::WebSocket(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn exact_planned_fields_map_to_sdk_spot_request_without_mutation() {
        let order = planned_order(
            AccountOrderSide::Buy,
            AiOrderType::Limit,
            Decimal::new(1, 4),
            Some(Decimal::new(50_000, 0)),
            Some(AiTimeInForce::Gtc),
        )
        .expect("fixture");
        let mapping = map_planned_spot_order_to_testnet(
            BYBIT_TESTNET_SYMBOL,
            "ip4-c-012345678901234567890123456789",
            &order,
        )
        .expect("mapping");
        let request = mapping.request();
        assert_eq!(request.category, Category::Spot);
        assert_eq!(request.symbol, BYBIT_TESTNET_SYMBOL);
        assert_eq!(request.side, Side::Buy);
        assert_eq!(request.order_type, OrderType::Limit);
        assert_eq!(request.qty, Decimal::new(1, 4));
        assert_eq!(request.price, Some(Decimal::new(50_000, 0)));
        assert_eq!(request.time_in_force, Some(TimeInForce::GTC));
        assert_eq!(request.is_leverage, Some(0));
        assert_eq!(request.market_unit.as_deref(), Some("baseCoin"));
    }

    #[test]
    fn non_testnet_symbol_and_foreign_order_link_fail_closed() {
        let order = planned_order(
            AccountOrderSide::Buy,
            AiOrderType::Market,
            Decimal::new(1, 4),
            None,
            None,
        )
        .expect("fixture");
        assert!(map_planned_spot_order_to_testnet("ETHUSDT", "ip4-f-safe", &order).is_err());
        assert!(
            map_planned_spot_order_to_testnet(BYBIT_TESTNET_SYMBOL, "foreign-order", &order)
                .is_err()
        );
    }

    #[test]
    fn quote_cap_is_independent_of_sdk_request_construction() {
        let order = planned_order(
            AccountOrderSide::Buy,
            AiOrderType::Market,
            Decimal::ONE,
            None,
            None,
        )
        .expect("fixture");
        let mapping = map_planned_spot_order_to_testnet(BYBIT_TESTNET_SYMBOL, "ip4-f-safe", &order)
            .expect("mapping");
        assert!(enforce_quote_cap(mapping.request(), Decimal::new(11, 0)).is_err());
    }

    #[test]
    fn sdk_parses_current_testnet_spot_order_event() {
        let json = r#"{"topic":"order","id":"test-order-event","creationTime":1766600379878,"data":[{"category":"spot","symbol":"BTCUSDT","orderId":"test-order-id","orderLinkId":"ip4-c-012345678901234567890123456789","blockTradeId":"","side":"Buy","positionIdx":0,"orderStatus":"New","cancelType":"UNKNOWN","rejectReason":"EC_NoError","timeInForce":"GTC","isLeverage":"0","price":"64200.1","qty":"0.000078","avgPrice":"","leavesQty":"0.000078","leavesValue":"5.0076078","cumExecQty":"0","cumExecValue":"0.0000000","cumExecFee":"0","orderType":"Limit","stopOrderType":"","orderIv":"","triggerPrice":"0.0","takeProfit":"0.0","stopLoss":"0.0","triggerBy":"","tpTriggerBy":"","slTriggerBy":"","triggerDirection":0,"placeType":"","lastPriceOnCreated":"64848.6","closeOnTrigger":false,"reduceOnly":false,"smpGroup":0,"smpType":"None","smpOrderId":"","slLimitPrice":"0.0","tpLimitPrice":"0.0","marketUnit":"","createdTime":"1766600379876","updatedTime":"1766600379876","feeCurrency":"","slippageTolerance":"","slippageToleranceType":"UNKNOWN","cumFeeDetail":{},"rpiTakerAccess":false,"rpiMatchedQty":"0"}]}"#;

        let message: IncomingMessage =
            serde_json::from_str(json).expect("current Testnet private order");
        let IncomingMessage::Topic(bybit_sdk::ws::TopicMessage::Order(message)) = message else {
            panic!("expected private order topic");
        };
        assert_eq!(message.data.len(), 1);
        assert_eq!(message.data[0].category, Category::Spot);
        assert_eq!(message.data[0].order_status, OrderStatus::New);
        assert_eq!(message.data[0].closed_pnl, Decimal::ZERO);
        assert_eq!(message.data[0].fee_currency, None);
    }

    #[test]
    fn sdk_parses_current_testnet_spot_execution_event() {
        let json = r#"{"topic":"execution","id":"test-execution-event","creationTime":1766600379878,"data":[{"category":"spot","symbol":"BTCUSDT","closedSize":"","execFee":"0.000000081","execId":"test-execution-id","execPrice":"64840.8","execQty":"0.000081","execType":"Trade","execValue":"5.2521048","feeRate":"0.001","tradeIv":"","markIv":"","blockTradeId":"","markPrice":"","indexPrice":"","underlyingPrice":"","leavesQty":"0","orderId":"test-order-id","orderLinkId":"ip4-f-012345678901234567890123456789","orderPrice":"64840.8","orderQty":"0.000081","orderType":"Market","stopOrderType":"","side":"Buy","execTime":"1766600379876","isLeverage":"0","isMaker":false,"seq":140612148849382,"marketUnit":"baseCoin","execPnl":"","extraFees":"","feeCurrency":"BTC"}]}"#;

        let message: IncomingMessage =
            serde_json::from_str(json).expect("current Testnet private execution");
        let IncomingMessage::Topic(bybit_sdk::ws::TopicMessage::Execution(message)) = message
        else {
            panic!("expected private execution topic");
        };
        assert_eq!(message.data.len(), 1);
        assert_eq!(message.data[0].category, Category::Spot);
        assert_eq!(message.data[0].exec_qty, Decimal::new(81, 6));
        assert_eq!(message.data[0].create_type, None);
        assert_eq!(message.data[0].extra_fees, None);
        assert_eq!(message.data[0].fee_currency, "BTC");
    }

    #[tokio::test]
    async fn sdk_accepts_success_envelope_when_proxy_strips_ret_code_header() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("read");
            let body = r#"{"retCode":0,"retMsg":"OK","result":{"timeSecond":"1","timeNano":"1000000000"},"retExtInfo":{},"time":1000}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("write response");
        });
        let client = SdkClient::new(SdkHttpConfig {
            base_url: format!("http://{address}"),
            api_key: None,
            api_secret: None,
            recv_window: 5_000,
            referer: None,
        })
        .expect("client");

        let response = client
            .get_server_time()
            .await
            .expect("body retCode=0 must be authoritative");
        server.join().expect("server");
        assert_eq!(response.result.time_second, 1);
        assert_eq!(response.headers.ret_code, None);
    }
}
