use crate::edit_session::{EditOperation, plan_operations};
use crate::{InputSession, keymap};

/// Result of `OnTestKeyDown`-style handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyTestResult {
    Eat,
    Pass,
}

/// Determines whether `NovaType` should handle a virtual key.
///
/// This mirrors the decision point in TSF `ITfKeyEventSink::OnTestKeyDown`.
#[must_use]
pub fn test_key_down(
    is_active: bool,
    sink_active: bool,
    is_composing: bool,
    vk: u32,
) -> KeyTestResult {
    let Some(key) = keymap::map_vk(vk) else {
        return KeyTestResult::Pass;
    };

    if !is_active || !sink_active {
        return KeyTestResult::Pass;
    }

    match key {
        crate::Key::Char(_) => KeyTestResult::Eat,
        crate::Key::Space
        | crate::Key::Enter
        | crate::Key::Backspace
        | crate::Key::Escape
        | crate::Key::PageNext
        | crate::Key::PagePrev
        | crate::Key::Digit(_) => {
            if is_composing {
                KeyTestResult::Eat
            } else {
                KeyTestResult::Pass
            }
        }
    }
}

/// Processes a key that has been accepted by `test_key_down`.
#[must_use]
pub fn key_down<C: crate::EngineClient>(
    session: &mut InputSession<C>,
    vk: u32,
) -> Vec<EditOperation> {
    let Some(key) = keymap::map_vk(vk) else {
        return plan_operations(crate::SessionAction::PassThrough);
    };

    plan_operations(session.handle_key_action(key))
}

#[cfg(test)]
mod tests {
    use super::{KeyTestResult, key_down, test_key_down};
    use crate::{EngineClient, InputSession, edit_session::EditOperation};
    use novatype_protocol::CandidateDto;

    struct MockClient;

    impl EngineClient for MockClient {
        fn suggest(&mut self, input: &str, _limit: usize) -> Vec<CandidateDto> {
            vec![CandidateDto {
                text: input.to_string(),
                reading: vec![input.to_string()],
                score: 1.0,
            }]
        }

        fn commit(&mut self, _text: &str, _reading: &[String]) -> Vec<String> {
            Vec::new()
        }
    }

    #[test]
    fn test_key_down_eats_letters_when_active() {
        assert_eq!(test_key_down(true, true, false, 0x4E), KeyTestResult::Eat);
        assert_eq!(test_key_down(false, true, false, 0x4E), KeyTestResult::Pass);
        assert_eq!(test_key_down(true, false, false, 0x4E), KeyTestResult::Pass);
    }

    #[test]
    fn test_key_down_eats_controls_only_while_composing() {
        assert_eq!(test_key_down(true, true, false, 0x20), KeyTestResult::Pass);
        assert_eq!(test_key_down(true, true, true, 0x20), KeyTestResult::Eat);
    }

    #[test]
    fn key_down_updates_and_commits() {
        let mut session = InputSession::new(MockClient);
        let operations = key_down(&mut session, 0x4E); // N
        assert!(matches!(operations[0], EditOperation::SetComposition(_)));

        let operations = key_down(&mut session, 0x20); // Space
        assert!(matches!(operations[0], EditOperation::CommitText(_)));
    }
}
