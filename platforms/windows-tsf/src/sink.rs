/// Cookie assigned by TSF when a sink is advised.
pub type SinkCookie = u32;

/// Tracks TSF sink lifecycle for the text service.
///
/// The current implementation is a deterministic model used by the COM object
/// and tests. The future `ITfSource::AdviseSink` integration should update this
/// state from real TSF cookies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SinkState {
    key_event: Option<SinkCookie>,
}

impl SinkState {
    /// Records a key-event sink cookie.
    pub fn advise_key_event(&mut self, cookie: SinkCookie) {
        self.key_event = Some(cookie);
    }

    /// Clears the key-event sink cookie.
    pub fn unadvise_key_event(&mut self) {
        self.key_event = None;
    }

    /// Whether key events should currently be handled by `NovaType`.
    #[must_use]
    pub fn key_event_active(&self) -> bool {
        self.key_event.is_some()
    }

    /// Current key-event cookie, if any.
    #[must_use]
    pub fn key_event_cookie(&self) -> Option<SinkCookie> {
        self.key_event
    }
}

#[cfg(test)]
mod tests {
    use super::SinkState;

    #[test]
    fn tracks_key_event_cookie() {
        let mut state = SinkState::default();

        assert!(!state.key_event_active());
        state.advise_key_event(42);
        assert!(state.key_event_active());
        assert_eq!(state.key_event_cookie(), Some(42));
        state.unadvise_key_event();
        assert!(!state.key_event_active());
    }
}
