use crossterm::event::{KeyCode, KeyEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserAction {
    None,
    Quit,
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    Confirm,                // Enter
    BackToBenchmarkList,    // Esc
    RotateCodeViewMode,     // Tab
    RotateCodeViewModePrev, // Shift-Tab
    ShowCSource,            // 'c'
    Run,                    // 'r' → BuildAndRun
    Analyze,                // 'a' → AnalyzeFast
    ClearSession,           // 'C'
    CopyDetailToClipboard,  // 'y'
    Backspace,              // Backspace
    TextChar(char),         // Generic text input for config modal
}

pub fn map_key_event(key: KeyEvent) -> UserAction {
    match key.code {
        KeyCode::Char('q') => UserAction::Quit,
        KeyCode::Char('r') => UserAction::Run,
        KeyCode::Char('a') => UserAction::Analyze,
        KeyCode::Char('c') => UserAction::ShowCSource,
        KeyCode::Char('C') => UserAction::ClearSession,
        KeyCode::Char('y') => UserAction::CopyDetailToClipboard,
        KeyCode::Char(ch) => UserAction::TextChar(ch),
        KeyCode::Up => UserAction::MoveUp,
        KeyCode::Down => UserAction::MoveDown,
        KeyCode::Left => UserAction::MoveLeft,
        KeyCode::Right => UserAction::MoveRight,
        KeyCode::Enter => UserAction::Confirm,
        KeyCode::Esc => UserAction::BackToBenchmarkList,
        KeyCode::Backspace => UserAction::Backspace,
        KeyCode::Tab => UserAction::RotateCodeViewMode,
        KeyCode::BackTab => UserAction::RotateCodeViewModePrev,
        _ => UserAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: crossterm::event::KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn maps_page_navigation_and_focus_keys() {
        assert_eq!(map_key_event(key(KeyCode::Enter)), UserAction::Confirm);
        assert_eq!(
            map_key_event(key(KeyCode::Esc)),
            UserAction::BackToBenchmarkList
        );
        assert_eq!(map_key_event(key(KeyCode::Left)), UserAction::MoveLeft);
        assert_eq!(map_key_event(key(KeyCode::Right)), UserAction::MoveRight);
        assert_eq!(
            map_key_event(key(KeyCode::Tab)),
            UserAction::RotateCodeViewMode
        );
        assert_eq!(
            map_key_event(key(KeyCode::BackTab)),
            UserAction::RotateCodeViewModePrev
        );

        assert_eq!(map_key_event(key(KeyCode::Char('a'))), UserAction::Analyze);
        assert_eq!(map_key_event(key(KeyCode::Char('r'))), UserAction::Run);
        assert_eq!(
            map_key_event(key(KeyCode::Char('c'))),
            UserAction::ShowCSource
        );
        assert_eq!(
            map_key_event(key(KeyCode::Char('C'))),
            UserAction::ClearSession
        );
        assert_eq!(
            map_key_event(key(KeyCode::Char('y'))),
            UserAction::CopyDetailToClipboard
        );
        assert_eq!(
            map_key_event(key(KeyCode::Char('o'))),
            UserAction::TextChar('o')
        );
        assert_eq!(
            map_key_event(key(KeyCode::Backspace)),
            UserAction::Backspace
        );
    }
}
