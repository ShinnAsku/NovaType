/// Cookie assigned by TSF when a sink is advised.
pub type SinkCookie = u32;

pub type SinkResult<T> = Result<T, String>;

/// Abstraction over TSF sink advise/unadvise calls.
pub trait SinkAdvisor {
    /// Advises the key-event sink and returns the TSF cookie.
    ///
    /// # Errors
    ///
    /// Returns an error when TSF refuses the sink connection.
    fn advise_key_event(&mut self) -> SinkResult<SinkCookie>;

    /// Unadvises the key-event sink.
    ///
    /// # Errors
    ///
    /// Returns an error when TSF refuses to remove the sink.
    fn unadvise_key_event(&mut self, cookie: SinkCookie) -> SinkResult<()>;
}

/// Deterministic local advisor used until `ITfSource` is wired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSinkAdvisor {
    next_cookie: SinkCookie,
    unadvised: Vec<SinkCookie>,
}

/// Windows TSF sink advisor placeholder.
///
/// The fields stay opaque until the `windows-rs` `ITfSource` interface is wired
/// in. This keeps the `TextService` lifecycle independent from the transport.
#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealSinkAdvisor {
    source: *mut core::ffi::c_void,
    sink: *mut core::ffi::c_void,
}

#[cfg(windows)]
impl RealSinkAdvisor {
    #[must_use]
    pub fn new(source: *mut core::ffi::c_void, sink: *mut core::ffi::c_void) -> Self {
        Self { source, sink }
    }

    #[must_use]
    pub fn has_source_and_sink(&self) -> bool {
        !self.source.is_null() && !self.sink.is_null()
    }
}

#[cfg(windows)]
impl SinkAdvisor for RealSinkAdvisor {
    fn advise_key_event(&mut self) -> SinkResult<SinkCookie> {
        if !self.has_source_and_sink() {
            return Err("missing ITfSource or ITfKeyEventSink".to_string());
        }
        // TODO(v0.3): call ITfSource::AdviseSink(IID_ITfKeyEventSink, sink, &mut cookie).
        Ok(1)
    }

    fn unadvise_key_event(&mut self, cookie: SinkCookie) -> SinkResult<()> {
        if cookie == 0 {
            return Err("invalid sink cookie".to_string());
        }
        // TODO(v0.3): call ITfSource::UnadviseSink(cookie).
        Ok(())
    }
}

impl Default for LocalSinkAdvisor {
    fn default() -> Self {
        Self {
            next_cookie: 1,
            unadvised: Vec::new(),
        }
    }
}

impl SinkAdvisor for LocalSinkAdvisor {
    fn advise_key_event(&mut self) -> SinkResult<SinkCookie> {
        let cookie = self.next_cookie;
        self.next_cookie = self.next_cookie.saturating_add(1);
        Ok(cookie)
    }

    fn unadvise_key_event(&mut self, cookie: SinkCookie) -> SinkResult<()> {
        self.unadvised.push(cookie);
        Ok(())
    }
}

impl LocalSinkAdvisor {
    #[must_use]
    pub fn unadvised(&self) -> &[SinkCookie] {
        &self.unadvised
    }
}

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
    use super::{LocalSinkAdvisor, SinkAdvisor, SinkState};

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

    #[test]
    fn local_advisor_generates_and_records_cookies() {
        let mut advisor = LocalSinkAdvisor::default();

        let first = advisor.advise_key_event().expect("advise");
        let second = advisor.advise_key_event().expect("advise");
        advisor.unadvise_key_event(first).expect("unadvise");

        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert_eq!(advisor.unadvised(), &[1]);
    }

    #[cfg(windows)]
    #[test]
    fn real_advisor_requires_source_and_sink() {
        let mut advisor = super::RealSinkAdvisor::new(core::ptr::null_mut(), core::ptr::null_mut());

        assert!(advisor.advise_key_event().is_err());
        assert!(advisor.unadvise_key_event(0).is_err());
    }
}
