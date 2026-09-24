mod chat;
mod pi;
mod tui;

use std::env;
use std::error::Error;
use std::io::{self, IsTerminal};

use chat::{sessions, start_chat};
use pi::{
    dialog::Dialogs,
    rpc::{Rpc, SessionStart},
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
    Sessions,
    Prompt(String),
    Help,
    Version,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Start> {
    match args.next().as_deref() {
        Some("--help" | "-h") => Ok(Start::Help),
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
    match parse_args(env::args().skip(1))? {
        Start::Version => {
            println!("hibiscus {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Start::Help => {
            println!("Usage: hibiscus [PROMPT...]\n       hibiscus --continue\n       hibiscus --sessions\n       hibiscus --version\n       hibiscus --help\n\nWithout a prompt, start a chat (or read prompts from piped stdin).\nIn chat: /new, /continue, /sessions, /models, /login, /logout, /quit.\n/login uses Pi's Codex SDK flow in the full-screen UI when available; other providers and /logout hand off to Pi. Finish there and type /quit to return.\n--continue resumes the latest session in this directory.\n--sessions lets you select a saved session in this directory.\nSet HIBISCUS_PI to override the pi executable.");
            Ok(())
        }
        Start::Prompt(prompt) => {
            if prompt.trim().is_empty() {
                return Err("prompt cannot be empty".into());
            }
            let mut rpc = Rpc::start(SessionStart::New)?;
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
        assert!(parse_args(["-V".into(), "extra".into()].into_iter()).is_err());
    }
}
