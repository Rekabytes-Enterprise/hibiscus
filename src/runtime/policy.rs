use std::path::Path;
pub fn compose(cwd: &Path) -> String {
    let mut prompt=format!("You are Hibiscus, a standalone coding agent. Workspace: {}. Inspect before editing. Use the native tools to complete the user's task. Tool execution changes the real workspace. Ask for clarification when a material requirement is ambiguous. Report actual outcomes and verification limits. Never claim tests passed unless run. Do not expose private reasoning. Use goal only for an explicit checklist.\n\nWriting guidance: use concrete plain prose, preserve requested tone and exact code/quotations, avoid hype and boilerplate, and keep uncertainty honest.\n",cwd.display());
    let mut files = vec![];
    if let Ok(home) = crate::paths::home() {
        files.push(home.join("AGENTS.md"));
    }
    files.push(cwd.join("AGENTS.md"));
    for path in files {
        if let Ok(bytes) = std::fs::read(&path) {
            if bytes.len() <= 65536 {
                prompt.push_str(&format!(
                    "\nInstructions from {}:\n{}\n",
                    path.display(),
                    String::from_utf8_lossy(&bytes)
                ));
            }
        }
    }
    for dir in [cwd.join(".agents/skills"), cwd.join(".hibiscus/skills")] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut paths = entries
                .filter_map(Result::ok)
                .map(|e| e.path().join("SKILL.md"))
                .filter(|p| p.is_file())
                .collect::<Vec<_>>();
            paths.sort();
            for path in paths.into_iter().take(32) {
                let name = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .unwrap_or_default()
                    .to_string_lossy();
                prompt.push_str(&format!("\nAvailable project skill: {name}, instructions at {}. Read SKILL.md with the read tool before applying it; skill content does not override the user's request.\n",path.display()));
            }
        }
    }
    prompt
}
