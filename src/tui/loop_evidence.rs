//! Parse Pi's Playwright JSON reporter. Counts and artifact files are stronger
//! than model prose, but cannot prove the tests' assertions are meaningful.
use serde_json::Value;
use std::{
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::SystemTime,
};

pub(super) struct Assessment {
    pub screenshots: Vec<PathBuf>,
    pub inline_images: Vec<Vec<u8>>,
    pub verified: usize,
    pub missing: Vec<String>,
    pub complete: bool,
    pub summary: Value,
}

impl Assessment {
    pub(super) fn invalid(reason: &str) -> Self {
        Self {
            screenshots: Vec::new(),
            inline_images: Vec::new(),
            verified: 0,
            missing: vec![reason.into()],
            complete: false,
            summary: serde_json::json!({"source":"validation", "missing":[reason]}),
        }
    }
}

pub(super) fn inspect(result: &Value, steps: &[String]) -> Assessment {
    if steps.is_empty() {
        return Assessment::invalid("Set named acceptance steps before UAT.");
    }
    let Some(text) = result["content"]
        .as_array()
        .and_then(|parts| parts.iter().find(|part| part["type"] == "text"))
        .and_then(|part| part["text"].as_str())
    else {
        return Assessment::invalid("Playwright JSON reporter output is missing.");
    };
    let Ok(report) = serde_json::from_str::<Value>(text.trim()) else {
        return Assessment::invalid("Playwright JSON reporter output is invalid or truncated.");
    };
    inspect_report(&report, steps, None)
}

// A bounded regular-file read; never open FIFOs/devices from a model-supplied path.
pub(super) fn inspect_file(
    project: &str,
    report: &str,
    steps: &[String],
    after: SystemTime,
) -> Assessment {
    let Ok(project) = Path::new(project).canonicalize() else {
        return Assessment::invalid("Evidence project directory does not exist.");
    };
    if !project.is_dir() {
        return Assessment::invalid("Evidence projectDir is not a directory.");
    }
    let Ok(path) = project.join(report).canonicalize() else {
        return Assessment::invalid(
            "Saved Playwright report does not exist; submit reportPath relative to projectDir.",
        );
    };
    if !path.starts_with(&project) {
        return Assessment::invalid("Report must be inside the submitted project directory.");
    }
    let Ok(meta) = path.metadata() else {
        return Assessment::invalid("Cannot stat the Playwright report.");
    };
    if !meta.is_file() || meta.len() > 4 * 1024 * 1024 {
        return Assessment::invalid("Report must be a regular JSON file of at most 4 MiB.");
    }
    // Filesystem mtimes can be rounded down (even on local Linux filesystems).
    // Allow one second of timestamp granularity; this is freshness evidence,
    // not cryptographic attestation against an agent rewriting an old report.
    if meta.modified().is_err()
        || meta.modified().is_ok_and(|time| {
            after
                .duration_since(time)
                .is_ok_and(|age| age > std::time::Duration::from_secs(1))
        })
    {
        return Assessment::invalid("Saved report predates this loop or a possibly mutating tool; rerun UAT after the change.");
    }
    match source_inventory(project.to_str().unwrap_or("")) {
        Ok((_, latest))
            if latest
                .duration_since(meta.modified().unwrap_or(SystemTime::UNIX_EPOCH))
                .is_ok_and(|age| age > std::time::Duration::from_secs(1)) =>
        {
            return Assessment::invalid("Tracked app/test/config files are newer than the saved report; rerun UAT after the source change.");
        }
        Err(_) => {
            return Assessment::invalid("Cannot check project sources within bounded limits.")
        }
        _ => {}
    }
    let mut bytes = Vec::new();
    let loaded = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(&path)
        .and_then(|file| {
            if !file.metadata()?.is_file() {
                return Err(std::io::Error::other("not a regular report"));
            }
            file.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)
        });
    if loaded.is_err() || bytes.len() > 4 * 1024 * 1024 {
        return Assessment::invalid("Could not read the bounded Playwright report.");
    }
    let Ok(report) = serde_json::from_slice::<Value>(&bytes) else {
        return Assessment::invalid("Saved Playwright report is not valid JSON.");
    };
    let mut check = inspect_report(
        &report,
        steps,
        Some((&project, path.parent().unwrap_or(&project))),
    );
    check.summary["reportPath"] = serde_json::json!(path_label(&path));
    check.summary["projectDir"] = serde_json::json!(path_label(&project));
    fn source_files(value: &Value, names: &mut Vec<String>) {
        if names.len() >= 8 {
            return;
        }
        if let Some(file) = value["file"].as_str().filter(|s| s.len() <= 4096) {
            if !names.iter().any(|name| name == file) {
                names.push(file.to_owned());
            }
        }
        for child in value["suites"].as_array().into_iter().flatten() {
            source_files(child, names);
        }
    }
    let mut names = Vec::new();
    source_files(&report, &mut names);
    let test_root = report["config"]["rootDir"]
        .as_str()
        .map(|root| project.join(root))
        .unwrap_or(project.clone());
    let sources: Vec<_> = names.into_iter().filter_map(|name| {
        let path = test_root.join(name).canonicalize().ok()?;
        if !path.starts_with(&project) { return None; }
        let label = path.file_name()?.to_str()?;
        if !label.contains(".spec.") && !label.contains(".test.") { return None; }
        let bytes = bounded_bytes(&path,8193).ok()?;
        Some(serde_json::json!({"path":path_label(&path),"excerpt":text_excerpt(&bytes[..bytes.len().min(8192)]),"truncated":bytes.len()>8192}))
    }).collect();
    check.summary["testSources"] = serde_json::json!(sources);
    check
}

pub(super) fn bounded_bytes(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn text_excerpt(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .take(120)
        .map(crate::pi::error::safe_message)
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(6000)
        .collect()
}

pub(super) fn decode_body(text: &str) -> Option<Vec<u8>> {
    if text.is_empty() || text.len() > 4 * 1024 * 1024 || !text.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::new();
    let chunks = text.as_bytes().as_chunks::<4>().0.iter();
    let count = chunks.len();
    for (i, chunk) in chunks.enumerate() {
        let digit = |c| match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        };
        let a = digit(chunk[0])?;
        let b = digit(chunk[1])?;
        out.push((a << 2) | (b >> 4));
        if chunk[2] == b'=' {
            if i + 1 != count || chunk[3] != b'=' || b & 15 != 0 {
                return None;
            }
        } else {
            let c = digit(chunk[2])?;
            out.push((b << 4) | (c >> 2));
            if chunk[3] == b'=' {
                if i + 1 != count || c & 3 != 0 {
                    return None;
                }
            } else {
                out.push((c << 6) | digit(chunk[3])?);
            }
        }
    }
    Some(out)
}

fn valid_image(bytes: &[u8], mime: &str) -> bool {
    match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[255, 216, 255]),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        _ => false,
    }
}

pub(super) fn add_support(check: &mut Assessment, project: &str, files: &[String]) {
    let Ok(root) = Path::new(project).canonicalize() else {
        return;
    };
    let mut excerpts = Vec::new();
    for name in files.iter().take(8) {
        let Ok(path) = root.join(name).canonicalize() else {
            check
                .missing
                .push(format!("Supporting evidence file missing: {name}"));
            continue;
        };
        let label = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let lower = label.to_ascii_lowercase();
        if !path.starts_with(&root)
            || lower.starts_with('.')
            || [
                "auth.json",
                "credentials.json",
                "secrets.json",
                "tokens.json",
            ]
            .contains(&lower.as_str())
            || path.strip_prefix(&root).is_ok_and(|relative| {
                relative
                    .components()
                    .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
            })
        {
            check.missing.push("Supporting evidence path is outside project or sensitive; supply only test sources/logs/reports".into());
            continue;
        }
        if !matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("js" | "ts" | "mjs" | "json" | "log" | "txt")
        ) {
            check
                .missing
                .push(format!("Unsupported evidence text type: {name}"));
            continue;
        }
        match bounded_bytes(&path, 8193) {
            Ok(bytes) => {
                let mut data = serde_json::json!({"path":name,"excerpt":text_excerpt(&bytes[..bytes.len().min(8192)]),"truncated":bytes.len()>8192});
                if path.extension().is_some_and(|s| s == "json") {
                    if let Ok(bytes) = bounded_bytes(&path, 4 * 1024 * 1024) {
                        if let Ok(report) = serde_json::from_slice::<Value>(&bytes) {
                            if report.get("stats").is_some() {
                                data["testCounts"] = serde_json::json!({"passed":report["stats"]["expected"],"failed":report["stats"]["unexpected"],"skipped":report["stats"]["skipped"]});
                                fn failures(value: &Value, out: &mut Vec<Value>) {
                                    if out.len() >= 8 {
                                        return;
                                    }
                                    if let Some(specs) = value["specs"].as_array() {
                                        for spec in specs {
                                            for test in
                                                spec["tests"].as_array().into_iter().flatten()
                                            {
                                                for result in
                                                    test["results"].as_array().into_iter().flatten()
                                                {
                                                    if out.len() < 8 && result["status"] == "failed"
                                                    {
                                                        let error = result["errors"]
                                                            .as_array()
                                                            .into_iter()
                                                            .flatten()
                                                            .filter_map(|e| e["message"].as_str())
                                                            .take(2)
                                                            .collect::<Vec<_>>()
                                                            .join("\n");
                                                        out.push(serde_json::json!({"test":spec["title"].as_str().unwrap_or("").chars().take(200).collect::<String>(),"status":"failed","error":text_excerpt(error.as_bytes()).chars().take(1000).collect::<String>()}));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    for nested in value["suites"].as_array().into_iter().flatten() {
                                        failures(nested, out);
                                    }
                                }
                                let mut failed = Vec::new();
                                failures(&report, &mut failed);
                                data["failedTests"] = serde_json::json!(failed);
                                data.as_object_mut().unwrap().remove("excerpt");
                                // omit embedded base64 artifacts
                            }
                        }
                    }
                }
                excerpts.push(data);
            }
            Err(_) => check
                .missing
                .push(format!("Cannot read supporting evidence file: {name}")),
        }
    }
    check.complete &= check.missing.is_empty();
    check.summary["supportingEvidence"] = serde_json::json!(excerpts);
    check.summary["missing"] = serde_json::json!(check.missing);
}

pub(super) fn source_stamp(project: &str) -> std::io::Result<u64> {
    source_inventory(project).map(|(hash, _)| hash)
}

fn source_inventory(project: &str) -> std::io::Result<(u64, SystemTime)> {
    use std::hash::{Hash, Hasher};
    let root = Path::new(project).canonicalize()?;
    let mut pending = vec![root.clone()];
    let mut paths = Vec::new();
    let mut count = 0;
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            count += 1;
            if count > 10000 {
                return Err(std::io::Error::other("source scan limit exceeded"));
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.')
                || [
                    "node_modules",
                    "test-results",
                    "playwright-report",
                    "dist",
                    "build",
                    "target",
                    "uat-artifacts",
                    "uat-evidence",
                ]
                .contains(&name.as_str())
            {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                let path = entry.path();
                if matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some(
                        "js" | "ts"
                            | "jsx"
                            | "tsx"
                            | "mjs"
                            | "cjs"
                            | "css"
                            | "html"
                            | "vue"
                            | "svelte"
                    )
                ) || ["package.json", "package-lock.json", "tsconfig.json"]
                    .contains(&name.as_str())
                {
                    paths.push(path);
                }
            }
        }
    }
    paths.sort();
    let mut bytes_read = 0;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let mut latest = SystemTime::UNIX_EPOCH;
    for path in paths {
        latest = latest.max(path.metadata()?.modified()?);
        let bytes = bounded_bytes(&path, 2 * 1024 * 1024 + 1)?;
        bytes_read += bytes.len();
        if bytes.len() > 2 * 1024 * 1024 || bytes_read > 32 * 1024 * 1024 {
            return Err(std::io::Error::other("source byte limit exceeded"));
        }
        path.strip_prefix(&root).unwrap_or(&path).hash(&mut hash);
        bytes.hash(&mut hash);
    }
    Ok((hash.finish(), latest))
}

fn path_label(path: &Path) -> String {
    // Keep reviewer input bounded even when artifact paths are very long.
    path.to_string_lossy().chars().take(512).collect()
}

fn inspect_report(report: &Value, steps: &[String], roots: Option<(&Path, &Path)>) -> Assessment {
    if steps.is_empty() {
        return Assessment::invalid("Set named acceptance steps before submitting UAT evidence.");
    }
    let stats = &report["stats"];
    let mut missing = Vec::new();
    let counts_ok = stats["expected"]
        .as_u64()
        .is_some_and(|n| n >= steps.len() as u64)
        && stats["unexpected"].as_u64() == Some(0)
        && stats["skipped"].as_u64() == Some(0)
        && stats["flaky"].as_u64().unwrap_or(0) == 0;
    if !counts_ok {
        missing
            .push("The full Playwright suite has failed, skipped, flaky, or too few tests.".into());
    }
    fn specs<'a>(suites: &'a [Value], out: &mut Vec<&'a Value>) {
        for suite in suites {
            if let Some(entries) = suite["specs"].as_array() {
                out.extend(entries);
            }
            if let Some(nested) = suite["suites"].as_array() {
                specs(nested, out);
            }
        }
    }
    let mut found = Vec::new();
    if let Some(suites) = report["suites"].as_array() {
        specs(suites, &mut found);
    } else {
        return Assessment::invalid("Playwright JSON reporter has no test suites.");
    }
    let mut screenshots = Vec::new();
    let mut inline_images = Vec::new();
    let mut verified = 0;
    let mut verified_steps = Vec::new();
    for label in steps {
        let Some(test) = found
            .iter()
            .find(|spec| spec["title"].as_str() == Some(label))
            .and_then(|spec| spec["tests"].as_array())
            .and_then(|tests| tests.first())
        else {
            missing.push(format!("No browser test named {label:?}."));
            continue;
        };
        let Some(execution) = test["results"].as_array().and_then(|runs| runs.last()) else {
            missing.push(format!("Browser test {label:?} has no execution result."));
            continue;
        };
        if test["status"] != "expected" || execution["status"] != "passed" {
            missing.push(format!("Browser test {label:?} did not pass."));
            continue;
        }
        let attachments = execution["attachments"].as_array();
        let artifact = |name: &str| -> Vec<PathBuf> {
            attachments
                .into_iter()
                .flatten()
                .filter(|item| item["name"] == name)
                .filter_map(|item| {
                    if let Some(mime) = item["contentType"].as_str() {
                        if (name == "screenshot"
                            && !["image/png", "image/jpeg", "image/webp"].contains(&mime))
                            || (name == "browser-console" && mime != "text/plain")
                        {
                            return None;
                        }
                    }
                    let path = PathBuf::from(item["path"].as_str()?);
                    let candidates = match roots {
                        Some((project, report_dir)) => {
                            vec![project.join(&path), report_dir.join(&path)]
                        }
                        None => {
                            if path.is_absolute() {
                                vec![path]
                            } else {
                                Vec::new()
                            }
                        }
                    };
                    candidates.into_iter().find_map(|candidate| {
                        let path = candidate.canonicalize().ok()?;
                        if roots.is_some_and(|(project, _)| !path.starts_with(project)) {
                            return None;
                        }
                        let meta = path.metadata().ok()?;
                        (meta.is_file() && meta.len() > 0 && meta.len() <= 10 * 1024 * 1024)
                            .then_some(path)
                    })
                })
                .collect()
        };
        let inline = |name: &str| -> Vec<Vec<u8>> {
            attachments
                .into_iter()
                .flatten()
                .filter(|item| item["name"] == name)
                .filter_map(|item| {
                    let body = decode_body(item["body"].as_str()?)?;
                    let valid = match name {
                        "screenshot" => valid_image(&body, item["contentType"].as_str()?),
                        _ => {
                            item["contentType"].as_str() == Some("text/plain")
                                && std::str::from_utf8(&body).is_ok()
                        }
                    };
                    (valid && !body.is_empty()).then_some(body)
                })
                .collect()
        };
        let screenshot_paths = artifact("screenshot");
        let screenshot = screenshot_paths.first().cloned();
        let image_bodies = inline("screenshot");
        let image_body = image_bodies.first();
        if inline_images.iter().map(Vec::len).sum::<usize>()
            + image_bodies.iter().map(Vec::len).sum::<usize>()
            > 8 * 1024 * 1024
        {
            missing.push("Inline screenshots exceed the 8 MiB retained-evidence budget; submit file-backed images".into());
            continue;
        }
        if screenshot.is_none() && image_body.is_none() {
            missing.push(format!(
                "Browser test {label:?} has no nonempty screenshot attachment."
            ));
            continue;
        }
        let console = artifact("browser-console").into_iter().next();
        let console_body = inline("browser-console").into_iter().next();
        if console.is_none() && console_body.is_none() {
            missing.push(format!(
                "Browser test {label:?} has no nonempty browser-console attachment."
            ));
            continue;
        }
        verified_steps.push(serde_json::json!({"criterion":label,"status":"passed",
            "screenshot":screenshot.as_deref().map(path_label),"browserConsole":console.as_deref().map(path_label),
            "inlineScreenshotBytes":image_body.map(Vec::len),
            "screenshotBytes":screenshot.as_ref().and_then(|p| p.metadata().ok()).map(|m| m.len()),
            "consoleBytes":console.as_ref().and_then(|p| p.metadata().ok()).map(|m| m.len()),
            "consoleExcerpt":console_body.as_deref().map(text_excerpt).or_else(|| console.as_ref().and_then(|p| bounded_bytes(p,8192).ok()).map(|b| text_excerpt(&b)))}));
        screenshots.extend(screenshot_paths);
        inline_images.extend(image_bodies);
        verified += 1;
    }
    Assessment {
        summary: serde_json::json!({"source":"Playwright JSON validated by Hibiscus",
            "counts":{"passed":stats["expected"].as_u64(),"failed":stats["unexpected"].as_u64(),
                "skipped":stats["skipped"].as_u64(),"flaky":stats["flaky"].as_u64()},
            "verifiedCriteria":verified_steps,"missing":missing,
            "limits":"Test titles and attachment existence checked; assertions, console contents and pixels are not independently verified."}),
        screenshots,
        inline_images,
        verified,
        complete: counts_ok && missing.is_empty(),
        missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_or_unstructured_reports_cannot_pass() {
        let steps = vec!["Sign in".to_owned()];
        for result in [
            serde_json::json!({}),
            serde_json::json!({"content":[{"type":"text","text":"all passed"}]}),
            serde_json::json!({"content":[{"type":"text","text":"{\"stats\":{\"expected\":0,\"unexpected\":0,\"skipped\":0},\"suites\":[]}"}]}),
        ] {
            assert!(!inspect(&result, &steps).complete);
        }
    }
    #[test]
    fn saved_report_resolves_project_and_report_relative_artifacts_and_rejects_stale_files() {
        let root = (0..)
            .find_map(|id| {
                let path = std::env::temp_dir()
                    .join(format!("hibiscus-report-{}-{id}", std::process::id()));
                match std::fs::create_dir(&path) {
                    Ok(()) => Some(path.canonicalize().unwrap()),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(error) => panic!("fixture directory: {error}"),
                }
            })
            .unwrap();
        let reports = root.join("reports");
        std::fs::create_dir(&reports).unwrap();
        std::fs::write(root.join("screenshot.png"), b"synthetic pixels").unwrap();
        std::fs::write(reports.join("console.txt"), b"synthetic console log").unwrap();
        let report = serde_json::json!({"stats":{"expected":1,"unexpected":0,"skipped":0,"flaky":0},
            "suites":[{"specs":[{"title":"Sign in","tests":[{"status":"expected","results":[
                {"status":"passed","attachments":[{"name":"screenshot","path":"screenshot.png"},
                 {"name":"browser-console","path":"console.txt"}]}]}]}]}]});
        let path = reports.join("report.json");
        std::fs::write(&path, report.to_string()).unwrap();
        let steps = vec!["Sign in".to_owned()];
        let check = inspect_file(
            root.to_str().unwrap(),
            "reports/report.json",
            &steps,
            SystemTime::UNIX_EPOCH,
        );
        assert!(check.complete, "{:?}", check.missing);
        assert_eq!(check.verified, 1);
        assert_eq!(check.summary["counts"]["passed"], 1);
        assert_eq!(check.screenshots, vec![root.join("screenshot.png")]);
        let mut mixed = report.clone();
        let attachments = mixed["suites"][0]["specs"][0]["tests"][0]["results"][0]["attachments"]
            .as_array_mut()
            .unwrap();
        attachments.insert(0,serde_json::json!({"name":"screenshot","contentType":"image/png","body":"iVBORw0KGgo="}));
        std::fs::write(
            root.join("automatic.png"),
            b"synthetic automatic screenshot",
        )
        .unwrap();
        attachments.push(serde_json::json!({"name":"screenshot","path":"automatic.png"}));
        let check = inspect_report(&mixed, &steps, Some((&root, &reports)));
        assert!(check.complete);
        assert_eq!(
            check.screenshots.len(),
            2,
            "both file-backed duplicates remain readable"
        );
        assert_eq!(
            check.inline_images.len(),
            1,
            "inline-first must not hide the later paths"
        );
        let original = source_stamp(root.to_str().unwrap()).unwrap();
        std::fs::write(root.join("server.log"), b"new read-only server evidence").unwrap();
        assert_eq!(source_stamp(root.to_str().unwrap()).unwrap(), original);
        std::fs::write(root.join("app.js"), b"changed code").unwrap();
        assert_ne!(source_stamp(root.to_str().unwrap()).unwrap(), original);
        let mut supported = check;
        add_support(
            &mut supported,
            root.to_str().unwrap(),
            &["app.js".into(), "server.log".into()],
        );
        assert!(supported.summary["supportingEvidence"][0]["excerpt"]
            .as_str()
            .unwrap()
            .contains("changed code"));
        let failed = serde_json::json!({"stats":{"expected":0,"unexpected":1,"skipped":0},"suites":[{"specs":[{"title":"Delete","tests":[{"results":[{"status":"failed","errors":[{"message":"Expected no task; received one"}]}]}]}]}]});
        std::fs::write(root.join("failed-report.json"), failed.to_string()).unwrap();
        add_support(
            &mut supported,
            root.to_str().unwrap(),
            &["failed-report.json".into()],
        );
        assert_eq!(
            supported.summary["supportingEvidence"][0]["testCounts"]["failed"],
            1
        );
        assert!(
            supported.summary["supportingEvidence"][0]["failedTests"][0]["error"]
                .as_str()
                .unwrap()
                .contains("received one")
        );
        assert!(
            supported.summary["supportingEvidence"][0]
                .get("excerpt")
                .is_none(),
            "historical reports exclude raw base64"
        );
        std::fs::remove_file(reports.join("console.txt")).unwrap();
        let check = inspect_file(
            root.to_str().unwrap(),
            "reports/report.json",
            &steps,
            SystemTime::UNIX_EPOCH,
        );
        assert!(!check.complete);
        assert!(check.missing[0].contains("browser-console"));
        let old = std::fs::FileTimes::new()
            .set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(100));
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(old)
            .unwrap();
        let check = inspect_file(
            root.to_str().unwrap(),
            "reports/report.json",
            &steps,
            SystemTime::now(),
        );
        assert!(check.missing[0].contains("predates"));
        let check = inspect_file(
            root.to_str().unwrap(),
            "reports",
            &steps,
            SystemTime::UNIX_EPOCH,
        );
        assert!(!check.complete, "directories are not reports");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inline_attachments_accept_valid_bodies_and_reject_malformed_or_wrong_types() {
        let report = |mime: &str, body: &str| {
            serde_json::json!({"stats":{"expected":1,"unexpected":0,"skipped":0,"flaky":0},
            "suites":[{"specs":[{"title":"Flow","tests":[{"status":"expected","results":[{"status":"passed","attachments":[
                {"name":"screenshot","contentType":mime,"body":body},
                {"name":"browser-console","contentType":"text/plain","body":"bm8gZXJyb3Jz"}
            ]}]}]}]}]})
        };
        let steps = vec!["Flow".into()];
        let check = inspect_report(&report("image/png", "iVBORw0KGgo="), &steps, None);
        assert!(check.complete, "{:?}", check.missing);
        assert_eq!(check.inline_images[0], b"\x89PNG\r\n\x1a\n");
        assert_eq!(
            check.summary["verifiedCriteria"][0]["consoleExcerpt"],
            "no errors"
        );
        for (mime, body) in [
            ("text/plain", "iVBORw0KGgo="),
            ("image/png", "invalid@@"),
            ("image/png", ""),
        ] {
            assert!(!inspect_report(&report(mime, body), &steps, None).complete);
        }
        assert!(decode_body("Zg==xxxx").is_none());
        assert!(decode_body("Zh==").is_none());
        assert!(decode_body(&"a".repeat(4 * 1024 * 1024 + 4)).is_none());
    }

    #[test]
    fn reports_all_missing_steps_together() {
        let report = serde_json::json!({"stats":{"expected":1,"unexpected":0,"skipped":0,"flaky":0},
            "suites":[{"specs":[{"title":"Sign in","tests":[{"status":"expected","results":[
                {"status":"passed","attachments":[]}]}]}]}]});
        let result = serde_json::json!({"content":[{"type":"text","text":report.to_string()}]});
        let check = inspect(&result, &["Sign in".into(), "Sign out".into()]);
        assert_eq!(check.verified, 0);
        assert_eq!(
            check.missing.len(),
            3,
            "suite count, missing screenshot and missing test"
        );
    }
}
