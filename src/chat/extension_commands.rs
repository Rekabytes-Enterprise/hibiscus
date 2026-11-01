//! Local extension command invocation; never creates a model prompt.
use super::*;
pub(super) fn namespaced(message: &str) -> bool {
    message
        .split_whitespace()
        .next()
        .is_some_and(crate::extensions::is_command_token)
}
#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch<R: BufRead>(
    message: &str,
    rpc: &mut Client,
    input: &mut R,
    output: &mut Frontend,
    dialogs: &mut Dialogs,
    raw: &mut Option<RawMode>,
    events: Option<&Receiver<u8>>,
    interactive: bool,
) -> Result<()> {
    let (name, argument) = message
        .split_once(char::is_whitespace)
        .map_or((message, ""), |(name, arg)| (name, arg.trim_start()));
    rpc.command(
        json!({"type":"extension_command","name":name,"input":argument}),
        &[],
        output,
        input,
        dialogs,
        events,
        raw,
        interactive,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namespace_tokens_stay_local_while_absolute_paths_remain_prompts() {
        assert!(namespaced("/demo:run argument"));
        assert!(namespaced("/demo:run"));
        assert!(namespaced("/Demo:run"));
        assert!(namespaced("/demo:run:extra"));
        for text in [
            "/tmp/file:42 explain this",
            "/help",
            "/compact text:note",
            "demo:run",
        ] {
            assert!(!namespaced(text));
        }
    }
}
