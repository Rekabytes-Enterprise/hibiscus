use crate::ui::welcome::{commands, Context};
pub(super) fn render(context: Context<'_>, color: bool) -> String {
    let (brand, muted, reset) = if color {
        (
            "\x1b[1;38;2;225;80;130m",
            "\x1b[38;2;155;155;155m",
            "\x1b[0m",
        )
    } else {
        ("", "", "")
    };
    let clean = |text: &str| text.chars().filter(|c| !c.is_control()).collect::<String>();
    format!(
        "{brand}hibiscus{reset}{muted} {} · {} · CLI\n{}    {}{reset}\n",
        env!("CARGO_PKG_VERSION"),
        clean(context.workspace),
        clean(context.model),
        commands(&context),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::mode::Mode;
    #[test]
    fn tips_offer_tui_and_hide_login_with_credentials() {
        for has_credentials in [false, true] {
            let text = render(
                Context {
                    workspace: "workspace",
                    model: "provider/model",
                    mode: Mode::Cli,
                    has_credentials,
                },
                false,
            );
            assert!(text.contains("/tui"));
            assert_eq!(text.contains("/login"), !has_credentials);
            assert!(text.contains("CLI"));
        }
    }
}
