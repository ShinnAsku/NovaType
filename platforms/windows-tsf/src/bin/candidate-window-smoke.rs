#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    use novatype_protocol::CandidateDto;
    use novatype_tsf::candidate_window::CandidateWindowView;
    use novatype_tsf::edit_session::CandidateWindowModel;
    use novatype_tsf::native_window::NativeCandidateWindow;
    use novatype_tsf::window::CandidateWindowMetrics;

    let view = CandidateWindowView::from_model(&CandidateWindowModel {
        composition: "zhong'guo'ren".to_string(),
        candidates: vec![
            CandidateDto {
                text: "中国人".to_string(),
                reading: vec!["zhong".into(), "guo".into(), "ren".into()],
                score: 1.0,
            },
            CandidateDto {
                text: "中国".to_string(),
                reading: vec!["zhong".into(), "guo".into()],
                score: 0.8,
            },
            CandidateDto {
                text: "中".to_string(),
                reading: vec!["zhong".into()],
                score: 0.6,
            },
        ],
        page: 0,
        has_prev_page: false,
        has_next_page: true,
    });

    let mut window = NativeCandidateWindow::create()?;
    window.update_view(&view, (320, 320), CandidateWindowMetrics::default())?;
    std::thread::sleep(std::time::Duration::from_secs(1));
    window.hide();
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("candidate-window-smoke is Windows-only");
}
