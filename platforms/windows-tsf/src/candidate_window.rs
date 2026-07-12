use crate::edit_session::CandidateWindowModel;

/// One candidate item ready for native-window rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateItemView {
    pub number: usize,
    pub text: String,
    pub highlighted: bool,
}

/// Presentation model for the simplified Sogou-style candidate window.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateWindowView {
    pub composition_line: String,
    pub candidates: Vec<CandidateItemView>,
    pub show_prev: bool,
    pub show_next: bool,
}

impl CandidateWindowView {
    /// Builds a render-ready view from the edit-session model.
    #[must_use]
    pub fn from_model(model: &CandidateWindowModel) -> Self {
        Self {
            composition_line: model.composition.clone(),
            candidates: model
                .candidates
                .iter()
                .enumerate()
                .map(|(index, candidate)| CandidateItemView {
                    number: index + 1,
                    text: candidate.text.clone(),
                    highlighted: index == 0 && model.page == 0,
                })
                .collect(),
            show_prev: model.has_prev_page,
            show_next: model.has_next_page,
        }
    }

    /// Produces a compact debug string matching the intended native layout.
    #[must_use]
    pub fn debug_line(&self) -> String {
        let candidates = self
            .candidates
            .iter()
            .map(|candidate| format!("{}. {}", candidate.number, candidate.text))
            .collect::<Vec<_>>()
            .join("  ");
        let prev = if self.show_prev { "‹" } else { " " };
        let next = if self.show_next { "›" } else { " " };
        format!("{}\n{}  {prev} {next}", self.composition_line, candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::CandidateWindowView;
    use crate::edit_session::CandidateWindowModel;
    use novatype_protocol::CandidateDto;

    fn model() -> CandidateWindowModel {
        CandidateWindowModel {
            composition: "zhong'guo".to_string(),
            candidates: vec![
                CandidateDto {
                    text: "中国".to_string(),
                    reading: vec!["zhong".into(), "guo".into()],
                    score: 1.0,
                },
                CandidateDto {
                    text: "中".to_string(),
                    reading: vec!["zhong".into()],
                    score: 0.5,
                },
            ],
            page: 0,
            has_prev_page: false,
            has_next_page: true,
        }
    }

    #[test]
    fn builds_view_from_model() {
        let view = CandidateWindowView::from_model(&model());

        assert_eq!(view.composition_line, "zhong'guo");
        assert_eq!(view.candidates[0].number, 1);
        assert_eq!(view.candidates[0].text, "中国");
        assert!(view.candidates[0].highlighted);
        assert!(!view.show_prev);
        assert!(view.show_next);
    }

    #[test]
    fn debug_line_matches_compact_layout() {
        let view = CandidateWindowView::from_model(&model());
        let line = view.debug_line();

        assert!(line.contains("zhong'guo"));
        assert!(line.contains("1. 中国"));
        assert!(line.contains('›'));
    }
}
