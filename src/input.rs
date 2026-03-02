use crossterm::event::{KeyCode, KeyEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserAction {
    None,
    Quit,
    MoveUp,
    MoveDown,
    OpenBenchmarkPage,
    BackToBenchmarkList,
    PrevOptimizationStep,
    NextOptimizationStep,
    CycleProfile,
    Build,
    Run,
    BuildAndRun,
    SwitchTab,
    ClearSession,
}

pub fn map_key_event(key: KeyEvent) -> UserAction {
    match key.code {
        KeyCode::Char('q') => UserAction::Quit,
        KeyCode::Up => UserAction::MoveUp,
        KeyCode::Down => UserAction::MoveDown,
        KeyCode::Enter => UserAction::OpenBenchmarkPage,
        KeyCode::Esc => UserAction::BackToBenchmarkList,
        KeyCode::Left => UserAction::PrevOptimizationStep,
        KeyCode::Right => UserAction::NextOptimizationStep,
        KeyCode::Char('p') => UserAction::CycleProfile,
        KeyCode::Char('b') => UserAction::Build,
        KeyCode::Char('r') => UserAction::Run,
        KeyCode::Char('a') => UserAction::BuildAndRun,
        KeyCode::Tab => UserAction::SwitchTab,
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
    fn maps_page_and_step_navigation_keys() {
        assert_eq!(
            map_key_event(key(KeyCode::Enter)),
            UserAction::OpenBenchmarkPage
        );
        assert_eq!(
            map_key_event(key(KeyCode::Esc)),
            UserAction::BackToBenchmarkList
        );
        assert_eq!(
            map_key_event(key(KeyCode::Left)),
            UserAction::PrevOptimizationStep
        );
        assert_eq!(
            map_key_event(key(KeyCode::Right)),
            UserAction::NextOptimizationStep
        );
    }
}
