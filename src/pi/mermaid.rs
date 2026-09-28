//! Optional display-only Mermaid rendering through the selected Pi installation.
//! A missing Pi package or unsupported diagram leaves the original fence intact.
use super::auth::sdk_entry;
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub(crate) fn render(sources: &[String]) -> HashMap<String, Vec<String>> {
    let Some(sdk) = sdk_entry() else {
        return HashMap::new();
    };
    render_with_sdk(sources, &sdk)
}

fn render_with_sdk(sources: &[String], sdk: &std::path::Path) -> HashMap<String, Vec<String>> {
    if sources.is_empty()
        || sources.len() > 8
        || sources.iter().map(String::len).sum::<usize>() > 8192
    {
        return HashMap::new();
    }
    let Ok(mut child) =
        Command::new(std::env::var("HIBISCUS_NODE").unwrap_or_else(|_| "node".into()))
            .args([
                "--input-type=module",
                "--eval",
                include_str!("mermaid.mjs"),
                "--",
            ])
            .arg(sdk)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
    else {
        return HashMap::new();
    };
    let payload = serde_json::to_vec(sources).unwrap_or_default();
    if child
        .stdin
        .take()
        .is_none_or(|mut input| input.write_all(&payload).is_err())
    {
        let _ = child.kill();
        let _ = child.wait();
        return HashMap::new();
    }
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return HashMap::new();
    };
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut output = Vec::new();
        let read = stdout.take(32 * 1024).read_to_end(&mut output);
        let _ = sender.send(read.map(|_| output));
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut status = None;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(exit)) => {
                status = Some(exit);
                break;
            }
            Err(_) => break,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    }
    if status.is_none() {
        // The helper is dedicated to this render request, never Pi's RPC child.
        unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
        let _ = child.kill();
        let _ = child.wait();
        return HashMap::new();
    }
    if !status.is_some_and(|exit| exit.success()) {
        return HashMap::new();
    }
    let Ok(Ok(output)) = receiver.recv_timeout(Duration::from_millis(100)) else {
        return HashMap::new();
    };
    let Ok(values) = serde_json::from_slice::<Vec<Value>>(&output) else {
        return HashMap::new();
    };
    values
        .into_iter()
        .filter_map(|value| {
            let source = value["source"].as_str()?;
            if !sources.iter().any(|known| known == source) {
                return None;
            }
            let lines: Vec<String> = value["lines"]
                .as_array()?
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect::<Option<_>>()?;
            if lines.is_empty()
                || lines.len() > 60
                || lines
                    .iter()
                    .any(|line| line.chars().count() > 160 || line.chars().any(|c| c.is_control()))
            {
                return None;
            }
            Some((source.to_owned(), lines))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn selected_pi_module_renders_without_using_a_provider() {
        static ID: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hibiscus-mermaid-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        let pkg = root.join("node_modules/grok-mermaid");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(root.join("sdk.js"), "").unwrap();
        std::fs::write(
            pkg.join("package.json"),
            r#"{"type":"module","exports":"./index.js"}"#,
        )
        .unwrap();
        std::fs::write(pkg.join("index.js"), "export function render(src) { return src.includes('bad') ? null : { warnings: [], width: 4, plain: ['┌──┐', '│ok│', '└──┘'] }; }\n").unwrap();
        let sources = ["flowchart LR\n A --> B\n".into(), "bad".into()];
        let diagrams = render_with_sdk(&sources, &root.join("sdk.js"));
        assert_eq!(diagrams[&sources[0]], ["┌──┐", "│ok│", "└──┘"]);
        assert!(!diagrams.contains_key("bad"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
