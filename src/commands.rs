/// Commands implemented by Hibiscus itself. Suggestions must not advertise
/// runtime-only or unimplemented commands: anything selected here is submitted to
/// the same chat command dispatcher as a manually typed name.
#[derive(Clone, Copy)]
pub(crate) struct Command<'a> {
    pub(crate) name: &'a str,
    pub(crate) description: &'a str,
}

const COMMANDS: &[Command<'static>] = &[
    Command {
        name: "/cli",
        description: "Switch to inline terminal chat",
    },
    Command {
        name: "/tui",
        description: "Switch to full-screen terminal chat",
    },
    Command {
        name: "/compact",
        description: "Summarize old context with runtime",
    },
    Command {
        name: "/continue",
        description: "Resume the latest session",
    },
    Command {
        name: "/exit",
        description: "Leave Hibiscus",
    },
    Command {
        name: "/help",
        description: "Show commands and keys",
    },
    Command {
        name: "/login",
        description: "Browser login or API key",
    },
    Command {
        name: "/logout",
        description: "Remove a stored provider credential",
    },
    Command {
        name: "/models",
        description: "Choose a model",
    },
    Command {
        name: "/new",
        description: "Start a new session",
    },
    Command {
        name: "/output",
        description: "Inspect saved tool output locally",
    },
    Command {
        name: "/quit",
        description: "Leave Hibiscus",
    },
    Command {
        name: "/reconnect",
        description: "Reconnect Runtime without resending",
    },
    Command {
        name: "/restore",
        description: "Restore a failed prompt for review",
    },
    Command {
        name: "/sessions",
        description: "Browse saved sessions",
    },
    Command {
        name: "/thinking",
        description: "Choose Runtime reasoning level",
    },
];

pub(crate) fn looks_like_command(name: &str) -> bool {
    name.strip_prefix('/').is_some_and(|name| {
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b':'))
    })
}
pub(crate) fn reserved(name: &str) -> bool {
    COMMANDS.iter().any(|command| command.name == name)
}

pub(crate) fn matches<'a>(
    prefix: &str,
    extensions: &'a [crate::extensions::CommandInfo],
) -> Vec<Command<'a>> {
    if !prefix.starts_with('/') || prefix.chars().any(char::is_whitespace) {
        return Vec::new();
    }
    COMMANDS
        .iter()
        .copied()
        .chain(extensions.iter().map(|c| Command {
            name: &c.name,
            description: &c.description,
        }))
        .filter(|c| c.name.starts_with(prefix))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suggestions_filter_prefix_and_never_include_pi_only_commands() {
        assert_eq!(
            matches("/m", &[])
                .iter()
                .map(|cmd| cmd.name)
                .collect::<Vec<_>>(),
            ["/models"]
        );
        assert_eq!(
            matches("/lo", &[])
                .iter()
                .map(|cmd| cmd.name)
                .collect::<Vec<_>>(),
            ["/login", "/logout"]
        );
        assert_eq!(
            matches("/thi", &[])
                .iter()
                .map(|cmd| cmd.name)
                .collect::<Vec<_>>(),
            ["/thinking"]
        );
        assert_eq!(
            matches("/comp", &[])
                .iter()
                .map(|cmd| cmd.name)
                .collect::<Vec<_>>(),
            ["/compact"]
        );
        assert!(matches("/model ", &[]).is_empty());
        assert!(matches("hello", &[]).is_empty());
        assert!(!matches("/", &[]).iter().any(|cmd| cmd.name == "/fork"));
        assert!(matches("/loop", &[]).is_empty());
        assert!(matches("/approval", &[]).is_empty());
        assert!(matches("/extension", &[]).is_empty());
        assert!(!reserved("/extensions"));
        assert!(looks_like_command("/extensions"));
        assert!(!looks_like_command("/tmp/file:42"));
    }
}
