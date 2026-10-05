use std::process::Command;

use serde_json::Value;

#[test]
fn reports_fatal_startup_failures_as_structured_events() {
    let output = Command::new(env!("CARGO_BIN_EXE_flaggo-evidence-materializer"))
        .env("FLAGGO_MATERIALIZER_POLL_INTERVAL_MS", "0")
        .output()
        .expect("start Evidence Materializer");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 materializer stdout");
    let failure = stdout
        .lines()
        .find_map(|line| serde_json::from_str::<Value>(line).ok())
        .expect("structured materializer failure");
    assert_eq!(failure["event.name"], "flaggo.service.failed");
    assert!(
        failure["message"]
            .as_str()
            .expect("failure message")
            .contains("FLAGGO_MATERIALIZER_POLL_INTERVAL_MS must be greater than zero")
    );
}
