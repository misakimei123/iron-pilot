use core::str::FromStr;
use std::path::PathBuf;

use bybit_sdk::{
    OrderStatus,
    http::PlaceOrderResponse,
    ws::{Event, IncomingMessage, TopicMessage},
};
use ironpilot_adapters::{
    BYBIT_PRIVATE_COMMAND_QUEUE_SIZE, BYBIT_PRIVATE_EVENT_QUEUE_SIZE,
    BYBIT_PRIVATE_MAX_RECONNECT_ATTEMPTS, BybitOrderFact, BybitPrivateSyncError,
    BybitReconciliationSnapshot, BybitSyncEffect, BybitWalletFact, SqliteBybitPrivateSync,
    SqliteRepository, bybit_private_sdk_config,
};
use ironpilot_domain::{
    AssetCode, DomainDecimal, LocalAssetBalance, PortfolioReconciliationStatus,
};
use uuid::Uuid;

fn database_path() -> PathBuf {
    std::env::temp_dir().join(format!("ironpilot-p4-01-{}.sqlite3", Uuid::new_v4()))
}

fn sdk_event(fixture: &str) -> Event {
    let message: IncomingMessage =
        serde_json::from_str(fixture).expect("fixture must be decoded by the selected Bybit SDK");
    Event::Message(message)
}

fn execution_event_with_message_id(message_id: &str) -> Event {
    let message: IncomingMessage =
        serde_json::from_str(execution_message()).expect("SDK execution fixture");
    let IncomingMessage::Topic(TopicMessage::Execution(mut message)) = message else {
        panic!("execution fixture shape");
    };
    message.id = message_id.to_owned();
    Event::Message(IncomingMessage::Topic(TopicMessage::Execution(message)))
}

fn order_message() -> &'static str {
    include_str!("fixtures/bybit-ws-private-order-spot.json")
}

fn execution_message() -> &'static str {
    include_str!("fixtures/bybit-ws-private-execution-spot.json")
}

fn wallet_message() -> &'static str {
    include_str!("fixtures/bybit-ws-private-wallet.json")
}

fn decimal(value: &str) -> DomainDecimal {
    DomainDecimal::from_str(value).expect("test decimal must be valid")
}

fn btc_local_balance() -> LocalAssetBalance {
    LocalAssetBalance::new(
        AssetCode::new("BTC").expect("asset"),
        decimal("0.001"),
        decimal("0.001"),
    )
    .expect("local balance")
}

#[test]
fn private_sdk_transport_is_bounded_and_uses_finite_reconnects() {
    let config = bybit_private_sdk_config("wss://stream-testnet.bybit.com/v5/private");
    assert_eq!(config.command_queue_size, BYBIT_PRIVATE_COMMAND_QUEUE_SIZE);
    assert_eq!(config.event_queue_size, BYBIT_PRIVATE_EVENT_QUEUE_SIZE);
    assert_eq!(
        config.max_reconnect_attempts,
        BYBIT_PRIVATE_MAX_RECONNECT_ATTEMPTS
    );
    assert_eq!(config.reconnect_base_delay.as_millis(), 500);
    assert_eq!(config.reconnect_max_delay.as_secs(), 8);
    assert_eq!(config.ping_interval.expect("heartbeat").as_secs(), 20);
    assert_eq!(config.pong_timeout.as_secs(), 10);
}

#[tokio::test]
async fn sdk_private_order_execution_and_wallet_facts_feed_the_next_context() {
    let repository = SqliteRepository::connect(database_path(), 1)
        .await
        .expect("repository");
    let sync = SqliteBybitPrivateSync::new(&repository);
    sync.ingest_sdk_event(Event::Connected, 1_766_600_379_000)
        .await
        .expect("connected");

    assert_eq!(
        sync.ingest_sdk_event(sdk_event(order_message()), 1_766_600_379_878)
            .await
            .expect("order")
            .applied(),
        1
    );
    assert_eq!(
        sync.ingest_sdk_event(sdk_event(execution_message()), 1_766_600_381_000)
            .await
            .expect("execution")
            .applied(),
        1
    );
    assert_eq!(
        sync.ingest_sdk_event(sdk_event(wallet_message()), 1_766_600_382_000)
            .await
            .expect("wallet")
            .applied(),
        1
    );

    let facts = sync
        .load_context_facts(1_766_600_383_000, vec![btc_local_balance()])
        .await
        .expect("trusted context facts");
    assert_eq!(
        facts.portfolio().status(),
        PortfolioReconciliationStatus::Balanced
    );
    assert_eq!(facts.open_orders().len(), 1);
    assert_eq!(
        facts.open_orders()[0].exchange_order_id(),
        "exchange-order-1"
    );
    assert_eq!(
        facts.latest_execution_at_unix_millis(),
        Some(1_766_600_380_999)
    );
}

#[tokio::test]
async fn rest_ack_is_not_a_fill_and_duplicate_private_events_have_zero_effect() {
    let repository = SqliteRepository::connect(database_path(), 1)
        .await
        .expect("repository");
    let sync = SqliteBybitPrivateSync::new(&repository);
    let ack = PlaceOrderResponse {
        order_id: "exchange-order-1".to_owned(),
        order_link_id: "ironpilot-order-1".to_owned(),
    };
    assert_eq!(
        sync.record_sdk_order_ack(&ack, 1_766_600_379_000)
            .await
            .expect("ack"),
        BybitSyncEffect::Applied
    );
    assert_eq!(
        sync.record_sdk_order_ack(&ack, 1_766_600_379_000)
            .await
            .expect("duplicate ack"),
        BybitSyncEffect::DuplicateNoEffect
    );
    let before = sync.evidence_counts().await.expect("counts");
    assert_eq!(before.order_acks(), 1);
    assert_eq!(before.executions(), 0);

    let first = sync
        .ingest_sdk_event(sdk_event(execution_message()), 1_766_600_381_000)
        .await
        .expect("execution");
    let duplicate = sync
        .ingest_sdk_event(
            execution_event_with_message_id("different-delivery-id"),
            1_766_600_381_000,
        )
        .await
        .expect("duplicate execution");
    assert_eq!(first.applied(), 1);
    assert_eq!(duplicate.duplicate(), 1);
    let after = sync.evidence_counts().await.expect("counts");
    assert_eq!(after.executions(), 1);
    assert_eq!(after.private_events(), 1);
}

#[tokio::test]
async fn disconnect_requires_rest_reconciliation_and_complete_snapshot_converges() {
    let repository = SqliteRepository::connect(database_path(), 1)
        .await
        .expect("repository");
    let sync = SqliteBybitPrivateSync::new(&repository);
    sync.ingest_sdk_event(Event::Connected, 1_766_600_379_000)
        .await
        .expect("connected");
    sync.ingest_sdk_event(sdk_event(order_message()), 1_766_600_379_878)
        .await
        .expect("order");
    sync.ingest_sdk_event(sdk_event(wallet_message()), 1_766_600_382_000)
        .await
        .expect("wallet");
    sync.record_disconnect(1_766_600_383_000)
        .await
        .expect("disconnect");
    assert!(matches!(
        sync.load_context_facts(1_766_600_383_001, vec![btc_local_balance()])
            .await,
        Err(BybitPrivateSyncError::RecoveryRequired)
    ));

    let order_message: IncomingMessage =
        serde_json::from_str(order_message()).expect("SDK order fixture");
    let IncomingMessage::Topic(TopicMessage::Order(mut order_message)) = order_message else {
        panic!("order fixture shape");
    };
    order_message.data[0].order_status = OrderStatus::Filled;
    order_message.data[0].cum_exec_qty = order_message.data[0].qty;
    order_message.data[0].updated_time = 1_766_600_383_500;
    let terminal_order =
        BybitOrderFact::from_sdk_stream(&order_message.data[0]).expect("terminal order fact");

    let wallet_message: IncomingMessage =
        serde_json::from_str(wallet_message()).expect("SDK wallet fixture");
    let IncomingMessage::Topic(TopicMessage::Wallet(wallet_message)) = wallet_message else {
        panic!("wallet fixture shape");
    };
    let coin = wallet_message.data[0].coin.get("BTC").expect("BTC wallet");
    let wallet = BybitWalletFact::from_sdk_stream(
        &coin.coin,
        coin.wallet_balance.to_string(),
        "0".to_owned(),
        1_766_600_383_500,
    )
    .expect("wallet fact");
    let snapshot =
        BybitReconciliationSnapshot::new(1_766_600_383_500, vec![terminal_order], vec![wallet])
            .expect("complete snapshot");
    assert_eq!(
        sync.apply_reconciliation(&snapshot)
            .await
            .expect("reconcile"),
        BybitSyncEffect::Applied
    );
    assert_eq!(
        sync.apply_reconciliation(&snapshot)
            .await
            .expect("duplicate reconcile"),
        BybitSyncEffect::DuplicateNoEffect
    );

    let facts = sync
        .load_context_facts(1_766_600_383_501, vec![btc_local_balance()])
        .await
        .expect("reconciled facts");
    assert!(facts.open_orders().is_empty());
    assert_eq!(
        facts.portfolio().status(),
        PortfolioReconciliationStatus::Balanced
    );
}

#[tokio::test]
async fn sdk_parse_failure_forces_reconciliation_before_context_reuse() {
    let repository = SqliteRepository::connect(database_path(), 1)
        .await
        .expect("repository");
    let sync = SqliteBybitPrivateSync::new(&repository);
    sync.ingest_sdk_event(Event::Connected, 1_766_600_379_000)
        .await
        .expect("connected");
    let error = sync
        .ingest_sdk_event(
            Event::ParseError("malformed private payload".to_owned()),
            1_766_600_379_001,
        )
        .await
        .expect_err("parse failure must fail closed");
    assert!(matches!(error, BybitPrivateSyncError::SdkParse(_)));
    assert!(matches!(
        sync.load_context_facts(1_766_600_379_002, vec![btc_local_balance()])
            .await,
        Err(BybitPrivateSyncError::RecoveryRequired)
    ));
}
