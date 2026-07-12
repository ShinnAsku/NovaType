use crate::edit_session::{CandidateWindowModel, DocumentEditor};
use crate::{
    candidate_window::CandidateWindowView,
    window::{CandidateWindowMetrics, CandidateWindowState},
};
use core::ffi::c_void;

/// Opaque handles captured from TSF callbacks and edit sessions.
///
/// These are raw pointers/cookies by design; the next step replaces the opaque
/// fields with typed `windows-rs` interfaces (`ITfContext`, `ITfRange`, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TsfEditContext {
    pub thread_mgr: *mut c_void,
    pub client_id: u32,
    pub edit_cookie: u32,
}

/// Testable adapter boundary for the future real `ITfEditSession` code.
#[derive(Debug, Default)]
pub struct TsfDocumentEditor {
    context: Option<TsfEditContext>,
    composition: String,
    committed: String,
    candidate_model: Option<CandidateWindowModel>,
    candidate_window: CandidateWindowState,
    #[cfg(windows)]
    native_window: Option<crate::native_window::NativeCandidateWindow>,
    caret: (i32, i32),
    pass_through_count: usize,
}

impl TsfDocumentEditor {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn attach_context(&mut self, context: TsfEditContext) {
        self.context = Some(context);
    }

    pub fn detach_context(&mut self) {
        self.context = None;
        self.clear_composition();
        self.hide_candidates();
    }

    #[must_use]
    pub fn context(&self) -> Option<TsfEditContext> {
        self.context
    }

    #[must_use]
    pub fn composition(&self) -> &str {
        &self.composition
    }

    #[must_use]
    pub fn committed(&self) -> &str {
        &self.committed
    }

    #[must_use]
    pub fn candidate_model(&self) -> Option<&CandidateWindowModel> {
        self.candidate_model.as_ref()
    }

    #[must_use]
    pub fn candidate_window(&self) -> &CandidateWindowState {
        &self.candidate_window
    }

    #[cfg(windows)]
    pub fn ensure_native_window(&mut self) {
        if self.native_window.is_none() {
            self.native_window = crate::native_window::NativeCandidateWindow::create().ok();
            if let Some(window) = &self.native_window {
                self.candidate_window.attach_handle(window.handle());
            }
        }
    }

    #[must_use]
    pub fn pass_through_count(&self) -> usize {
        self.pass_through_count
    }
}

impl DocumentEditor for TsfDocumentEditor {
    fn pass_through(&mut self) {
        self.pass_through_count += 1;
    }

    fn set_composition(&mut self, text: &str) {
        self.composition = text.to_string();
    }

    fn commit_text(&mut self, text: &str) {
        self.committed.push_str(text);
    }

    fn show_candidates(&mut self, model: &CandidateWindowModel) {
        self.candidate_model = Some(model.clone());
        self.candidate_window.update(
            CandidateWindowView::from_model(model),
            self.caret,
            CandidateWindowMetrics::default(),
        );
        #[cfg(windows)]
        {
            self.ensure_native_window();
            if let Some(window) = &mut self.native_window {
                let _ = window.update_view(
                    &CandidateWindowView::from_model(model),
                    self.caret,
                    CandidateWindowMetrics::default(),
                );
            }
        }
    }

    fn set_caret(&mut self, x: i32, y: i32) {
        self.caret = (x, y);
    }

    fn hide_candidates(&mut self) {
        self.candidate_model = None;
        self.candidate_window.hide();
        #[cfg(windows)]
        if let Some(window) = &self.native_window {
            window.hide();
        }
    }

    fn clear_composition(&mut self) {
        self.composition.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{TsfDocumentEditor, TsfEditContext};
    use crate::edit_session::{CandidateWindowModel, DocumentEditor};
    use novatype_protocol::CandidateDto;

    fn model() -> CandidateWindowModel {
        CandidateWindowModel {
            composition: "ni".to_string(),
            candidates: vec![CandidateDto {
                text: "你".to_string(),
                reading: vec!["ni".to_string()],
                score: 1.0,
            }],
            page: 0,
            has_prev_page: false,
            has_next_page: false,
        }
    }

    #[test]
    fn stores_context_and_edit_state() {
        let mut editor = TsfDocumentEditor::new();
        let context = TsfEditContext {
            thread_mgr: core::ptr::dangling_mut(),
            client_id: 7,
            edit_cookie: 11,
        };

        editor.attach_context(context);
        editor.set_caret(100, 200);
        editor.set_composition("ni");
        editor.show_candidates(&model());
        editor.commit_text("你");

        assert_eq!(editor.context(), Some(context));
        assert_eq!(editor.composition(), "ni");
        assert_eq!(editor.committed(), "你");
        assert!(editor.candidate_model().is_some());
        assert!(editor.candidate_window().is_visible());
        assert_eq!(
            editor.candidate_window().last_bounds().map(|rect| rect.x),
            Some(100)
        );
    }

    #[test]
    fn detach_clears_ui_state() {
        let mut editor = TsfDocumentEditor::new();
        editor.attach_context(TsfEditContext {
            thread_mgr: core::ptr::dangling_mut(),
            client_id: 1,
            edit_cookie: 2,
        });
        editor.set_composition("ni");
        editor.show_candidates(&model());

        editor.detach_context();

        assert!(editor.context().is_none());
        assert_eq!(editor.composition(), "");
        assert!(editor.candidate_model().is_none());
    }
}
