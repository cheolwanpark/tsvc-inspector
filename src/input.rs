use crossterm::event::{KeyCode, KeyEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserAction {
    None,
    Quit,
    MoveUp,
    MoveDown,
    Confirm,             // Enter
    BackToBenchmarkList, // Esc
    FocusPrevPane,       // Left
    FocusNextPane,       // Right
    CycleProfile,        // 'p'
    Run,                 // 'r' → BuildAndRun
    Analyze,             // 'a' → AnalyzeFast
    ClearSession,        // 'c'
}

pub fn map_key_event(key: KeyEvent) -> UserAction {
    match key.code {
        KeyCode::Char('q') => UserAction::Quit,
        KeyCode::Up => UserAction::MoveUp,
        KeyCode::Down => UserAction::MoveDown,
        KeyCode::Enter => UserAction::Confirm,
        KeyCode::Esc => UserAction::BackToBenchmarkList,
        KeyCode::Left => UserAction::FocusPrevPane,
        KeyCode::Right => UserAction::FocusNextPane,
        KeyCode::Char('p') => UserAction::CycleProfile,
        KeyCode::Char('r') => UserAction::Run,
        KeyCode::Char('a') => UserAction::Analyze,
        KeyCode::Char('c') => UserAction::ClearSession,
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
        assert_eq!(
            map_key_event(key(KeyCode::Left)),
            UserAction::FocusPrevPane
        );
        assert_eq!(
            map_key_event(key(KeyCode::Right)),
            UserAction::FocusNextPane
        );
        // Removed keys return None
        assert_eq!(
            map_key_event(key(KeyCode::Char('o'))),
            UserAction::None
        );
        assert_eq!(
            map_key_event(key(KeyCode::Char('b'))),
            UserAction::None
        );
        assert_eq!(
            map_key_event(key(KeyCode::Char('X'))),
            UserAction::None
        );
        // New bindings
        assert_eq!(
            map_key_event(key(KeyCode::Char('a'))),
            UserAction::Analyze
        );
        assert_eq!(map_key_event(key(KeyCode::Char('r'))), UserAction::Run);
    }
}
