use std::process::Command;

#[test]
fn coding_evaluation_runner_scores_actual_native_outcomes() {
    let output = Command::new("python3")
        .args(["-m", "unittest", "discover", "-s", "tests/evaluation", "-v"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("HIBISCUS_EVAL_BINARY", env!("CARGO_BIN_EXE_hibiscus"))
        .output()
        .expect("Evaluation development tests require Python 3.9 or newer");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
