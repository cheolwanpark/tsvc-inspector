use crossterm::event::{KeyCode, KeyEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserAction {
    None,
    Quit,
    MoveUp,
    MoveDown,
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
        KeyCode::Char('p') => UserAction::CycleProfile,
        KeyCode::Char('b') => UserAction::Build,
        KeyCode::Char('r') => UserAction::Run,
        KeyCode::Char('a') => UserAction::BuildAndRun,
        KeyCode::Tab => UserAction::SwitchTab,
        KeyCode::Char('c') => UserAction::ClearSession,
        _ => UserAction::None,
    }
}
