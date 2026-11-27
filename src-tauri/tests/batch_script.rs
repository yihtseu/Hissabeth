use serde_json::{Value, json};
use std::{path::Path, process::Command};

fn run(dir: &Path, script: &Path, success: bool) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_hissabeth"))
        .env("HISSABETH_DATA_DIR", dir)
        .args(["--engine", "--run"])
        .arg(script)
        .output()
        .unwrap();
    assert_eq!(
        output.status.success(),
        success,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn headless_script_traces_exports_and_asserts_lab_readiness() {
    let dir = tempfile::tempdir().unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scripts/bench_smoke.json");
    let report = run(dir.path(), &script, true);
    assert_eq!(report["ok"], true);
    let results = &report["results"];
    let trace = &results[1]["result"];
    assert!(trace["balance_error"].as_f64().unwrap().abs() < 1e-12);
    assert_eq!(trace["revision"], report["revision"]);
    let ends = trace["terminations"].as_array().unwrap();
    assert_eq!(ends.iter().filter(|t| t["reason"] == "detected").count(), 2);
    let export = &results[2]["result"];
    assert_eq!(export["mime"], "image/svg+xml");
    assert!(export["content"].as_str().unwrap().contains("<svg"));
    assert_eq!(results[3]["result"]["passed"], true);
    assert_eq!(results[3]["result"]["counts"]["errors"], 0);

    let saved = std::fs::read(dir.path().join("system.json")).unwrap();
    let mut bench = results[0]["result"]["bench"].clone();
    bench["elements"][1]["aperture"] = json!(0.1);
    let invalid = json!({"steps":[{"command":"bench_set","args":{"bench":bench}},
        {"command":"bench_lab_check","args":{"require_pass":true}}]});
    let script = dir.path().join("clipped.json");
    std::fs::write(&script, invalid.to_string()).unwrap();
    let failure = run(dir.path(), &script, false);
    assert_eq!(failure["failed"]["index"], 1);
    assert!(
        failure["failed"]["error"]
            .as_str()
            .unwrap()
            .contains("Bench lab check failed")
    );
    assert_eq!(
        std::fs::read(dir.path().join("system.json")).unwrap(),
        saved
    );
}
