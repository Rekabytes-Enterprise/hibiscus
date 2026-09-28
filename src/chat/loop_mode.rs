//! An unattended continuation workflow driven by Pi, not a second agent.
use super::*;
use crate::pi::{
    error::{ChatError, ErrorCategory},
    loop_verify::{self, Verdict},
    LoopApprovalBypass,
};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn parse_mode(input: &str) -> Option<(bool, &str)> {
    let input = input.trim();
    let (strict, goal) = if input == "--strict" {
        (true, "")
    } else if let Some(goal) = input
        .strip_prefix("--strict")
        .filter(|rest| rest.starts_with(char::is_whitespace))
    {
        (true, goal.trim())
    } else if let Some(goal) = input.strip_prefix("-- ") {
        (false, goal.trim())
    } else if input.starts_with("--") {
        return None;
    } else {
        (false, input)
    };
    (!goal.is_empty() && goal.len() <= 4096).then_some((strict, goal))
}

const CAPTURE_GUIDED: &str = "Before the first browser test, configure project-local Playwright to retain automatic failure evidence: use screenshot: 'only-on-failure' and trace: 'retain-on-failure' in the existing Playwright config, plus video: 'retain-on-failure' when video works on this platform. Preserve existing app-specific Playwright settings. Save failure artifacts before test teardown; do not wait until after the page/browser has closed. After a fix, explicitly save an unmodified passing screenshot of the same flow. If the browser never opens, no page screenshot exists; report the setup failure and logs honestly.";
const CAPTURE_STRICT: &str = "Before the first browser test, configure the existing Playwright project with screenshot: 'on' so passing UAT also has screenshot paths, trace: 'retain-on-failure', and video: 'retain-on-failure' where supported. Preserve other app-specific config. Attach browser-console output for each test; keep original failure screenshots/traces before teardown. Do not fabricate a page screenshot for browser startup failures.";

const FAILURE_GUIDANCE: &str = "When a browser E2E assertion fails, identify the exact page, button/control name and locator, reproduction steps, expected behavior, actual observed behavior, and assertion/error. Keep Playwright's automatic unmodified failure screenshot first when a page existed, then a separate annotated PNG with a visible raspberry/red outline or arrow and a short label around the implicated control. Label annotations as test annotations, not original UI, and never alter app state or mask a failure to make a test pass. If the element is absent, label its expected area as missing; do not invent a visible button or pretend a startup/network failure is a button defect. If capture or annotation is unavailable, explain that limitation instead of fabricating an image. Preserve original screenshots and traces; after fixing the bug, save a separate passing screenshot from the same flow. Keep failure-first evidence without repeatedly reintroducing the same bug. Create a concise Markdown failure summary mapping each issue to its control/locator, expected versus actual result, original and annotated screenshots, fix, and retest outcome. Show the relevant issue and artifact paths to the user; prefer viewable PNGs and text summaries, with trace ZIPs as supplemental evidence. Do not create a bug merely to demonstrate highlighting unless the user requested a deliberate failure.";

fn guided_instruction(goal: &str, continuation: bool) -> String {
    let next = if continuation {
        "Continue the remaining work; do not merely repeat a done summary."
    } else {
        "Work through the whole request and track progress using the goal checklist."
    };
    format!("Hibiscus unattended /loop (guided mode). Original request: {goal}\n{next} Hibiscus approvals are bypassed for this loop; Esc lets the user interrupt. Inspect the project and choose suitable test tools and artifact formats. For web apps, install project-local Playwright/browser dependencies on the user's host if needed, start the development server, run meaningful real browser E2E/UAT on relevant flows and buttons, inspect screenshots/DOM and browser/server logs, reproduce bugs, repair and retest. Do not substitute a smoke test for requested UAT. Use video only where tooling and the model support it. Preserve working features and scope; do not repeat a deliberate failure-first exercise once its evidence is saved. No special test names, report JSON schema or separate reviewer is required in guided mode. Summarize what you actually tested and limitations. If you judge the requested work and tests complete, call loop_status candidate_complete and explain the results in ordinary prose. Hibiscus will label this as your assessment, not independent verification. If genuinely blocked, call loop_status blocked with the reason. Never fabricate results or print internal status markers.\n{CAPTURE_GUIDED}\n{FAILURE_GUIDANCE}")
}

fn instruction(goal: &str, remaining: Option<&str>) -> String {
    if let Some(remaining) = remaining {
        return format!("Hibiscus unattended /loop. Immutable original goal: {goal}\nWork on THESE outstanding acceptance/UAT checks together: {remaining}. Do not repeat a completion claim without new test evidence. First re-submit an existing current Playwright JSON report using loop_status submit_evidence (projectDir and reportPath) and read an attached screenshot. Do not rerun passing tests solely because the reviewer could not see them. Rerun the browser suite only after code/test changes, stale results or actual test failures. Keep the original goal checklist intact; call loop_status candidate_complete only after all checks truly pass. If no action can resolve the mismatch, report a concrete blocker using loop_status. Do not substitute smoke tests for browser UAT.\n{CAPTURE_STRICT}\n{FAILURE_GUIDANCE}");
    }
    format!("Hibiscus unattended /loop. Goal: {goal}\nStart the task. Do not do smoke tests instead of UAT. Use Pi's goal tool to set explicit acceptance steps covering the ENTIRE original goal in the first turn; keep those same named steps in later turns instead of shrinking or renaming them. Implement and test them. On the user's machine/server, inspect the app and start its dev server. Install project-local Playwright and browser dependencies there if missing. Write one real Playwright browser E2E test with a title EXACTLY matching EACH original goal step, covering the relevant pages, buttons and flows. In each test, assert behavior and attach a nonempty screenshot as `screenshot` and captured browser-console output as `browser-console` using testInfo.attach. Run real Playwright tests with --reporter=json in the project directory and save their JSON report inside that project (redirecting stdout is supported). Submit it using loop_status action submit_evidence with projectDir (relative to the workspace or absolute) and reportPath (relative to projectDir or absolute). Artifact paths may be absolute, project-relative or report-directory-relative inside that project. Submit existing current evidence before rerunning tests; after modifying app/tests rerun them to get fresh evidence. Include supportFiles with the test source, server log and earlier failing report when needed to substantiate the original goal. Submission returns actual accepted counts and missing checks; act on that result rather than treating an evidence visibility problem as an app failure. Check DOM, console and server logs, and use Pi read to inspect a screenshot attached to the successful final test run. Diagnose failures, fix them and rerun. If available, inspect video too. Do not claim success from a model-reported goal alone. Only after real UAT passes and all goal steps are completed, call the loop_status tool with action candidate_complete, then explain the observed work and tests in ordinary prose. If a required environment, credential, app or browser dependency truly cannot be made available, call loop_status with action blocked and a specific reason. Never print internal loop markers in assistant text. Otherwise keep working; do not ask for routine approvals. Do not publish fabricated results.\n{CAPTURE_STRICT}\n{FAILURE_GUIDANCE}")
}

fn last_assistant(messages: &Value) -> Option<String> {
    messages["messages"]
        .as_array()?
        .iter()
        .rev()
        .find(|entry| entry["role"] == "assistant")
        .and_then(|entry| entry["content"].as_array())
        .map(|blocks| {
            blocks
                .iter()
                .filter(|block| block["type"] == "text")
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run<R: BufRead>(
    goal: &str,
    rpc: &mut Rpc,
    input: &mut R,
    output: &mut Screen,
    dialogs: &mut Dialogs,
    raw: &mut Option<RawMode>,
    events: Option<&Receiver<u8>>,
    ui: &Ui,
) -> Result<()> {
    if !output.is_full() || events.is_none() {
        ui.info(
            output,
            "/loop needs an interactive full-screen terminal for Esc cancellation.",
        )?;
        return Ok(());
    }
    let Some((strict, goal)) = parse_mode(goal) else {
        ui.info(output, "Usage: /loop [--strict] <goal> (up to 4096 bytes)")?;
        return Ok(());
    };
    output.reset_loop_goal(strict)?;
    let _bypass = LoopApprovalBypass::start()?;
    output.set_loop_phase("Loop · continuing…")?;
    ui.info(
        output,
        if strict {
            "Loop active · strict verification · approvals bypassed · Esc stops"
        } else {
            "Loop active · guided mode · approvals bypassed · Esc stops"
        },
    )?;
    let mut rate_attempts = 0usize;
    let mut prompt = if strict {
        instruction(goal, None)
    } else {
        guided_instruction(goal, false)
    };
    let mut previous_gate: Option<String> = None;
    let mut repeated_empty_turns = 0usize;
    loop {
        let checkpoint = output.loop_checkpoint();
        let result = rpc.command(
            json!({"type":"prompt","message":&prompt}),
            &[],
            output,
            input,
            dialogs,
            events,
            raw,
            true,
        );
        match result {
            Ok(true) => {
                ui.info(output, "Loop stopped by Esc; approvals restored.")?;
                return Ok(());
            }
            Ok(false) => {}
            Err(error) => {
                // Pi owns its own retry attempts. Never replay an uncertain
                // provider/tool outcome; only pre-tool transient failures can
                // be tried again in this opt-in mode.
                let transient = error
                    .downcast_ref::<ChatError>()
                    .is_some_and(|err| err.category == ErrorCategory::RateLimit);
                if transient {
                    rate_attempts = rate_attempts.saturating_add(1 + output.loop_auto_retries());
                }
                if transient
                    && !output.loop_used_tools()
                    && rate_attempts < 10
                    && rpc.is_connected()
                {
                    output.set_loop_phase("Loop · waiting to retry…")?;
                    ui.info(
                        output,
                        &format!("Temporary rate limit · {rate_attempts}/10 attempts observed"),
                    )?;
                    let until = Instant::now() + Duration::from_secs(rate_attempts.min(5) as u64);
                    if let Some(keys) = events {
                        while Instant::now() < until {
                            let remaining = until.saturating_duration_since(Instant::now());
                            match keys.recv_timeout(remaining.min(Duration::from_millis(100))) {
                                Ok(27) => {
                                    ui.info(output, "Loop stopped by Esc; approvals restored.")?;
                                    return Ok(());
                                }
                                Ok(byte) => output.defer_session_typing([byte]),
                                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                    return Err(error)
                                }
                                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                            }
                        }
                    }
                    continue;
                }
                return Err(error);
            }
        }
        if !strict {
            if let Some(reason) = output.loop_blocked().map(str::to_owned) {
                ui.info(
                    output,
                    &format!("⚠ Loop blocked · {reason} · approvals restored"),
                )?;
                return Ok(());
            }
            if output.loop_candidate() {
                ui.info(output, "✓ Loop finished · Pi reports the requested work and tests complete. Not independently verified.")?;
                return Ok(());
            }
            repeated_empty_turns = if output.loop_used_tools() {
                0
            } else {
                repeated_empty_turns + 1
            };
            if repeated_empty_turns >= 4 {
                ui.info(output, "⚠ Loop blocked · Pi made no tool progress after targeted recovery · approvals restored")?;
                return Ok(());
            }
            prompt = guided_instruction(goal, true);
            if repeated_empty_turns >= 2 {
                prompt.push_str(" Recovery: take a concrete test/repair step, or report completion/blocker through loop_status instead of repeating prose.");
            }
            output.set_loop_phase("Loop · continuing…")?;
            continue;
        }
        output.revalidate_loop_sources();
        output.set_loop_phase("Loop · checking UAT…")?;
        let messages = rpc.request(
            json!({"type":"get_messages"}),
            input,
            output,
            dialogs,
            raw,
            events,
            true,
        )?;
        let text = last_assistant(&messages).unwrap_or_default();
        if let Some(reason) = output.loop_blocked().map(str::to_owned) {
            ui.info(
                output,
                &format!("⚠ Loop blocked · {reason} · approvals restored"),
            )?;
            return Ok(());
        }
        let mut review_reason = String::new();
        if let Some(steps) = output.loop_steps() {
            output.set_loop_phase("Loop · review")?;
            let state = rpc.request(
                json!({"type":"get_state"}),
                input,
                output,
                dialogs,
                raw,
                events,
                true,
            )?;
            let verdict = loop_verify::evaluate(
                goal,
                &steps,
                output.loop_uat_observed(),
                &output.loop_evidence_summary(),
                &text,
                &state["model"],
                events.expect("full-screen input checked"),
                output,
            )?;
            if matches!(verdict, Verdict::Met) {
                output.lock_loop_scope();
            }
            match verdict {
                Verdict::Met if output.loop_candidate() && output.loop_goal_complete() => {
                    let count = steps.len();
                    ui.info(output, &format!("✓ Loop finished · {count}/{count} acceptance checks · browser UAT passed. Review test evidence."))?;
                    return Ok(());
                }
                Verdict::Met => {
                    review_reason = "Reviewer marked scope met, but Pi has not sent the candidate-complete tool signal after UAT.".into()
                }
                Verdict::NotMet(reason) => {
                    output.lock_loop_scope();
                    review_reason = reason;
                }
                Verdict::Missing(reason) => {
                    output.allow_loop_scope_revision();
                    review_reason = reason;
                }
                Verdict::Blocked(reason) => {
                    ui.info(
                        output,
                        &format!("Loop reviewer blocked: {reason}. Approvals restored."),
                    )?;
                    return Ok(());
                }
                Verdict::Cancelled => {
                    ui.info(output, "Loop stopped by Esc; approvals restored.")?;
                    return Ok(());
                }
                Verdict::Unavailable(reason) => {
                    ui.info(
                        output,
                        &format!("Loop blocked: {reason}; approvals restored."),
                    )?;
                    return Ok(());
                }
            }
        }
        let (done, total, verified) = output.loop_progress();
        let missing = output.loop_missing();
        let gate = format!(
            "{:?}|{done}/{total}|{verified}/{total}|{missing:?}|{}",
            output.loop_steps(),
            output.loop_goal_complete()
        );
        repeated_empty_turns =
            if previous_gate.as_deref() == Some(&gate) && !output.loop_used_tools() {
                repeated_empty_turns.saturating_add(1)
            } else {
                0
            };
        previous_gate = Some(gate);
        if repeated_empty_turns > 0 && output.loop_candidate() {
            output.collapse_repeated_loop_reply(checkpoint)?;
        }
        let mut outstanding = format!(
            "Reviewer: {review_reason}. Checklist {done}/{total}; UAT {verified}/{total}. {}",
            if missing.is_empty() {
                output.loop_verification_reason()
            } else {
                missing.join(" ")
            }
        );
        if repeated_empty_turns >= 4 {
            ui.info(output, &format!("⚠ Loop blocked · Repeated completion claims without new work or UAT evidence. Missing: {} · approvals restored",
                outstanding.chars().filter(|ch| !ch.is_control()).take(300).collect::<String>()))?;
            return Ok(());
        }
        if repeated_empty_turns >= 2 {
            outstanding.push_str(" Recovery: stop summarizing; inspect the missing criteria, run the failing browser case and produce new evidence.");
        }
        let shown: String = outstanding
            .chars()
            .filter(|ch| !ch.is_control())
            .take(500)
            .collect();
        ui.info(output, &format!("↻ Loop continuing · Missing: {shown}"))?;
        output.set_loop_phase("Loop · continuing…")?;
        prompt = instruction(goal, Some(&outstanding));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failure_annotation_guidance_is_kept_in_both_modes_and_continuations() {
        for prompt in [
            guided_instruction("Build board", false),
            guided_instruction("Build board", true),
            instruction("Build board", None),
            instruction("Build board", Some("Fix Delete")),
        ] {
            assert!(prompt.contains(FAILURE_GUIDANCE));
            assert!(prompt.contains("unmodified failure screenshot first"));
            assert!(prompt.contains("expected versus actual"));
            assert!(prompt.contains("Do not create a bug"));
            assert!(prompt.contains("retain-on-failure"));
            assert!(prompt.contains("Before the first browser test"));
        }
    }

    #[test]
    fn default_is_guided_and_strict_requires_an_explicit_flag() {
        assert_eq!(parse_mode("test app"), Some((false, "test app")));
        assert_eq!(parse_mode("--strict test app"), Some((true, "test app")));
        assert_eq!(parse_mode("--strict\ttest app"), Some((true, "test app")));
        assert_eq!(parse_mode("--strict"), None);
        assert_eq!(parse_mode("--strictly test"), None);
        assert_eq!(
            parse_mode("-- --strictly test"),
            Some((false, "--strictly test"))
        );
        assert!(guided_instruction("test app", false).contains("No special test names"));
        assert!(!guided_instruction("test app", false).contains("--reporter=json"));
        assert!(guided_instruction("test app", false).contains("screenshot: 'only-on-failure'"));
        assert!(instruction("test app", None).contains("screenshot: 'on'"));
    }

    #[test]
    fn loop_uses_tool_signal_instead_of_a_visible_completion_marker() {
        assert_eq!(last_assistant(&json!({"messages":[{"role":"assistant","content":[{"type":"text","text":"still working"}]},{"role":"user","content":[{"type":"text","text":"candidate_complete"}]}]})).as_deref(), Some("still working"));
        let text = instruction("ship UI", None);
        assert!(text.contains("real Playwright browser E2E test"));
        assert!(text.contains("call the loop_status tool"));
        assert!(!text.contains("HIBISCUS_LOOP_COMPLETE"));
    }
}
