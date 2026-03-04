use crossterm::event::{KeyCode, KeyEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserAction {
    None,
    Quit,
    MoveUp,
    MoveDown,
    Confirm,               // Enter
    BackToBenchmarkList,   // Esc
    FocusNextPaneCycle,    // Tab
    FocusPrevPaneCycle,    // Shift-Tab
    CycleProfile,          // 'p'
    Run,                   // 'r' → BuildAndRun
    Analyze,               // 'a' → AnalyzeFast
    ClearSession,          // 'c'
    CopyDetailToClipboard, // 'y'
}

pub fn map_key_event(key: KeyEvent) -> UserAction {
    match key.code {
        KeyCode::Char('q') => UserAction::Quit,
        KeyCode::Up => UserAction::MoveUp,
        KeyCode::Down => UserAction::MoveDown,
        KeyCode::Enter => UserAction::Confirm,
        KeyCode::Esc => UserAction::BackToBenchmarkList,
        KeyCode::Tab => UserAction::FocusNextPaneCycle,
        KeyCode::BackTab => UserAction::FocusPrevPaneCycle,
        KeyCode::Char('p') => UserAction::CycleProfile,
        KeyCode::Char('r') => UserAction::Run,
        KeyCode::Char('a') => UserAction::Analyze,
        KeyCode::Char('c') => UserAction::ClearSession,
        KeyCode::Char('y') => UserAction::CopyDetailToClipboard,
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
        assert_eq!(map_key_event(key(KeyCode::Left)), UserAction::None);
        assert_eq!(map_key_event(key(KeyCode::Right)), UserAction::None);
        assert_eq!(
            map_key_event(key(KeyCode::Tab)),
            UserAction::FocusNextPaneCycle
        );
        assert_eq!(
            map_key_event(key(KeyCode::BackTab)),
            UserAction::FocusPrevPaneCycle
        );
        // Removed keys return None
        assert_eq!(map_key_event(key(KeyCode::Char('o'))), UserAction::None);
        assert_eq!(map_key_event(key(KeyCode::Char('b'))), UserAction::None);
        assert_eq!(map_key_event(key(KeyCode::Char('X'))), UserAction::None);
        // New bindings
        assert_eq!(map_key_event(key(KeyCode::Char('a'))), UserAction::Analyze);
        assert_eq!(map_key_event(key(KeyCode::Char('r'))), UserAction::Run);
        assert_eq!(
            map_key_event(key(KeyCode::Char('y'))),
            UserAction::CopyDetailToClipboard
        );
    }
}
