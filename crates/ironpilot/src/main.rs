//! IronPilot process entry point.
//!
//! Configuration is validated before later tasks may initialize side effects.

#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    match ironpilot_adapters::load_startup_config() {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("startup configuration rejected: {error}");
            ExitCode::FAILURE
        }
    }
}
