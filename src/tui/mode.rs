#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Tui,
    Cli,
}
impl Mode {
    pub(crate) fn switch_command(self) -> &'static str {
        match self {
            Self::Tui => "/cli",
            Self::Cli => "/tui",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Tui => "TUI",
            Self::Cli => "CLI",
        }
    }
}
