use std::process::Command;

use serde_json::Value;

#[test]
fn reports_configuration_failures_as_service_events() {
    let output = Command::new(env!("CARGO_BIN_EXE_flaggo-async-analysis"))
        .env("FLAGGO_ANALYSIS_MAX_AGENTS", "0")
        .output()
        .expect("start Async Analysis");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 Async Analysis stdout");
    let failure = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|entry| entry["event.name"] == "flaggo.service.failed")
        .expect("structured Async Analysis failure");
    assert_eq!(failure["flaggo.operation.outcome"], "failure");
    assert_eq!(failure["flaggo.failure.category"], "configuration");
}
