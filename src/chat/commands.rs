/// Commands implemented by Hibiscus itself. Suggestions must not advertise
/// Pi-only or unimplemented commands: anything selected here is submitted to
/// the same chat command dispatcher as a manually typed name.
#[derive(Clone, Copy)]
pub(crate) struct Command {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
}

const COMMANDS: &[Command] = &[
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
        description: "Sign in to Codex or open Pi",
    },
    Command {
        name: "/logout",
        description: "Open Pi for provider logout",
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
        name: "/quit",
        description: "Leave Hibiscus",
    },
    Command {
        name: "/sessions",
        description: "Browse saved sessions",
    },
];

pub(crate) fn matches(prefix: &str) -> Vec<&'static Command> {
    if !prefix.starts_with('/') || prefix.chars().any(char::is_whitespace) {
        return Vec::new();
    }
    COMMANDS
        .iter()
        .filter(|cmd| cmd.name.starts_with(prefix))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suggestions_filter_prefix_and_never_include_pi_only_commands() {
        assert_eq!(
            matches("/m").iter().map(|cmd| cmd.name).collect::<Vec<_>>(),
            ["/models"]
        );
        assert_eq!(
            matches("/lo")
                .iter()
                .map(|cmd| cmd.name)
                .collect::<Vec<_>>(),
            ["/login", "/logout"]
        );
        assert!(matches("/model ").is_empty());
        assert!(matches("hello").is_empty());
        assert!(!matches("/").iter().any(|cmd| cmd.name == "/fork"));
    }
}
