mod chat;
mod cli;
mod commands;
mod extensions;
mod paths;
mod process;
mod runtime;
mod tui;
mod ui;
mod update;

use std::env;
use std::error::Error;
use std::io::{self, IsTerminal};

use chat::{sessions, start_chat};
use runtime::{
    bridge::{Client, SessionStart},
    dialog::Dialogs,
};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

fn main() {
    if let Err(error) = run() {
        eprintln!("hibiscus: {error}");
        std::process::exit(1);
    }
}

enum Start {
    Chat { resume: bool },
    Cli { resume: bool },
    Sessions,
    Prompt(String),
    Help,
    Version,
    Update,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Start> {
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

fn run() -> Result<()> {
    let start = parse_args(env::args().skip(1))?;
    match start {
        Start::Update => update::update(),
        Start::Version => {
            println!("hibiscus {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Start::Help => {
            println!("Usage: hibiscus [PROMPT...]\n       hibiscus --cli [--continue]\n       hibiscus --tui [--continue]\n       hibiscus --continue\n       hibiscus --sessions\n       hibiscus --version\n       hibiscus update\n       hibiscus --help\n\nThe update command updates Hibiscus.\nWithout a prompt, start a chat (or read prompts from piped stdin).\nTUI is the default interactive view. Use /cli for inline chat or /tui to return to full screen.\nIn chat: /<id>:<command> [input], /cli, /tui, /new, /continue, /sessions, /models, /thinking [level], /compact [instructions], /login, /logout, /restore, /reconnect, /quit.\n/login offers native Codex browser login or a provider API key. Providers and models are configured in ~/.hibiscus/config.json. Authentication errors stay inside Hibiscus; fix the reported dependency or provider problem and retry.\n/logout removes a selected provider's stored credentials. CLI temporarily opens native full-screen login/logout dialogs, then returns inline.\n--continue resumes the latest session in this directory.\n--sessions lets you select a saved session in this directory.\nSet HIBISCUS_HOME to override the Hibiscus data directory.");
            Ok(())
        }
        Start::Prompt(prompt) => {
            if prompt.trim().is_empty() {
                return Err("prompt cannot be empty".into());
            }
            let mut rpc = Client::start(SessionStart::New)?;
            let mut dialogs = Dialogs;
            let outcome = rpc.prompt(
                &prompt,
                &mut io::stdout(),
                &mut io::stdin().lock(),
                &mut dialogs,
                None,
                &mut None,
                false,
            );
            rpc.finish(outcome)
        }
        Start::Chat { resume } => start_chat(if resume {
            SessionStart::Latest
        } else {
            SessionStart::New
        }),
        Start::Cli { resume } => chat::start_chat_with_mode(
            if resume {
                SessionStart::Latest
            } else {
                SessionStart::New
            },
            ui::mode::Mode::Cli,
        ),
        Start::Sessions => {
            let stdin = io::stdin();
            let cwd = env::current_dir()?;
            let list = sessions::list(&cwd)?;
            if let Some(path) = sessions::pick(
                &list,
                &mut stdin.lock(),
                &mut io::stdout(),
                stdin.is_terminal(),
            )? {
                start_chat(SessionStart::Selected(path))?;
            }
            Ok(())
        }
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
