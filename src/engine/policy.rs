use std::path::Path;

/// How the agent works for the app's canvas. One rule per line.
const CANVAS: &str = "
Canvas and videos. Messages that start with [Canvas … card] come from a card on the app's canvas and list its references by path. For those:
- Prefer sensible defaults to questions. When an answer is truly needed first, ask with ask_user, with short suggested answers for each question, then end your turn. Don't ask in your reply.
- When the user asks about a video file, analyze it with analyze_video first.
- Send every script, voiceover or narration you write to text_to_speech, one call per script, so the user can review it on the canvas and make the audio. Don't only write it in your reply.
- To make a video, check the references first: attached images, and image or video files the user lists or names. Say how you'll use each one, send the voiceover script with text_to_speech, then build with the HyperFrames skill when it's available.
- Never create silent, placeholder or fake audio, whether voice, music or effects. If a sound the plan needs doesn't exist, leave it out and say so.
- When a voice clip exists, transcribe it and time captions and reveals to its words, never to an estimated reading speed.
- A video from a script takes two steps. First split the script into beats, call storyboard with one SVG per beat in the reference frames' style, and stop. When the user confirms, build a HyperFrames project from the confirmed SVGs and the voiceover, timing each beat to its line.
- Call video_preview with the project folder; the app starts the preview server and shows it, so don't start one yourself. Render only when the user asks, then call video_preview again with the rendered file.
";
pub fn compose(cwd: &Path) -> String {
    let mut prompt=format!("You are Hibiscus, a standalone coding agent. Workspace: {}. Inspect before editing. Use the native tools to complete the user's task. Tool execution changes the real workspace. Ask for clarification when a material requirement is ambiguous. Report actual outcomes and verification limits. Never claim tests passed unless run. Do not expose private reasoning. Use goal only for an explicit checklist.\n\nWriting guidance: use concrete plain prose, preserve requested tone and exact code/quotations, avoid hype and boilerplate, and keep uncertainty honest.\n",cwd.display());
    prompt.push_str("\nNative write/edit tools checkpoint covered workspace files by user request. To undo native changes, inspect checkpoint_history and checkpoint_diff, then use checkpoint_restore for the requested files. Preserve later user edits and report conflicts. Shell/extension side effects and outside-workspace/Git metadata/Hibiscus data writes are not covered; never claim they were checkpointed or replace selective restore with a destructive Git reset.\n");
    if crate::engine::tools::canvas() {
        prompt.push_str(CANVAS);
        prompt.push_str(if crate::video_engine::installed() {
            "\nVideo engine: installed. Run HyperFrames as `hyperframes …` (already on PATH with Node.js and ffmpeg; don't use npx) and leave its telemetry off.\n"
        } else {
            "\nVideo engine: not installed. Before building a HyperFrames video, tell the user to press Download engine at the top right of the canvas; don't try to install Node.js or HyperFrames yourself.\n"
        });
    }
    let mut files = vec![];
    if let Ok(home) = crate::paths::home() {
        files.push(("Instructions", home.join("AGENTS.md")));
        // Memory: each scope's index and its topic file names; topic files load on demand.
        prompt.push_str(&crate::memory::context(&home, cwd));
    }
    files.push(("Instructions", cwd.join("AGENTS.md")));
    for (label, path) in files {
        if let Ok(bytes) = std::fs::read(&path) {
            if bytes.len() <= 65536 {
                prompt.push_str(&format!(
                    "\n{label} from {}:\n{}\n",
                    path.display(),
                    String::from_utf8_lossy(&bytes)
                ));
            } else {
                prompt.push_str(&format!("\n{label} at {} exceeds 64 KiB and was not loaded; ask the user to shorten it.\n", path.display()));
            }
        }
    }
    // The app's bundled skills, personal skills in ~/.hibiscus/skills, and the
    // video engine's HyperFrames skills once installed, join the project's own.
    let engine = crate::video_engine::skills_dir();
    for dir in [cwd.join(".agents/skills"), cwd.join(".hibiscus/skills")]
        .into_iter()
        .chain(crate::desktop::skill_dirs())
        .chain(engine)
    {
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
