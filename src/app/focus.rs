#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppFocus {
    #[default]
    Sidebar,
    Terminal,
    Conversation,
    Diff,
    Agents,
}

impl AppFocus {
    /// Returns whether an overlay occupies the main pane instead of the live terminal.
    pub fn overlay_owns_main(self) -> bool {
        matches!(self, Self::Conversation | Self::Diff | Self::Agents)
    }
}

#[cfg(test)]
mod tests {
    use super::AppFocus;

    #[test]
    fn overlay_owns_main_matches_which_focus_covers_the_terminal() {
        let cases = [
            (AppFocus::Sidebar, false),
            (AppFocus::Terminal, false),
            (AppFocus::Conversation, true),
            (AppFocus::Diff, true),
            (AppFocus::Agents, true),
        ];
        for (focus, expected) in cases {
            assert_eq!(
                focus.overlay_owns_main(),
                expected,
                "{focus:?} overlay_owns_main"
            );
        }
    }
}
