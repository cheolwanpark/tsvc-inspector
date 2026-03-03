use crossterm::event::{KeyCode, KeyEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserAction {
    None,
    Quit,
    MoveUp,
    MoveDown,
    OpenBenchmarkPage,
    BackToBenchmarkList,
    FocusPrevTab,
    FocusNextTab,
    CycleProfile,
    Build,
    Run,
    BuildAndRun,
    AnalyzeFast,
    AnalyzeDeep,
    ToggleOverlay,
    ClearSession,
}

pub fn map_key_event(key: KeyEvent) -> UserAction {
    match key.code {
        KeyCode::Char('q') => UserAction::Quit,
        KeyCode::Up => UserAction::MoveUp,
        KeyCode::Down => UserAction::MoveDown,
        KeyCode::Enter => UserAction::OpenBenchmarkPage,
        KeyCode::Esc => UserAction::BackToBenchmarkList,
        KeyCode::Left => UserAction::FocusPrevTab,
        KeyCode::Right => UserAction::FocusNextTab,
        KeyCode::Char('p') => UserAction::CycleProfile,
        KeyCode::Char('b') => UserAction::Build,
        KeyCode::Char('r') => UserAction::Run,
        KeyCode::Char('a') => UserAction::BuildAndRun,
        KeyCode::Char('x') => UserAction::AnalyzeFast,
        KeyCode::Char('X') => UserAction::AnalyzeDeep,
        KeyCode::Char('o') => UserAction::ToggleOverlay,
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
        assert_eq!(
            map_key_event(key(KeyCode::Enter)),
            UserAction::OpenBenchmarkPage
        );
        assert_eq!(
            map_key_event(key(KeyCode::Esc)),
            UserAction::BackToBenchmarkList
        );
        assert_eq!(map_key_event(key(KeyCode::Left)), UserAction::FocusPrevTab);
        assert_eq!(map_key_event(key(KeyCode::Right)), UserAction::FocusNextTab);
        assert_eq!(
            map_key_event(key(KeyCode::Char('o'))),
            UserAction::ToggleOverlay
        );
        assert_eq!(
            map_key_event(key(KeyCode::Char('x'))),
            UserAction::AnalyzeFast
        );
        assert_eq!(
            map_key_event(key(KeyCode::Char('X'))),
            UserAction::AnalyzeDeep
        );
    }
}
