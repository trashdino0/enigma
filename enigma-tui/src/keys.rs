//! Key-event filtering: on Windows, crossterm reports key *releases* as
//! `KeyEventKind::Release` events. Acting on them double-types every letter
//! (and double-steps Backspace, or toggles Enter twice into a no-op).
//! Every screen ignores releases and acts on press/repeat only.

use crossterm::event::{KeyEvent, KeyEventKind};

/// Whether this key event should drive UI actions.
#[inline]
pub fn is_press(key: &KeyEvent) -> bool {
    !matches!(key.kind, KeyEventKind::Release)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn event(kind: KeyEventKind) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::empty(),
            kind,
            state: crossterm::event::KeyEventState::empty(),
        }
    }

    #[test]
    fn releases_are_ignored_press_and_repeat_handled() {
        assert!(is_press(&event(KeyEventKind::Press)));
        assert!(is_press(&event(KeyEventKind::Repeat)));
        assert!(!is_press(&event(KeyEventKind::Release)));
    }
}
