use crate::{SessionAction, SessionSnapshot};
use novatype_protocol::CandidateDto;

/// Planned edit/candidate-window operations produced from a session action.
///
/// The future TSF `ITfEditSession` implementation will translate these into
/// real document mutations and candidate-window HWND updates.
#[derive(Debug, Clone, PartialEq)]
pub enum EditOperation {
    /// Let the host application handle the key.
    PassThrough,
    /// Replace the active composition/preedit text.
    SetComposition(String),
    /// Commit text into the host application.
    CommitText(String),
    /// Show/update the candidate window.
    ShowCandidates(CandidateWindowModel),
    /// Hide candidate window.
    HideCandidates,
    /// Clear composition/preedit text.
    ClearComposition,
}

/// Minimal document/candidate-window operations required by the TSF shell.
///
/// The future real implementation will wrap `ITfEditSession` and native HWND
/// rendering. Tests use a fake document implementation.
pub trait DocumentEditor {
    fn pass_through(&mut self);
    fn set_composition(&mut self, text: &str);
    fn commit_text(&mut self, text: &str);
    fn show_candidates(&mut self, model: &CandidateWindowModel);
    fn set_caret(&mut self, x: i32, y: i32);
    fn hide_candidates(&mut self);
    fn clear_composition(&mut self);
}

/// Candidate-window model consumed by the future HWND renderer.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateWindowModel {
    pub composition: String,
    pub candidates: Vec<CandidateDto>,
    pub page: usize,
    pub has_prev_page: bool,
    pub has_next_page: bool,
}

impl From<SessionSnapshot> for CandidateWindowModel {
    fn from(snapshot: SessionSnapshot) -> Self {
        Self {
            composition: snapshot.composition,
            candidates: snapshot.candidates,
            page: snapshot.page,
            has_prev_page: snapshot.has_prev_page,
            has_next_page: snapshot.has_next_page,
        }
    }
}

/// Converts a session action into deterministic edit/candidate operations.
#[must_use]
pub fn plan_operations(action: SessionAction) -> Vec<EditOperation> {
    match action {
        SessionAction::PassThrough => vec![EditOperation::PassThrough],
        SessionAction::Update(snapshot) => vec![
            EditOperation::SetComposition(snapshot.composition.clone()),
            EditOperation::ShowCandidates(snapshot.into()),
        ],
        SessionAction::Commit(text) => vec![
            EditOperation::CommitText(text),
            EditOperation::ClearComposition,
            EditOperation::HideCandidates,
        ],
        SessionAction::Dismiss => vec![
            EditOperation::ClearComposition,
            EditOperation::HideCandidates,
        ],
    }
}

/// Executes planned operations against a document/candidate-window adapter.
pub fn execute_operations(editor: &mut impl DocumentEditor, operations: &[EditOperation]) {
    for operation in operations {
        match operation {
            EditOperation::PassThrough => editor.pass_through(),
            EditOperation::SetComposition(text) => editor.set_composition(text),
            EditOperation::CommitText(text) => editor.commit_text(text),
            EditOperation::ShowCandidates(model) => editor.show_candidates(model),
            EditOperation::HideCandidates => editor.hide_candidates(),
            EditOperation::ClearComposition => editor.clear_composition(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CandidateWindowModel, DocumentEditor, EditOperation, execute_operations, plan_operations,
    };
    use crate::{SessionAction, SessionSnapshot};
    use novatype_protocol::CandidateDto;

    fn snapshot() -> SessionSnapshot {
        SessionSnapshot {
            composition: "ni".to_string(),
            candidates: vec![CandidateDto {
                text: "你".to_string(),
                reading: vec!["ni".to_string()],
                score: 1.0,
            }],
            page: 0,
            has_prev_page: false,
            has_next_page: true,
        }
    }

    #[test]
    fn update_sets_composition_and_candidates() {
        let operations = plan_operations(SessionAction::Update(snapshot()));

        assert_eq!(
            operations[0],
            EditOperation::SetComposition("ni".to_string())
        );
        assert_eq!(
            operations[1],
            EditOperation::ShowCandidates(CandidateWindowModel {
                composition: "ni".to_string(),
                candidates: vec![CandidateDto {
                    text: "你".to_string(),
                    reading: vec!["ni".to_string()],
                    score: 1.0,
                }],
                page: 0,
                has_prev_page: false,
                has_next_page: true,
            })
        );

        let EditOperation::ShowCandidates(model) = &operations[1] else {
            panic!("expected candidate model");
        };
        let view = crate::candidate_window::CandidateWindowView::from_model(model);
        assert_eq!(view.composition_line, "ni");
        assert_eq!(view.candidates[0].text, "你");
    }

    #[test]
    fn commit_writes_text_and_clears_ui() {
        let operations = plan_operations(SessionAction::Commit("你好".to_string()));

        assert_eq!(
            operations,
            vec![
                EditOperation::CommitText("你好".to_string()),
                EditOperation::ClearComposition,
                EditOperation::HideCandidates,
            ]
        );
    }

    #[test]
    fn dismiss_clears_ui_without_commit() {
        let operations = plan_operations(SessionAction::Dismiss);

        assert_eq!(
            operations,
            vec![
                EditOperation::ClearComposition,
                EditOperation::HideCandidates
            ]
        );
    }

    #[test]
    fn pass_through_stays_single_operation() {
        assert_eq!(
            plan_operations(SessionAction::PassThrough),
            vec![EditOperation::PassThrough]
        );
    }

    #[derive(Default)]
    struct FakeEditor {
        composition: String,
        committed: String,
        candidates_visible: bool,
        pass_through_count: usize,
    }

    impl DocumentEditor for FakeEditor {
        fn pass_through(&mut self) {
            self.pass_through_count += 1;
        }

        fn set_composition(&mut self, text: &str) {
            self.composition = text.to_string();
        }

        fn commit_text(&mut self, text: &str) {
            self.committed.push_str(text);
        }

        fn show_candidates(&mut self, _model: &CandidateWindowModel) {
            self.candidates_visible = true;
        }

        fn set_caret(&mut self, _x: i32, _y: i32) {}

        fn hide_candidates(&mut self) {
            self.candidates_visible = false;
        }

        fn clear_composition(&mut self) {
            self.composition.clear();
        }
    }

    #[test]
    fn execute_update_operations_sets_fake_document_state() {
        let operations = plan_operations(SessionAction::Update(snapshot()));
        let mut editor = FakeEditor::default();

        execute_operations(&mut editor, &operations);

        assert_eq!(editor.composition, "ni");
        assert!(editor.candidates_visible);
    }

    #[test]
    fn execute_commit_operations_commits_and_clears_ui() {
        let operations = plan_operations(SessionAction::Commit("你好".to_string()));
        let mut editor = FakeEditor {
            composition: "nihao".to_string(),
            candidates_visible: true,
            ..FakeEditor::default()
        };

        execute_operations(&mut editor, &operations);

        assert_eq!(editor.committed, "你好");
        assert!(editor.composition.is_empty());
        assert!(!editor.candidates_visible);
    }

    #[test]
    fn execute_pass_through_counts_operation() {
        let operations = plan_operations(SessionAction::PassThrough);
        let mut editor = FakeEditor::default();

        execute_operations(&mut editor, &operations);

        assert_eq!(editor.pass_through_count, 1);
    }
}
