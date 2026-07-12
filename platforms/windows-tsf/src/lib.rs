//! Platform-independent input session core for the Windows TSF shell.
//!
//! This crate owns everything the TSF COM layer can delegate: composition
//! buffer management, candidate paging, key handling, and daemon access.
//! The COM glue (text service registration, edit sessions, candidate window)
//! stays in the future `dll` module and calls into [`InputSession`].
//!
//! Design references: azooKey-Windows (Rust TSF), weasel (C++ TSF), and
//! rakukan (out-of-process engine host).

pub mod candidate_window;
pub(crate) mod debug_log;
pub mod edit_session;
pub mod key_event;
pub mod keymap;
pub mod metadata;
pub mod profile;
pub mod sink;
pub mod tsf_document;
pub mod window;

#[cfg(windows)]
mod com;
#[cfg(windows)]
pub mod native_window;
#[cfg(windows)]
mod registration;

use novatype_protocol::{CandidateDto, Request, Response, resolve_endpoint, send_request};

#[cfg(windows)]
const S_OK: i32 = 0;
#[cfg(windows)]
const S_FALSE: i32 = 1;
#[cfg(windows)]
const SELFREG_E_CLASS: i32 = i32::from_ne_bytes(0x8004_0201_u32.to_ne_bytes());

/// Standard COM export used by `regsvr32` to register this TSF DLL.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "system" fn DllRegisterServer() -> i32 {
    registration::register_server().map_or(SELFREG_E_CLASS, |()| S_OK)
}

/// Standard COM export used by `regsvr32 /u` to unregister this TSF DLL.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "system" fn DllUnregisterServer() -> i32 {
    registration::unregister_server().map_or(SELFREG_E_CLASS, |()| S_OK)
}

/// Standard COM export queried by COM before unloading the DLL.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> i32 {
    if com::can_unload() { S_OK } else { S_FALSE }
}

/// Standard COM export for class factory retrieval.
///
/// The factory returns a minimal `ITfTextInputProcessor` object. Activation
/// currently records success only; key sinks/edit sessions land next.
#[cfg(windows)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "system" fn DllGetClassObject(
    class_id: *const com::Guid,
    interface_id: *const com::Guid,
    object: *mut *mut core::ffi::c_void,
) -> i32 {
    // SAFETY: COM owns the raw pointers and expects HRESULT error returns for
    // invalid pointers. `get_class_object` validates nulls before dereference.
    unsafe { com::get_class_object(class_id, interface_id, object) }
}

pub const PAGE_SIZE: usize = 5;
const CANDIDATE_FETCH_LIMIT: usize = 20;

/// Abstract engine access so the session core is testable without a daemon.
pub trait EngineClient {
    /// Fetches candidates for the composition buffer.
    fn suggest(&mut self, input: &str, limit: usize) -> Vec<CandidateDto>;
    /// Reports a committed candidate for learning; returns predictions.
    fn commit(&mut self, text: &str, reading: &[String]) -> Vec<String>;
}

/// Engine client backed by the `novatyped` daemon.
pub struct DaemonClient {
    endpoint: String,
}

impl DaemonClient {
    #[must_use]
    pub fn new() -> Self {
        Self {
            endpoint: resolve_endpoint(),
        }
    }
}

impl Default for DaemonClient {
    fn default() -> Self {
        Self::new()
    }
}

impl EngineClient for DaemonClient {
    fn suggest(&mut self, input: &str, limit: usize) -> Vec<CandidateDto> {
        match send_request(
            &self.endpoint,
            &Request::Suggest {
                input: input.to_string(),
                limit,
            },
        ) {
            Ok(Response::Candidates(candidates)) => candidates,
            _ => Vec::new(),
        }
    }

    fn commit(&mut self, text: &str, reading: &[String]) -> Vec<String> {
        match send_request(
            &self.endpoint,
            &Request::Commit {
                text: text.to_string(),
                reading: reading.to_vec(),
            },
        ) {
            Ok(Response::Predictions(predictions)) => predictions,
            _ => Vec::new(),
        }
    }
}

/// Keys the TSF layer forwards to the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A lowercase ASCII letter.
    Char(char),
    /// Digit 1-9 selecting a candidate on the current page.
    Digit(u8),
    Space,
    Enter,
    Backspace,
    Escape,
    PageNext,
    PagePrev,
}

/// What the TSF layer must do after a key is processed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Key is not ours; let the application handle it.
    PassThrough,
    /// Composition/candidates changed; repaint the candidate window.
    Updated,
    /// Commit this text to the application and clear composition.
    Commit(String),
    /// Composition dismissed; hide the candidate window.
    Dismissed,
}

/// Candidate-window state after processing a key.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSnapshot {
    pub composition: String,
    pub candidates: Vec<CandidateDto>,
    pub page: usize,
    pub has_prev_page: bool,
    pub has_next_page: bool,
}

/// Rich action used by the future TSF edit-session and candidate-window layer.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionAction {
    /// Key is not ours; let the application handle it.
    PassThrough,
    /// Composition/candidates changed; update preedit and candidate UI.
    Update(SessionSnapshot),
    /// Commit this text to the application and clear composition.
    Commit(String),
    /// Composition dismissed; hide the candidate window.
    Dismiss,
}

/// Composition state machine shared by all future platform shells.
pub struct InputSession<C: EngineClient> {
    client: C,
    buffer: String,
    candidates: Vec<CandidateDto>,
    page: usize,
}

impl<C: EngineClient> InputSession<C> {
    pub fn new(client: C) -> Self {
        Self {
            client,
            buffer: String::new(),
            candidates: Vec::new(),
            page: 0,
        }
    }

    /// Current composition buffer (raw pinyin).
    #[must_use]
    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    /// Whether a composition is active.
    #[must_use]
    pub fn is_composing(&self) -> bool {
        !self.buffer.is_empty()
    }

    /// Candidates on the current page.
    #[must_use]
    pub fn page_candidates(&self) -> &[CandidateDto] {
        let start = self.page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(self.candidates.len());
        self.candidates.get(start..end).unwrap_or(&[])
    }

    /// Snapshot for candidate-window rendering and TSF edit sessions.
    #[must_use]
    pub fn snapshot(&self) -> SessionSnapshot {
        let start = self.page * PAGE_SIZE;
        SessionSnapshot {
            composition: self.buffer.clone(),
            candidates: self.page_candidates().to_vec(),
            page: self.page,
            has_prev_page: self.page > 0,
            has_next_page: start + PAGE_SIZE < self.candidates.len(),
        }
    }

    /// Handles a key event and returns the required UI action.
    pub fn handle_key(&mut self, key: Key) -> Outcome {
        match self.handle_key_action(key) {
            SessionAction::PassThrough => Outcome::PassThrough,
            SessionAction::Update(_) => Outcome::Updated,
            SessionAction::Commit(text) => Outcome::Commit(text),
            SessionAction::Dismiss => Outcome::Dismissed,
        }
    }

    /// Handles a key event and returns a rich action with render state.
    pub fn handle_key_action(&mut self, key: Key) -> SessionAction {
        match key {
            Key::Char(letter) if letter.is_ascii_lowercase() => {
                self.buffer.push(letter);
                self.refresh();
                SessionAction::Update(self.snapshot())
            }
            Key::Char(_) => SessionAction::PassThrough,
            Key::Digit(digit) => self.select_action(digit as usize),
            Key::Space => self.select_action(1),
            Key::Enter => {
                if !self.is_composing() {
                    return SessionAction::PassThrough;
                }
                if self.candidates.is_empty() {
                    self.clear();
                    SessionAction::Dismiss
                } else {
                    self.select_action(1)
                }
            }
            Key::Backspace => {
                if !self.is_composing() {
                    return SessionAction::PassThrough;
                }
                self.buffer.pop();
                if self.buffer.is_empty() {
                    self.clear();
                    return SessionAction::Dismiss;
                }
                self.refresh();
                SessionAction::Update(self.snapshot())
            }
            Key::Escape => {
                if !self.is_composing() {
                    return SessionAction::PassThrough;
                }
                self.clear();
                SessionAction::Dismiss
            }
            Key::PageNext => self.turn_page_action(1),
            Key::PagePrev => self.turn_page_action(-1),
        }
    }

    fn select_action(&mut self, index_on_page: usize) -> SessionAction {
        if !self.is_composing() {
            return SessionAction::PassThrough;
        }
        let index = self.page * PAGE_SIZE + index_on_page.saturating_sub(1);
        let Some(candidate) = self.candidates.get(index).cloned() else {
            return SessionAction::Update(self.snapshot());
        };

        let _predictions = self.client.commit(&candidate.text, &candidate.reading);
        self.clear();
        SessionAction::Commit(candidate.text)
    }

    fn turn_page_action(&mut self, delta: i64) -> SessionAction {
        if !self.is_composing() {
            return SessionAction::PassThrough;
        }
        let last_page = self.candidates.len().saturating_sub(1) / PAGE_SIZE;
        let next = i64::try_from(self.page).unwrap_or_default() + delta;
        let clamped = usize::try_from(next.max(0))
            .unwrap_or_default()
            .min(last_page);
        if clamped != self.page {
            self.page = clamped;
        }
        SessionAction::Update(self.snapshot())
    }

    fn refresh(&mut self) {
        self.candidates = self.client.suggest(&self.buffer, CANDIDATE_FETCH_LIMIT);
        self.page = 0;
    }

    fn clear(&mut self) {
        self.buffer.clear();
        self.candidates.clear();
        self.page = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::{EngineClient, InputSession, Key, Outcome, PAGE_SIZE, SessionAction};
    use novatype_protocol::CandidateDto;

    struct MockClient {
        committed: Vec<String>,
    }

    impl MockClient {
        fn new() -> Self {
            Self {
                committed: Vec::new(),
            }
        }
    }

    impl EngineClient for MockClient {
        fn suggest(&mut self, input: &str, _limit: usize) -> Vec<CandidateDto> {
            if input.is_empty() {
                return Vec::new();
            }
            (0..12)
                .map(|index| CandidateDto {
                    text: format!("词{index}"),
                    reading: vec![input.to_string()],
                    score: f64::from(12 - index),
                })
                .collect()
        }

        fn commit(&mut self, text: &str, _reading: &[String]) -> Vec<String> {
            self.committed.push(text.to_string());
            vec!["联想".to_string()]
        }
    }

    fn session() -> InputSession<MockClient> {
        InputSession::new(MockClient::new())
    }

    #[test]
    fn typing_builds_composition_and_candidates() {
        let mut session = session();

        assert_eq!(session.handle_key(Key::Char('n')), Outcome::Updated);
        assert_eq!(session.handle_key(Key::Char('i')), Outcome::Updated);
        assert_eq!(session.buffer(), "ni");
        assert_eq!(session.page_candidates().len(), PAGE_SIZE);
    }

    #[test]
    fn rich_update_contains_snapshot_for_candidate_window() {
        let mut session = session();

        let action = session.handle_key_action(Key::Char('n'));

        let SessionAction::Update(snapshot) = action else {
            panic!("expected update");
        };
        assert_eq!(snapshot.composition, "n");
        assert_eq!(snapshot.candidates.len(), PAGE_SIZE);
        assert_eq!(snapshot.page, 0);
        assert!(!snapshot.has_prev_page);
        assert!(snapshot.has_next_page);
    }

    #[test]
    fn rich_paging_snapshot_tracks_page_bounds() {
        let mut session = session();
        session.handle_key(Key::Char('n'));

        let action = session.handle_key_action(Key::PageNext);

        let SessionAction::Update(snapshot) = action else {
            panic!("expected update");
        };
        assert_eq!(snapshot.page, 1);
        assert!(snapshot.has_prev_page);
        assert!(snapshot.has_next_page);
    }

    #[test]
    fn rich_commit_carries_text() {
        let mut session = session();
        session.handle_key(Key::Char('n'));

        let action = session.handle_key_action(Key::Space);

        assert_eq!(action, SessionAction::Commit("词0".to_string()));
        assert_eq!(session.snapshot().composition, "");
    }

    #[test]
    fn space_commits_first_candidate() {
        let mut session = session();
        session.handle_key(Key::Char('n'));

        let outcome = session.handle_key(Key::Space);

        assert_eq!(outcome, Outcome::Commit("词0".to_string()));
        assert!(!session.is_composing());
    }

    #[test]
    fn enter_commits_or_clears_active_composition() {
        let mut session = session();
        session.handle_key(Key::Char('n'));

        assert_eq!(
            session.handle_key(Key::Enter),
            Outcome::Commit("词0".to_string())
        );
        assert!(!session.is_composing());

        let mut empty = InputSession::new(MockClient::new());
        empty.buffer.push('x');
        assert_eq!(empty.handle_key(Key::Enter), Outcome::Dismissed);
        assert!(!empty.is_composing());
    }

    #[test]
    fn digit_selects_on_current_page() {
        let mut session = session();
        session.handle_key(Key::Char('n'));
        session.handle_key(Key::PageNext);

        let outcome = session.handle_key(Key::Digit(2));

        assert_eq!(outcome, Outcome::Commit("词6".to_string()));
    }

    #[test]
    fn backspace_erases_and_dismisses() {
        let mut session = session();
        session.handle_key(Key::Char('n'));
        session.handle_key(Key::Char('i'));

        assert_eq!(session.handle_key(Key::Backspace), Outcome::Updated);
        assert_eq!(session.handle_key(Key::Backspace), Outcome::Dismissed);
        assert_eq!(session.handle_key(Key::Backspace), Outcome::PassThrough);
    }

    #[test]
    fn escape_dismisses_composition() {
        let mut session = session();
        session.handle_key(Key::Char('n'));

        assert_eq!(session.handle_key(Key::Escape), Outcome::Dismissed);
        assert_eq!(session.handle_key(Key::Escape), Outcome::PassThrough);
    }

    #[test]
    fn paging_clamps_to_bounds() {
        let mut session = session();
        session.handle_key(Key::Char('n'));

        session.handle_key(Key::PagePrev);
        assert_eq!(session.page_candidates()[0].text, "词0");

        session.handle_key(Key::PageNext);
        session.handle_key(Key::PageNext);
        session.handle_key(Key::PageNext);
        assert_eq!(session.page_candidates()[0].text, "词10");
    }

    #[test]
    fn uppercase_and_symbols_pass_through() {
        let mut session = session();

        assert_eq!(session.handle_key(Key::Char('A')), Outcome::PassThrough);
        assert_eq!(session.handle_key(Key::Char('1')), Outcome::PassThrough);
    }
}

#[cfg(all(test, windows))]
mod export_tests {
    use super::{DllCanUnloadNow, DllGetClassObject, S_OK};
    use crate::com::{CLSID_NOVATYPE_TEXT_SERVICE_FOR_TEST, Guid, IID_ICLASS_FACTORY_FOR_TEST};
    use core::ffi::c_void;
    use core::ptr;

    #[test]
    fn exported_get_class_object_returns_factory() {
        let mut object: *mut c_void = ptr::null_mut();
        let hr = DllGetClassObject(
            &CLSID_NOVATYPE_TEXT_SERVICE_FOR_TEST,
            &IID_ICLASS_FACTORY_FOR_TEST,
            &raw mut object,
        );

        assert_eq!(hr, S_OK);
        assert!(!object.is_null());
    }

    #[test]
    fn exported_can_unload_starts_true() {
        assert_eq!(DllCanUnloadNow(), S_OK);
    }

    #[test]
    fn exported_get_class_object_rejects_unknown_guid() {
        let unknown = Guid::for_test(0xDEAD_BEEF);
        let mut object: *mut c_void = ptr::null_mut();
        let hr = DllGetClassObject(
            &raw const unknown,
            &IID_ICLASS_FACTORY_FOR_TEST,
            &raw mut object,
        );

        assert_ne!(hr, S_OK);
        assert!(object.is_null());
    }
}
