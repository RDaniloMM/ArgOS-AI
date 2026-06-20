use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;
use crate::state::FocusPane;

pub fn map_key(key: KeyEvent, focus: FocusPane) -> Option<Action> {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
        KeyCode::Tab => Some(if focus == FocusPane::Composer {
            Action::ComposerAutocomplete
        } else {
            Action::FocusNext
        }),
        KeyCode::BackTab => Some(Action::FocusPrev),
        KeyCode::Esc => Some(Action::Escape),
        KeyCode::F(5) => Some(Action::SubmitPrompt),
        KeyCode::F(6) => Some(Action::RunSelectedWorkflow),
        KeyCode::F(7) => Some(Action::ToggleActivity),
        KeyCode::F(8) => Some(Action::ToggleSidebar),
        KeyCode::F(2) => Some(Action::ShowProviderPopup),
        KeyCode::Char('c') if ctrl && shift => Some(Action::CopySelection),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::Home if focus == FocusPane::Composer && shift => Some(Action::ComposerSelectHome),
        KeyCode::Home if focus == FocusPane::Composer => Some(Action::ComposerMoveHome),
        KeyCode::End if focus == FocusPane::Composer && shift => Some(Action::ComposerSelectEnd),
        KeyCode::End if focus == FocusPane::Composer => Some(Action::ComposerMoveEnd),
        KeyCode::Up => Some(if focus == FocusPane::Composer {
            if shift {
                Action::ComposerSelectUp
            } else {
                Action::ComposerMoveUp
            }
        } else {
            Action::MoveUp
        }),
        KeyCode::Down => Some(if focus == FocusPane::Composer {
            if shift {
                Action::ComposerSelectDown
            } else {
                Action::ComposerMoveDown
            }
        } else {
            Action::MoveDown
        }),
        KeyCode::Left => Some(if focus == FocusPane::Composer {
            if shift {
                Action::ComposerSelectLeft
            } else {
                Action::ComposerMoveLeft
            }
        } else {
            Action::FocusPrev
        }),
        KeyCode::Right => Some(if focus == FocusPane::Composer {
            if shift {
                Action::ComposerSelectRight
            } else {
                Action::ComposerMoveRight
            }
        } else {
            Action::FocusNext
        }),
        KeyCode::Char('q') if focus != FocusPane::Composer => Some(Action::Quit),
        KeyCode::Char('r') if focus != FocusPane::Composer => Some(Action::Refresh),
        KeyCode::Char('j') if focus != FocusPane::Composer => Some(Action::MoveDown),
        KeyCode::Char('k') if focus != FocusPane::Composer => Some(Action::MoveUp),
        KeyCode::Char('p') if ctrl => Some(Action::ShowCommandPalette),
        KeyCode::Enter if focus == FocusPane::Composer && shift => Some(Action::ComposerNewline),
        KeyCode::Enter if focus == FocusPane::Composer => Some(Action::SubmitPrompt),
        KeyCode::Backspace if focus == FocusPane::Composer => Some(Action::ComposerBackspace),
        KeyCode::Char(ch) if focus == FocusPane::Composer && is_plain_text(key.modifiers) => {
            Some(Action::ComposerInsert(ch))
        }
        _ => None,
    }
}

fn is_plain_text(modifiers: KeyModifiers) -> bool {
    modifiers.is_empty() || modifiers == KeyModifiers::SHIFT
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::map_key;
    use crate::action::Action;
    use crate::state::FocusPane;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn f5_always_submits() {
        assert_eq!(
            map_key(key(KeyCode::F(5)), FocusPane::Composer),
            Some(Action::SubmitPrompt)
        );
        assert_eq!(
            map_key(key(KeyCode::F(5)), FocusPane::Workflows),
            Some(Action::SubmitPrompt)
        );
    }

    #[test]
    fn composer_enter_submits_and_shift_enter_keeps_newline() {
        assert_eq!(
            map_key(key(KeyCode::Enter), FocusPane::Composer),
            Some(Action::SubmitPrompt)
        );
        assert_eq!(
            map_key(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
                FocusPane::Composer
            ),
            Some(Action::ComposerNewline)
        );
    }

    #[test]
    fn composer_chars_are_inserted_while_typing() {
        assert_eq!(
            map_key(key(KeyCode::Char('r')), FocusPane::Composer),
            Some(Action::ComposerInsert('r'))
        );
        assert_eq!(
            map_key(key(KeyCode::Char('q')), FocusPane::Composer),
            Some(Action::ComposerInsert('q'))
        );
    }

    #[test]
    fn navigation_keys_follow_focus_context() {
        assert_eq!(
            map_key(key(KeyCode::Up), FocusPane::Composer),
            Some(Action::ComposerMoveUp)
        );
        assert_eq!(
            map_key(key(KeyCode::Up), FocusPane::Activity),
            Some(Action::MoveUp)
        );
        assert_eq!(
            map_key(key(KeyCode::Char('j')), FocusPane::Workflows),
            Some(Action::MoveDown)
        );
    }

    #[test]
    fn ctrl_p_opens_command_palette_and_f2_keeps_provider_picker() {
        assert_eq!(
            map_key(
                KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
                FocusPane::Composer
            ),
            Some(Action::ShowCommandPalette)
        );
        assert_eq!(
            map_key(key(KeyCode::F(2)), FocusPane::Composer),
            Some(Action::ShowProviderPopup)
        );
    }

    #[test]
    fn composer_shift_navigation_extends_selection() {
        assert_eq!(
            map_key(
                KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT),
                FocusPane::Composer
            ),
            Some(Action::ComposerSelectLeft)
        );
        assert_eq!(
            map_key(
                KeyEvent::new(KeyCode::Home, KeyModifiers::SHIFT),
                FocusPane::Composer
            ),
            Some(Action::ComposerSelectHome)
        );
    }
}
