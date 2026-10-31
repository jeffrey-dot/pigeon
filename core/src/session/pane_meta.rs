//! What a pane reports about itself (program-driven, never guessed from the screen).

/// The pane's last reported cwd (OSC 7) and title (OSC 0/2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaneMeta {
    pub cwd: Option<String>,
    pub title: Option<String>,
}

impl PaneMeta {
    /// The title, but only when it names a program: the sidebar calls a pane `vim`, not
    /// `~/dev`.
    pub fn program(&self) -> Option<&str> {
        self.title
            .as_deref()
            .filter(|title| !looks_like_a_prompt(title))
    }
}

/// A shell prompt or a path is not a program name: fish titles its window `~/dev`, zsh sets
/// the command, and the sidebar should read the same on both.
fn looks_like_a_prompt(title: &str) -> bool {
    let title = title.trim();
    title.is_empty()
        || title.contains('/')
        || title.contains('~')
        || title.contains('@')
        || title.contains(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titled(title: &str) -> PaneMeta {
        PaneMeta {
            cwd: None,
            title: Some(title.to_owned()),
        }
    }

    #[test]
    fn a_program_title_names_the_pane() {
        assert_eq!(titled("vim").program(), Some("vim"));
        assert_eq!(titled("htop").program(), Some("htop"));
    }

    #[test]
    fn prompts_paths_and_blank_titles_are_not_programs() {
        for title in ["~/dev", "/usr/bin", "me@box", "host:dir", "", "   "] {
            assert_eq!(titled(title).program(), None, "{title:?}");
        }
        assert_eq!(PaneMeta::default().program(), None);
    }
}
