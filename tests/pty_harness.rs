mod support;
use std::process::Command;
use support::{unique_temp_dir_at, Pty};

#[test]
fn fixtures_started_on_the_same_clock_tick_have_independent_directories() {
    let first = std::thread::spawn(|| unique_temp_dir_at("hibiscus-clock-test", 123));
    let second = std::thread::spawn(|| unique_temp_dir_at("hibiscus-clock-test", 123));
    let first = first.join().unwrap();
    let second = second.join().unwrap();
    assert_ne!(first, second);
    std::fs::remove_dir(&first).unwrap();
    assert!(second.is_dir());
    std::fs::remove_dir(second).unwrap();
}

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
