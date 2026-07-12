use crate::candidate_window::CandidateWindowView;

/// Native candidate window class name used by the future HWND renderer.
pub const WINDOW_CLASS_NAME: &str = "NovaTypeCandidateWindow";

/// Basic rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Logical colors used by the native renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintColor {
    Background,
    Border,
    Brand,
    Text,
    MutedText,
    HighlightBackground,
}

/// Render command emitted by the layout engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaintCommand {
    FillRect {
        rect: Rect,
        color: PaintColor,
    },
    StrokeRect {
        rect: Rect,
        color: PaintColor,
    },
    Text {
        rect: Rect,
        text: String,
        color: PaintColor,
    },
}

/// A backend that can consume logical paint commands.
pub trait PaintRenderer {
    fn fill_rect(&mut self, rect: Rect, color: PaintColor);
    fn stroke_rect(&mut self, rect: Rect, color: PaintColor);
    fn text(&mut self, rect: Rect, text: &str, color: PaintColor);
}

/// Executes paint commands against a renderer.
pub fn render_commands(renderer: &mut impl PaintRenderer, commands: &[PaintCommand]) {
    for command in commands {
        match command {
            PaintCommand::FillRect { rect, color } => renderer.fill_rect(*rect, *color),
            PaintCommand::StrokeRect { rect, color } => renderer.stroke_rect(*rect, *color),
            PaintCommand::Text { rect, text, color } => renderer.text(*rect, text, *color),
        }
    }
}

/// Layout constants for the simplified Sogou-style candidate window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateWindowMetrics {
    pub padding_x: i32,
    pub padding_y: i32,
    pub composition_height: i32,
    pub candidate_height: i32,
    pub candidate_gap: i32,
    pub min_width: i32,
    pub arrow_width: i32,
}

/// Opaque HWND-like handle wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateWindowHandle(pub isize);

/// State holder for the future native candidate HWND.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateWindowState {
    handle: Option<CandidateWindowHandle>,
    visible: bool,
    last_view: Option<CandidateWindowView>,
    last_bounds: Option<Rect>,
}

impl CandidateWindowState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            handle: None,
            visible: false,
            last_view: None,
            last_bounds: None,
        }
    }

    pub fn attach_handle(&mut self, handle: CandidateWindowHandle) {
        self.handle = Some(handle);
    }

    pub fn update(
        &mut self,
        view: CandidateWindowView,
        caret: (i32, i32),
        metrics: CandidateWindowMetrics,
    ) {
        self.last_bounds = Some(metrics.position_near_caret(caret, &view));
        self.last_view = Some(view);
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    #[must_use]
    pub fn last_bounds(&self) -> Option<Rect> {
        self.last_bounds
    }

    #[must_use]
    pub fn last_view(&self) -> Option<&CandidateWindowView> {
        self.last_view.as_ref()
    }
}

impl Default for CandidateWindowState {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for CandidateWindowMetrics {
    fn default() -> Self {
        Self {
            padding_x: 12,
            padding_y: 8,
            composition_height: 24,
            candidate_height: 32,
            candidate_gap: 12,
            min_width: 320,
            arrow_width: 42,
        }
    }
}

impl CandidateWindowMetrics {
    #[must_use]
    pub fn measure(&self, view: &CandidateWindowView) -> Rect {
        let composition_width = text_width(&view.composition_line, 9);
        let candidate_width: i32 = view
            .candidates
            .iter()
            .map(|candidate| text_width(&format!("{}. {}", candidate.number, candidate.text), 16))
            .sum::<i32>()
            + self.candidate_gap
                * i32::try_from(view.candidates.len().saturating_sub(1)).unwrap_or_default();
        let arrows = if view.show_prev || view.show_next {
            self.arrow_width
        } else {
            0
        };
        let width = (self.padding_x * 2)
            + composition_width
                .max(candidate_width + arrows)
                .max(self.min_width - self.padding_x * 2);
        let height = self.padding_y * 2 + self.composition_height + self.candidate_height;

        Rect {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    #[must_use]
    pub fn position_near_caret(&self, caret: (i32, i32), view: &CandidateWindowView) -> Rect {
        let mut rect = self.measure(view);
        rect.x = caret.0;
        rect.y = caret.1 + 4;
        rect
    }

    /// Builds deterministic paint commands for the future HWND renderer.
    #[must_use]
    pub fn paint_commands(&self, view: &CandidateWindowView) -> Vec<PaintCommand> {
        let bounds = self.measure(view);
        let mut commands = vec![
            PaintCommand::FillRect {
                rect: bounds,
                color: PaintColor::Background,
            },
            PaintCommand::StrokeRect {
                rect: bounds,
                color: PaintColor::Border,
            },
        ];

        let composition_rect = Rect {
            x: self.padding_x,
            y: self.padding_y,
            width: bounds.width - self.padding_x * 2,
            height: self.composition_height,
        };
        commands.push(PaintCommand::Text {
            rect: composition_rect,
            text: view.composition_line.clone(),
            color: PaintColor::Brand,
        });

        let mut x = self.padding_x;
        let y = self.padding_y + self.composition_height;
        for candidate in &view.candidates {
            let text = format!("{}. {}", candidate.number, candidate.text);
            let width = text_width(&text, 16) + 10;
            let rect = Rect {
                x,
                y,
                width,
                height: self.candidate_height,
            };
            if candidate.highlighted {
                commands.push(PaintCommand::FillRect {
                    rect,
                    color: PaintColor::HighlightBackground,
                });
            }
            commands.push(PaintCommand::Text {
                rect,
                text,
                color: if candidate.highlighted {
                    PaintColor::Brand
                } else {
                    PaintColor::Text
                },
            });
            x += width + self.candidate_gap;
        }

        if view.show_prev || view.show_next {
            let arrow_rect = Rect {
                x: bounds.width - self.padding_x - self.arrow_width,
                y,
                width: self.arrow_width,
                height: self.candidate_height,
            };
            let prev = if view.show_prev { "‹" } else { " " };
            let next = if view.show_next { "›" } else { " " };
            commands.push(PaintCommand::Text {
                rect: arrow_rect,
                text: format!("{prev} {next}"),
                color: PaintColor::MutedText,
            });
        }

        commands
    }
}

fn text_width(text: &str, ascii_width: i32) -> i32 {
    text.chars()
        .map(|ch| {
            if ch.is_ascii() {
                ascii_width
            } else {
                ascii_width * 2
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{
        CandidateWindowHandle, CandidateWindowMetrics, CandidateWindowState, PaintColor,
        PaintCommand, PaintRenderer, Rect, WINDOW_CLASS_NAME, render_commands,
    };
    use crate::candidate_window::CandidateWindowView;
    use crate::edit_session::CandidateWindowModel;
    use novatype_protocol::CandidateDto;

    fn view() -> CandidateWindowView {
        CandidateWindowView::from_model(&CandidateWindowModel {
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
        })
    }

    #[test]
    fn has_stable_window_class_name() {
        assert_eq!(WINDOW_CLASS_NAME, "NovaTypeCandidateWindow");
    }

    #[test]
    fn measures_minimum_window_size() {
        let rect = CandidateWindowMetrics::default().measure(&view());

        assert!(rect.width >= 320);
        assert_eq!(rect.height, 72);
    }

    #[test]
    fn positions_below_caret() {
        let rect = CandidateWindowMetrics::default().position_near_caret((100, 200), &view());

        assert_eq!(rect.x, 100);
        assert_eq!(rect.y, 204);
    }

    #[test]
    fn emits_paint_commands_for_native_renderer() {
        let commands = CandidateWindowMetrics::default().paint_commands(&view());

        assert!(matches!(
            commands[0],
            PaintCommand::FillRect {
                color: PaintColor::Background,
                ..
            }
        ));
        assert!(commands.iter().any(|command| matches!(
            command,
            PaintCommand::Text { text, .. } if text == "1. 中国"
        )));
        assert!(commands.iter().any(|command| matches!(
            command,
            PaintCommand::FillRect {
                color: PaintColor::HighlightBackground,
                ..
            }
        )));
    }

    #[test]
    fn candidate_window_state_tracks_visibility_and_bounds() {
        let mut state = CandidateWindowState::new();
        state.attach_handle(CandidateWindowHandle(100));
        state.update(view(), (10, 20), CandidateWindowMetrics::default());

        assert!(state.is_visible());
        assert_eq!(state.last_bounds().map(|rect| rect.x), Some(10));
        assert!(state.last_view().is_some());

        state.hide();
        assert!(!state.is_visible());
    }

    #[derive(Default)]
    struct RecordingRenderer {
        calls: Vec<String>,
    }

    impl PaintRenderer for RecordingRenderer {
        fn fill_rect(&mut self, _rect: Rect, _color: PaintColor) {
            self.calls.push("fill".to_string());
        }

        fn stroke_rect(&mut self, _rect: Rect, _color: PaintColor) {
            self.calls.push("stroke".to_string());
        }

        fn text(&mut self, _rect: Rect, text: &str, _color: PaintColor) {
            self.calls.push(format!("text:{text}"));
        }
    }

    #[test]
    fn render_commands_calls_renderer_in_order() {
        let commands = CandidateWindowMetrics::default().paint_commands(&view());
        let mut renderer = RecordingRenderer::default();

        render_commands(&mut renderer, &commands);

        assert_eq!(renderer.calls[0], "fill");
        assert_eq!(renderer.calls[1], "stroke");
        assert!(renderer.calls.iter().any(|call| call == "text:1. 中国"));
    }
}
