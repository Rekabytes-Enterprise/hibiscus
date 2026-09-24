mod support;
use std::process::Command;
use support::Pty;

#[test]
fn harness_drives_input_and_waits_for_successful_exit() {
    let mut command = Command::new("sh");
    command.args([
        "-c",
        "printf 'READY\\n'; read -r value; printf 'REPLY:%s\\n' \"$value\"",
    ]);
    let mut tty = Pty::spawn(&mut command);
    tty.wait_text("READY");
    tty.send(b"hello\r");
    tty.wait_text("REPLY:hello");
    assert!(tty.finish().contains("REPLY:hello"));
}

#[test]
fn early_exit_reports_captured_output_instead_of_hanging() {
    let failure = std::panic::catch_unwind(|| {
        let mut command = Command::new("sh");
        command.args(["-c", "printf 'MOCK_FAILURE\\n'; exit 7"]);
        let mut tty = Pty::spawn(&mut command);
        tty.wait_text("NEVER_EMITTED");
    })
    .expect_err("early exit must fail the wait");
    let message = failure.downcast_ref::<String>().unwrap();
    assert!(message.contains("MOCK_FAILURE"), "{message}");
    assert!(message.contains("child status"), "{message}");
}
