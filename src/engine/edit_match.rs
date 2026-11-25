//! Exact replacements first. Whitespace recovery must match a unique whole-line
//! block with the same relative indentation.
use crate::Result;

pub(super) fn replace(source: &str, old: &str, new: &str) -> Result<(String, bool)> {
    if old.is_empty() {
        return Err("Edit needs nonempty oldText.".into());
    }
    if let Some(start) = source.find(old) {
        let next_start = start + old.chars().next().unwrap().len_utf8();
        if source[next_start..].contains(old) {
            return Err(
                "Edit matches more than once. Include more surrounding text. File unchanged."
                    .into(),
            );
        }
        let mut next = source.to_owned();
        next.replace_range(start..start + old.len(), new);
        return Ok((next, false));
    }

    let pattern: Vec<_> = old.lines().take(257).collect();
    if pattern.is_empty()
        || pattern.len() > 256
        || pattern.iter().all(|line| line.trim().is_empty())
    {
        return Err(
            "Edit text did not match. Read the exact lines and resend the edit. File unchanged."
                .into(),
        );
    }
    let mut offset = 0;
    let lines: Vec<_> = source
        .split_inclusive('\n')
        .take(100_001)
        .map(|raw| {
            let start = offset;
            offset += raw.len();
            (
                start,
                raw.strip_suffix("\r\n")
                    .or_else(|| raw.strip_suffix('\n'))
                    .unwrap_or(raw),
            )
        })
        .collect();
    if offset != source.len() || lines.len() > 100_000 {
        return Err(
            "Edit text did not match. Read the exact lines and resend the edit. File unchanged."
                .into(),
        );
    }

    let mut matching = None;
    let mut hint = None;
    let mut budget = 32 * 1024 * 1024usize;
    for (index, window) in lines.windows(pattern.len()).enumerate() {
        // Bound comparisons on files with many almost-identical repeated blocks.
        let same_content = window
            .iter()
            .zip(&pattern)
            .take_while(|((_, real), expected)| {
                budget = budget.saturating_sub(real.len() + expected.len() + 128);
                budget > 0 && padding(real) == padding(expected)
            })
            .count();
        if budget == 0 {
            return Err("Edit search found no safely unique match. Read the exact lines and resend. File unchanged.".into());
        }
        if same_content > 0 && hint.is_none_or(|(score, _)| same_content > score) {
            hint = Some((same_content, index));
        }
        if same_content == pattern.len() {
            if matching.is_some() {
                return Err("Edit matches more than once after ignoring whitespace. Include more context. File unchanged.".into());
            }
            matching = Some(window);
        }
    }
    if hint.is_none() {
        if let Some((anchor_index, anchor)) = pattern
            .iter()
            .enumerate()
            .max_by_key(|(_, line)| padding(line).len())
        {
            let anchor = padding(anchor);
            let best = lines
                .iter()
                .enumerate()
                .map(|(index, (_, line))| {
                    let common_prefix = padding(line)
                        .chars()
                        .zip(anchor.chars())
                        .take(80)
                        .take_while(|(a, b)| a == b)
                        .count();
                    (common_prefix, index.saturating_sub(anchor_index))
                })
                .max_by_key(|&(score, _)| score);
            hint = best.filter(|(score, _)| *score >= 6);
        }
    }
    if let Some(window) = matching {
        let shifts = window
            .iter()
            .zip(&pattern)
            .filter(|(_, expected)| !padding(expected).is_empty())
            .map(|((_, actual), expected)| {
                let actual = indent(actual);
                let expected = indent(expected);
                if let Some(prefix) = actual.strip_suffix(expected) {
                    Some((prefix, ""))
                } else {
                    expected.strip_suffix(actual).map(|prefix| ("", prefix))
                }
            });
        let mut shift = None;
        for candidate in shifts {
            let Some(candidate) = candidate else {
                return Err(candidate_error(&lines, hint, pattern.len()));
            };
            if shift.is_some_and(|shift| shift != candidate) {
                return Err(candidate_error(&lines, hint, pattern.len()));
            }
            shift = Some(candidate);
        }
        if let Some((add, remove)) = shift {
            let start = window[0].0;
            let (last, line) = window[window.len() - 1];
            let end = last + line.len();
            let ending = if source[start..]
                .split_once('\n')
                .is_some_and(|(first, _)| first.ends_with('\r'))
            {
                "\r\n"
            } else {
                "\n"
            };
            let mut replacement = String::new();
            for (index, line) in new.lines().enumerate() {
                if index > 0 {
                    replacement.push_str(ending);
                }
                if line.trim().is_empty() {
                    replacement.push_str(line);
                } else {
                    let adjusted = line
                        .strip_prefix(remove)
                        .ok_or_else(|| candidate_error(&lines, hint, pattern.len()))?;
                    replacement.push_str(add);
                    replacement.push_str(adjusted);
                }
            }
            if new.ends_with('\n') {
                replacement.push_str(ending);
            }
            let mut next = source.to_owned();
            let end = if old.ends_with('\n') {
                let tail = &source[end..];
                if tail.starts_with("\r\n") {
                    end + 2
                } else if tail.starts_with('\n') {
                    end + 1
                } else {
                    return Err(candidate_error(&lines, hint, pattern.len()));
                }
            } else {
                end
            };
            next.replace_range(start..end, &replacement);
            return Ok((next, true));
        }
    }
    Err(candidate_error(&lines, hint, pattern.len()))
}

fn padding(line: &str) -> &str {
    line.trim_matches([' ', '\t'])
}

fn indent(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

fn candidate_error(
    lines: &[(usize, &str)],
    hint: Option<(usize, usize)>,
    pattern_len: usize,
) -> Box<dyn std::error::Error + Send + Sync> {
    let Some((_, start)) = hint else {
        return "Edit text did not match. Read the exact lines and resend the edit. File unchanged.".into();
    };
    let end = (start + pattern_len.min(8)).min(lines.len());
    let mut snippet = String::new();
    for (index, (_, line)) in lines[start..end].iter().enumerate() {
        let excerpt: String = line.chars().take(160).collect();
        snippet.push_str(&format!("{}: {excerpt}\n", start + index + 1));
    }
    format!("Edit text did not match safely. Candidate lines {}–{} below. Read them and resend the exact edit. File unchanged.\n{snippet}", start + 1, end).into()
}
