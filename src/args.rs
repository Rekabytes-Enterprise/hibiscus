//! Command-line arguments and the --help text.
use crate::Result;

pub(crate) const USAGE: &str = "Usage: hibiscus [PROMPT...]\n       hibiscus --cli [--continue]\n       hibiscus --tui [--continue]\n       hibiscus --continue\n       hibiscus --sessions\n       hibiscus --version\n       hibiscus update\n       hibiscus --help\n\nThe update command updates Hibiscus.\nWithout a prompt, start a chat (or read prompts from piped stdin).\nTUI is the default interactive view. Use /cli for inline chat or /tui to return to full screen.\nIn chat: /<id>:<command> [input], /cli, /tui, /new, /continue, /sessions, /models, /thinking [level], /compact [instructions], /login, /logout, /restore, /reconnect, /quit.\nRooxcode models are discovered automatically without keys or configuration; sending to them needs a Reka Bytes sign-in from /login. /login offers browser sign-in for Codex and Reka Bytes, or a key for other providers. Other providers and models are configured in ~/.hibiscus/config.json. Authentication errors stay inside Hibiscus; fix the reported dependency or provider problem and retry.\n/logout removes a selected provider's stored credentials. CLI temporarily opens native full-screen login/logout dialogs, then returns inline.\n--continue resumes the latest session in this directory.\n--sessions lets you select a saved session in this directory.\nSet HIBISCUS_HOME to override the Hibiscus data directory.";

pub(crate) enum Start {
    Chat { resume: bool },
    Cli { resume: bool },
    Sessions,
    Prompt(String),
    Help,
    Version,
    Update,
}

pub(crate) fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Start> {
    match args.next().as_deref() {
        Some("--help" | "-h") => Ok(Start::Help),
        Some(mode @ ("--cli" | "--tui")) => {
            let resume = match args.next().as_deref() {
                None => false,
                Some("--continue" | "-c") if args.next().is_none() => true,
                _ => return Err("Use --cli or --tui, optionally followed by --continue.".into()),
            };
            Ok(if mode == "--cli" {
                Start::Cli { resume }
            } else {
                Start::Chat { resume }
            })
        }
        Some("update") => {
            if args.next().is_some() {
                return Err("update takes no arguments".into());
            }
            Ok(Start::Update)
        }
        Some("--version" | "-V") => {
            if args.next().is_some() {
                return Err("--version takes no arguments".into());
            }
            Ok(Start::Version)
        }
        Some("--continue" | "-c") => {
            if args.next().is_some() {
                return Err("--continue takes no prompt; start chat and type your message".into());
            }
            Ok(Start::Chat { resume: true })
        }
        Some("--sessions") => {
            if args.next().is_some() {
                return Err("--sessions takes no prompt".into());
            }
            Ok(Start::Sessions)
        }
        Some(first) => Ok(Start::Prompt(
            std::iter::once(first.to_owned())
                .chain(args)
                .collect::<Vec<_>>()
                .join(" "),
        )),
        None => Ok(Start::Chat { resume: false }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_modes() {
        assert!(matches!(
            parse_args(["--cli".into()].into_iter()).unwrap(),
            Start::Cli { resume: false }
        ));
        assert!(matches!(
            parse_args(["--cli".into(), "--continue".into()].into_iter()).unwrap(),
            Start::Cli { resume: true }
        ));
        assert!(matches!(
            parse_args(["--tui".into()].into_iter()).unwrap(),
            Start::Chat { resume: false }
        ));
        assert!(parse_args(["--cli".into(), "hello".into()].into_iter()).is_err());
        assert!(matches!(
            parse_args([].into_iter()).unwrap(),
            Start::Chat { resume: false }
        ));
        assert!(matches!(
            parse_args(["--continue".into()].into_iter()).unwrap(),
            Start::Chat { resume: true }
        ));
        assert!(
            matches!(parse_args(["hello".into(), "world".into()].into_iter()).unwrap(), Start::Prompt(text) if text == "hello world")
        );
        assert!(parse_args(["--continue".into(), "extra".into()].into_iter()).is_err());
        assert!(matches!(
            parse_args(["--sessions".into()].into_iter()).unwrap(),
            Start::Sessions
        ));
        assert!(matches!(
            parse_args(["--version".into()].into_iter()).unwrap(),
            Start::Version
        ));
        assert!(matches!(
            parse_args(["update".into()].into_iter()).unwrap(),
            Start::Update
        ));
        assert!(parse_args(["update".into(), "extra".into()].into_iter()).is_err());
        assert!(parse_args(["-V".into(), "extra".into()].into_iter()).is_err());
    }
}
