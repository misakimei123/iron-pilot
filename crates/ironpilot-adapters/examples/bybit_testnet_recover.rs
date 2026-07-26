use std::process::ExitCode;

use ironpilot_adapters::{
    BYBIT_TESTNET_API_KEY_ENV, BYBIT_TESTNET_API_SECRET_ENV, BYBIT_TESTNET_WRITE_AUTHORIZATION_ENV,
    recover_bybit_testnet_owned_orders,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let result = async {
        let api_key = std::env::var(BYBIT_TESTNET_API_KEY_ENV)?;
        let api_secret = std::env::var(BYBIT_TESTNET_API_SECRET_ENV)?;
        let authorization = std::env::var(BYBIT_TESTNET_WRITE_AUTHORIZATION_ENV)?;
        let report =
            recover_bybit_testnet_owned_orders(api_key, api_secret, &authorization).await?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("P4-02A recovery failed: {error}");
            ExitCode::FAILURE
        }
    }
}
