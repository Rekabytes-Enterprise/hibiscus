//! Per-turn observations of repeated failures and unchanged native reads.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;

type Signature = [u8; 32];

#[derive(Default)]
pub(super) struct Monitor {
    recent: VecDeque<Observation>,
}

struct Observation {
    call: Signature,
    outcome: Signature,
    repetitions: u8,
    suppressed: bool,
}

#[derive(PartialEq, Eq)]
pub(super) enum Decision {
    Run,
    Suppress,
    Stop,
}

pub(super) fn signature(name: &str, args: &Value, malformed: Option<&Signature>) -> Signature {
    let mut hash = Sha256::new();
    hash.update(name.as_bytes());
    hash.update([0]);
    let mut canonical = args.clone();
    if name == "bash" {
        if let Some(fields) = canonical.as_object_mut() {
            fields.remove("description");
        }
    }
    hash.update(canonical.to_string().as_bytes());
    if let Some(raw) = malformed {
        hash.update(raw);
    }
    hash.finalize().into()
}

impl Monitor {
    pub(super) fn reset(&mut self) {
        self.recent.clear();
    }

    pub(super) fn before(&mut self, signature: Signature) -> Decision {
        let Some(observation) = self
            .recent
            .iter_mut()
            .find(|record| record.call == signature)
        else {
            return Decision::Run;
        };
        if observation.repetitions < 2 {
            return Decision::Run;
        }
        if observation.suppressed {
            return Decision::Stop;
        }
        observation.suppressed = true;
        Decision::Suppress
    }

    /// Observe before adding presentation notes, durations or per-attempt counters.
    pub(super) fn observe(
        &mut self,
        call: Signature,
        name: &str,
        failed: bool,
        result: &Value,
    ) -> bool {
        let details = &result["details"];
        let text = result["content"][0]["text"].as_str().unwrap_or_default();
        if name == "batch_edit"
            && details["editedPaths"]
                .as_array()
                .is_some_and(|files| !files.is_empty())
        {
            // A stopped batch may still have completed edits.
            self.reset();
            return false;
        }
        let outcome = if failed {
            if let Some(exit) = details["exitCode"].as_i64() {
                // Same command and exit status in one unchanged workspace epoch.
                format!("exit:{exit}")
            } else {
                format!("error:{text}")
            }
        } else if details["reportedCrash"] == true {
            "reported-crash".into()
        } else if matches!(name, "read" | "grep" | "glob") {
            format!("read:{text}")
        } else {
            if matches!(
                name,
                "write"
                    | "edit"
                    | "batch_edit"
                    | "checkpoint_restore"
                    | "bash"
                    | "working_repository"
            ) {
                self.reset();
            }
            return false;
        };
        let outcome = Sha256::digest(outcome.as_bytes()).into();
        let mut repetitions = 1;
        if let Some(index) = self.recent.iter().position(|record| record.call == call) {
            let previous = self.recent.remove(index).unwrap();
            if previous.outcome == outcome {
                repetitions = previous.repetitions + 1;
            }
        }
        if self.recent.len() == 64 {
            self.recent.pop_front();
        }
        self.recent.push_back(Observation {
            call,
            outcome,
            repetitions,
            suppressed: false,
        });
        repetitions == 2
    }
}
