//! Validate all edits against their original files, then checkpoint each file once.
use super::tools::{edit_diff_at, path, read_file, result};
use crate::{checkpoints, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, ops::Range, path::PathBuf};
use tokio::sync::watch;

const MAX_EDITS: usize = 32;
const MAX_FILE: usize = 16 * 1024 * 1024;
const MAX_SNAPSHOTS: usize = 32 * 1024 * 1024;
const MAX_DIFF: usize = 64 * 1024;

pub(super) fn definition() -> Value {
    json!({
        "name":"batch_edit",
        "description":"Apply 2 to 32 ready, independent exact replacements in existing UTF-8 files within the workspace. Multiple replacements can target the same file. All oldText matches must be unique and non-overlapping in the original files. Validates every match before writing, writes each changed file once, and returns a shared checkpoint and grouped diff. Inspect the result before retrying any failed batch.",
        "parameters":{"type":"object","properties":{"edits":{
            "type":"array","minItems":2,"maxItems":MAX_EDITS,
            "items":{"type":"object","properties":{"path":{"type":"string","minLength":1,"maxLength":4096},"oldText":{"type":"string","minLength":1},"newText":{"type":"string"}},"required":["path","oldText","newText"],"additionalProperties":false}
        }},"required":["edits"],"additionalProperties":false}
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Batch {
    edits: Vec<Edit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Edit {
    path: String,
    old_text: String,
    new_text: String,
}

pub(super) fn parse(args: &Value) -> Result<Batch> {
    let batch: Batch = serde_json::from_value(args.clone())?;
    if !(2..=MAX_EDITS).contains(&batch.edits.len())
        || batch.edits.iter().any(|edit| {
            edit.path.is_empty()
                || edit.path.len() > 4096
                || edit.old_text.is_empty()
                || edit.old_text.len() > MAX_FILE
                || edit.new_text.len() > MAX_FILE
        })
    {
        return Err("batch_edit needs 2 to 32 valid file replacements".into());
    }
    Ok(batch)
}

struct Prepared {
    path: PathBuf,
    label: String,
    before: String,
    after: String,
    regions: Vec<Region>,
}

struct Region {
    before: Range<usize>,
    after: Range<usize>,
}

fn prepare(
    batch: Batch,
    cwd: &std::path::Path,
    cancel: &watch::Receiver<bool>,
) -> Result<Vec<Prepared>> {
    let paths = batch
        .edits
        .iter()
        .map(|edit| path(cwd, &edit.path))
        .collect::<Result<Vec<_>>>()?;
    let targets = checkpoints::batch_targets(cwd, &paths)?;
    let mut files = BTreeMap::<_, (String, Vec<Edit>)>::new();
    for ((target, label), edit) in targets.into_iter().zip(batch.edits) {
        files
            .entry(target)
            .or_insert_with(|| (label, Vec::new()))
            .1
            .push(edit);
    }

    let mut bytes = 0;
    let mut prepared = Vec::new();
    for (path, (label, edits)) in files {
        if *cancel.borrow() {
            return Err("Batch edit cancelled before writing".into());
        }
        let before = read_file(&path)
            .map_err(|error| format!("Batch edit preflight failed for {label}: {error}"))?;
        let (after, regions) = replace(&before, &edits)
            .map_err(|error| format!("Batch edit preflight failed for {label}: {error}"))?;
        bytes += before.len() + after.len();
        if bytes > MAX_SNAPSHOTS {
            return Err(
                "Batch edit exceeds 32 MiB of file snapshots; no files were changed".into(),
            );
        }
        if before != after {
            prepared.push(Prepared {
                path,
                label,
                before,
                after,
                regions,
            });
        }
    }
    Ok(prepared)
}

fn replace(before: &str, edits: &[Edit]) -> Result<(String, Vec<Region>)> {
    let mut matches = Vec::with_capacity(edits.len());
    for edit in edits {
        let Some(start) = before.find(&edit.old_text) else {
            return Err("oldText was not found in the original file".into());
        };
        // A second match may overlap the first (for example "aa" in "aaa").
        let next_character = edit.old_text.chars().next().map_or(0, char::len_utf8);
        if before[start + next_character..].contains(&edit.old_text) {
            return Err("oldText matches more than once in the original file".into());
        }
        matches.push((start, start + edit.old_text.len(), edit.new_text.as_str()));
    }
    matches.sort_unstable_by_key(|(start, _, _)| *start);
    if matches.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err("replacements overlap in the original file".into());
    }
    let removed: usize = matches.iter().map(|(start, end, _)| end - start).sum();
    let inserted: usize = matches.iter().map(|(_, _, text)| text.len()).sum();
    let size = before.len() - removed + inserted;
    if size > MAX_FILE {
        return Err("the edited file would exceed 16 MiB".into());
    }

    let mut after = String::with_capacity(size);
    let mut regions: Vec<Region> = Vec::new();
    let mut end = 0;
    for (start, next_end, new_text) in matches {
        let gap = &before[end..start];
        after.push_str(gap);
        let new_start = after.len();
        after.push_str(new_text);
        // A full unchanged line between changes separates their diff hunks.
        if let Some(region) = regions
            .last_mut()
            .filter(|_| gap.bytes().filter(|b| *b == b'\n').nth(1).is_none())
        {
            region.before.end = next_end;
            region.after.end = after.len();
        } else {
            regions.push(Region {
                before: start..next_end,
                after: new_start..after.len(),
            });
        }
        end = next_end;
    }
    after.push_str(&before[end..]);
    Ok((after, regions))
}

fn whole_lines(text: &str, range: Range<usize>) -> Range<usize> {
    let start = text[..range.start].rfind('\n').map_or(0, |index| index + 1);
    let end = if text[..range.end].ends_with('\n') && range.end > start {
        range.end
    } else {
        text[range.end..]
            .find('\n')
            .map_or(text.len(), |index| range.end + index + 1)
    };
    start..end
}

fn preview(file: &Prepared, mut budget: usize) -> (String, bool) {
    let mut diff = String::new();
    for region in &file.regions {
        let before = whole_lines(&file.before, region.before.clone());
        // Expand both ranges by the same unchanged bytes on either side.
        let after = (region.after.start - (region.before.start - before.start))
            ..(region.after.end + (before.end - region.before.end));
        let (hunk, _, _) = edit_diff_at(
            &file.before[before.clone()],
            &file.after[after.clone()],
            file.before[..before.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count(),
            file.after[..after.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count(),
        );
        if hunk.len() > budget {
            if let Some(end) = hunk.as_bytes()[..budget]
                .iter()
                .rposition(|byte| *byte == b'\n')
            {
                diff.push_str(&hunk[..=end]);
            }
            return (diff, true);
        }
        budget -= hunk.len();
        diff.push_str(&hunk);
    }
    (diff, false)
}

pub(super) fn execute(
    batch: Batch,
    context: &mut checkpoints::Context<'_>,
    cancel: &watch::Receiver<bool>,
) -> Result<Value> {
    let prepared = prepare(batch, context.cwd, cancel)?;
    let diff_budget = MAX_DIFF / prepared.len().max(1);
    let mut diff = String::new();
    let mut edited = Vec::new();
    let mut checkpoint = Value::Null;
    let mut failure = None;
    let mut shortened = false;
    for file in prepared {
        if *cancel.borrow() {
            failure = Some("Batch edit cancelled before finishing.".to_owned());
            break;
        }
        match checkpoints::write_protected(
            context,
            &file.path,
            file.after.as_bytes(),
            file.before.as_bytes(),
        ) {
            Ok(receipt) => checkpoint = receipt,
            Err(error) => {
                failure = Some(format!("Stopped at {}: {error}", file.label));
                break;
            }
        }
        let from = serde_json::to_string(&format!("a/{}", file.label))?;
        let to = serde_json::to_string(&format!("b/{}", file.label))?;
        let header = format!("diff --git {from} {to}\n--- {from}\n+++ {to}\n");
        let (body, truncated) = preview(&file, diff_budget.saturating_sub(header.len()));
        shortened |= truncated;
        if !body.is_empty() {
            diff.push_str(&header);
            diff.push_str(&body);
        }
        edited.push(file.label);
    }

    let mut message = match edited.len() {
        0 => "No completed file edits.".to_owned(),
        1 => "Edited 1 file.".to_owned(),
        count => format!("Edited {count} files."),
    };
    if let Some(id) = checkpoint["checkpoint"].as_str() {
        message.push_str(&format!(" Checkpoint {id}."));
    }
    if shortened {
        message.push_str(" Diff preview shortened.");
    }
    let failed = failure.is_some();
    if let Some(failure) = failure {
        message.push_str(&format!(
            " {failure} Inspect the affected files and checkpoint before retrying."
        ));
    }
    let mut value = result(
        message,
        json!({"diff":diff,"editedPaths":edited,"checkpoint":checkpoint}),
    );
    if failed {
        value["isError"] = json!(true);
    }
    Ok(value)
}
