use std::{path::PathBuf, process::ExitCode};

use ironpilot_adapters::{
    BYBIT_TESTNET_API_KEY_ENV, BYBIT_TESTNET_API_SECRET_ENV, BYBIT_TESTNET_SOCKS5_PROXY_ENV,
    BYBIT_TESTNET_WRITE_AUTHORIZATION_ENV, run_bybit_testnet_protocol_smoke,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("P4-02A Testnet smoke failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let repository_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/p4-02a-bybit-testnet-smoke.db"));
    let api_key = std::env::var(BYBIT_TESTNET_API_KEY_ENV)?;
    let api_secret = std::env::var(BYBIT_TESTNET_API_SECRET_ENV)?;
    let authorization = std::env::var(BYBIT_TESTNET_WRITE_AUTHORIZATION_ENV)?;
    let socks5_proxy = std::env::var(BYBIT_TESTNET_SOCKS5_PROXY_ENV)?;
    let report = run_bybit_testnet_protocol_smoke(
        &repository_path,
        api_key,
        api_secret,
        &authorization,
        &socks5_proxy,
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
