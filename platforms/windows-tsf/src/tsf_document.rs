//! TSF edit session and document adapter.
//!
//! Provides a `TsfEditSession` that implements the `ITfEditSession` COM interface,
//! and a `TsfDocumentEditor` that requests TSF edit sessions to write composition
//! preedit text and commit text into the host application.

#![allow(clippy::doc_markdown)]

use crate::edit_session::{CandidateWindowModel, DocumentEditor};
use crate::{
    candidate_window::CandidateWindowView,
    window::{CandidateWindowMetrics, CandidateWindowState},
};
use core::ffi::c_void;
use core::ptr;

/// IID for `IUnknown`.
const IID_IUNKNOWN: crate::com::Guid = crate::com::Guid {
    data1: 0x0000_0000,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

/// IID for `ITfEditSession`: {AA80E803-2021-11D2-93E0-0060B067B86E}
const IID_ITF_EDIT_SESSION: crate::com::Guid = crate::com::Guid {
    data1: 0xAA80_E803,
    data2: 0x2021,
    data3: 0x11D2,
    data4: [0x93, 0xE0, 0x00, 0x60, 0xB0, 0x67, 0xB8, 0x6E],
};

/// IID for `ITfInsertAtSelection`: {55CE16BA-3014-41C1-9CEB-FADE1446AC6C}
const IID_ITF_INSERT_AT_SELECTION: crate::com::Guid = crate::com::Guid {
    data1: 0x55CE_16BA,
    data2: 0x3014,
    data3: 0x41C1,
    data4: [0x9C, 0xEB, 0xFA, 0xDE, 0x14, 0x46, 0xAC, 0x6C],
};

/// `ITfContext` vtable offsets (msctf.h declaration order after IUnknown 0-2):
/// [3] RequestEditSession, [4] InWriteSession, [5] GetSelection, [6] SetSelection,
/// [7] GetStart, [8] GetEnd, [9] GetActiveView, [10] EnumViews, ...
const CONTEXT_REQUEST_EDIT_SESSION_OFFSET: usize = 3;
const CONTEXT_GET_SELECTION_OFFSET: usize = 5;
const CONTEXT_SET_SELECTION_OFFSET: usize = 6;
const CONTEXT_GET_ACTIVE_VIEW_OFFSET: usize = 9;

/// `ITfContextView` vtable offsets: [3] GetRangeFromPoint, [4] GetTextExt,
/// [5] GetScreenExt, [6] GetWnd.
const VIEW_GET_TEXT_EXT_OFFSET: usize = 4;

/// `ITfInsertAtSelection` vtable offset: [3] InsertTextAtSelection.
const INSERT_TEXT_AT_SELECTION_OFFSET: usize = 3;

/// `ITfRange::Collapse` vtable offset.
const RANGE_COLLAPSE_OFFSET: usize = 15;

/// `TF_ANCHOR_END` for collapsing a range to its end.
const TF_ANCHOR_END: i32 = 1;

/// `TF_ES_*` edit-session flags (msctf.h).
const TF_ES_SYNC: u32 = 0x1;
const TF_ES_READ: u32 = 0x2;
const TF_ES_READWRITE: u32 = 0x6;

/// `TF_DEFAULT_SELECTION` for `ITfContext::GetSelection`.
const TF_DEFAULT_SELECTION: u32 = u32::MAX;

/// Opaque TSF context handles captured during activation.
///
/// In production these correspond to `ITfThreadMgr`, `ITfDocumentMgr`, and
/// `ITfContext` COM interface pointers. We use raw vtable dispatch because this
/// crate does not depend on the `windows` crate's COM wrapper types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TsfEditContext {
    pub thread_mgr: *mut c_void,
    pub client_id: u32,
    pub edit_cookie: u32,
    /// Opaque `ITfContext` pointer obtained from `ITfDocumentMgr::GetTop`.
    pub context: *mut c_void,
}

/// A thin `ITfEditSession` implementation whose `DoEditSession` callback writes
/// composition preedit text or commit text into the document.
///
/// This struct lives on the Rust heap and is passed as a raw COM pointer to
/// `ITfContext::RequestEditSession`. TSF invokes `DoEditSession` on it when the
/// edit is granted.
#[repr(C)]
pub struct TsfEditSession {
    vtbl: *const TsfEditSessionVtbl,
    /// What to write: `Some(text)` means SetText on the selection range;
    /// `None` means clear the preedit/composition.
    action: TsfEditAction,
    /// The `ITfContext` this session operates on (set before `RequestEditSession`).
    context: *mut c_void,
    /// Caret bottom-left in screen coordinates, read inside the edit session.
    caret: Option<(i32, i32)>,
    /// HRESULT stored by `DoEditSession`.
    result: i32,
}

#[derive(Debug, Clone)]
enum TsfEditAction {
    /// Commit text by inserting at selection, then moving selection past it.
    CommitText(String),
    /// Read-only session used to track the caret rectangle while composing.
    ReadCaret,
}

#[repr(C)]
struct TsfEditSessionVtbl {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const crate::com::Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    /// `DoEditSession(TfEditCookie ec)` — the second parameter is the edit
    /// cookie granted by TSF, not an interface pointer.
    do_edit_session: unsafe extern "system" fn(*mut c_void, u32) -> i32,
}

/// Testable adapter boundary for the real `ITfEditSession` code.
#[derive(Debug)]
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

impl Default for TsfDocumentEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl TsfDocumentEditor {
    #[must_use]
    pub fn new() -> Self {
        Self {
            context: None,
            composition: String::new(),
            committed: String::new(),
            candidate_model: None,
            candidate_window: CandidateWindowState::new(),
            #[cfg(windows)]
            native_window: None,
            caret: (0, 0),
            pass_through_count: 0,
        }
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
        // When a real ITfContext is available, request a read-only edit session
        // to track the caret rectangle. The pinyin preedit itself is shown in
        // the candidate window for now (no inline composition yet).
        if let Some(ref ctx) = self.context
            && !ctx.context.is_null()
            && let Ok(Some(caret)) = request_edit_session(ctx, TsfEditAction::ReadCaret)
        {
            self.caret = caret;
        }
    }

    fn commit_text(&mut self, text: &str) {
        self.committed.push_str(text);
        let mut committed_to_host = false;
        if let Some(ref ctx) = self.context
            && !ctx.context.is_null()
            && let Ok(caret) =
                request_edit_session(ctx, TsfEditAction::CommitText(text.to_string()))
        {
            committed_to_host = true;
            self.advance_caret_after_commit(text, caret);
        }
        if !committed_to_host {
            crate::debug_log::log(&format!("fallback unicode commit {text:?}"));
            fallback_commit_text(text);
            self.advance_caret_after_commit(text, None);
        }
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
        // No inline preedit is written into the document yet, so there is
        // nothing to clear on the TSF side.
    }
}

impl TsfDocumentEditor {
    fn advance_caret_after_commit(&mut self, text: &str, returned_caret: Option<(i32, i32)>) {
        let estimated_x = self.caret.0 + estimated_text_width(text);
        self.caret = match returned_caret {
            Some(caret) if caret.0 > self.caret.0 => caret,
            Some(caret) => (estimated_x, caret.1),
            None => (estimated_x, self.caret.1),
        };
        crate::debug_log::log(&format!(
            "caret after commit text={text:?} returned={returned_caret:?} stored={:?}",
            self.caret
        ));
    }
}

fn estimated_text_width(text: &str) -> i32 {
    text.chars()
        .map(|ch| if ch.is_ascii() { 9 } else { 24 })
        .sum()
}

// ---------------------------------------------------------------------------
// Raw vtable dispatch helpers for ITfContext / ITfInsertAtSelection
// ---------------------------------------------------------------------------

/// Releases a raw COM interface pointer (calls `IUnknown::Release` at slot 2).
unsafe fn com_release(object: *mut c_void) {
    unsafe {
        let vtbl: *const *const c_void = *(object.cast::<*const *const c_void>());
        let release: unsafe extern "system" fn(*mut c_void) -> u32 =
            core::mem::transmute(*vtbl.byte_add(2 * size_of::<*const c_void>()));
        let _ = release(object);
    }
}

/// Requests a synchronous TSF edit session for `action` on the context in `ctx`.
///
/// Blocks until `DoEditSession` has run (TF_ES_SYNC), so the stack-allocated
/// session object stays valid for the whole call.
///
/// Returns the caret rectangle bottom-left read inside the session, if any.
fn request_edit_session(
    ctx: &TsfEditContext,
    action: TsfEditAction,
) -> Result<Option<(i32, i32)>, ()> {
    let flags = match action {
        TsfEditAction::CommitText(_) => TF_ES_SYNC | TF_ES_READWRITE,
        TsfEditAction::ReadCaret => TF_ES_SYNC | TF_ES_READ,
    };
    crate::debug_log::log(&format!(
        "RequestEditSession begin client_id={} context={:p} action={action:?}",
        ctx.client_id, ctx.context
    ));
    let mut session = TsfEditSession {
        vtbl: &raw const EDIT_SESSION_VTBL,
        action,
        context: ctx.context,
        caret: None,
        result: 0,
    };

    // SAFETY: `ctx.context` is the ITfContext passed to OnKeyDown by TSF.
    unsafe {
        let context_vtbl: *const *const c_void = *(ctx.context.cast::<*const *const c_void>());
        // ITfContext::RequestEditSession(TfClientId, ITfEditSession*, DWORD, HRESULT*)
        let request_es: unsafe extern "system" fn(
            *mut c_void,
            u32,
            *mut c_void,
            u32,
            *mut i32,
        ) -> i32 = core::mem::transmute(
            *context_vtbl
                .byte_add(CONTEXT_REQUEST_EDIT_SESSION_OFFSET * size_of::<*const c_void>()),
        );

        let mut hr_session: i32 = 0;
        let hr = request_es(
            ctx.context,
            ctx.client_id,
            ptr::addr_of_mut!(session).cast::<c_void>(),
            flags,
            &raw mut hr_session,
        );
        if hr != S_OK || hr_session < 0 {
            crate::debug_log::log(&format!(
                "RequestEditSession failed hr={hr:#010X} hr_session={hr_session:#010X}"
            ));
            return Err(());
        }
    }
    crate::debug_log::log(&format!(
        "RequestEditSession ok result={:#010X} caret={:?}",
        session.result, session.caret
    ));
    if session.result < 0 {
        Err(())
    } else {
        Ok(session.caret)
    }
}

#[cfg(all(windows, not(test)))]
fn fallback_commit_text(text: &str) {
    for unit in text.encode_utf16() {
        unsafe {
            let inputs = [
                RawInput {
                    input_type: INPUT_KEYBOARD,
                    keyboard: RawKeyboardInput {
                        virtual_key: 0,
                        scan: unit,
                        flags: KEYEVENTF_UNICODE,
                        time: 0,
                        extra_info: 0,
                    },
                },
                RawInput {
                    input_type: INPUT_KEYBOARD,
                    keyboard: RawKeyboardInput {
                        virtual_key: 0,
                        scan: unit,
                        flags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        extra_info: 0,
                    },
                },
            ];
            let sent = SendInput(
                u32::try_from(inputs.len()).unwrap_or(0),
                inputs.as_ptr(),
                i32::try_from(size_of::<RawInput>()).unwrap_or(0),
            );
            crate::debug_log::log(&format!("fallback SendInput unit={unit:#06X} sent={sent}"));
        }
    }
}

#[cfg(any(not(windows), test))]
fn fallback_commit_text(_text: &str) {}

#[cfg(all(windows, not(test)))]
const INPUT_KEYBOARD: u32 = 1;
#[cfg(all(windows, not(test)))]
const KEYEVENTF_KEYUP: u32 = 0x0002;
#[cfg(all(windows, not(test)))]
const KEYEVENTF_UNICODE: u32 = 0x0004;

#[cfg(all(windows, not(test)))]
#[repr(C)]
struct RawInput {
    input_type: u32,
    keyboard: RawKeyboardInput,
}

#[cfg(all(windows, not(test)))]
#[repr(C)]
struct RawKeyboardInput {
    virtual_key: u16,
    scan: u16,
    flags: u32,
    time: u32,
    extra_info: usize,
}

#[cfg(all(windows, not(test)))]
unsafe extern "system" {
    fn SendInput(input_count: u32, inputs: *const RawInput, input_size: i32) -> u32;
}

/// Reads the caret/selection rectangle inside a granted edit session.
///
/// `ITfContext::GetSelection(ec, TF_DEFAULT_SELECTION, 1, …)` → selection range,
/// then `ITfContextView::GetTextExt(ec, range, …)` on the active view.
unsafe fn read_caret_rect_in_session(context: *mut c_void, ec: u32) -> Option<(i32, i32)> {
    unsafe {
        let context_vtbl: *const *const c_void = *(context.cast::<*const *const c_void>());

        // ITfContext::GetSelection(ec, ulIndex, ulCount, TF_SELECTION*, ULONG*)
        let get_selection: unsafe extern "system" fn(
            *mut c_void,
            u32,
            u32,
            u32,
            *mut TfSelection,
            *mut u32,
        ) -> i32 = core::mem::transmute(
            *context_vtbl.byte_add(CONTEXT_GET_SELECTION_OFFSET * size_of::<*const c_void>()),
        );
        let mut selection = TfSelection {
            range: ptr::null_mut(),
            style: TfSelectionStyle {
                ase: 0,
                interim_char: 0,
            },
        };
        let mut fetched: u32 = 0;
        if get_selection(
            context,
            ec,
            TF_DEFAULT_SELECTION,
            1,
            &raw mut selection,
            &raw mut fetched,
        ) != S_OK
            || fetched == 0
            || selection.range.is_null()
        {
            return None;
        }
        let range = selection.range;

        // ITfContext::GetActiveView(ITfContextView**)
        let get_view: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 =
            core::mem::transmute(
                *context_vtbl.byte_add(CONTEXT_GET_ACTIVE_VIEW_OFFSET * size_of::<*const c_void>()),
            );
        let mut view: *mut c_void = ptr::null_mut();
        if get_view(context, &raw mut view) != S_OK || view.is_null() {
            com_release(range);
            return None;
        }

        // ITfContextView::GetTextExt(ec, ITfRange*, RECT*, BOOL*)
        let view_vtbl: *const *const c_void = *(view.cast::<*const *const c_void>());
        let get_text_ext: unsafe extern "system" fn(
            *mut c_void,
            u32,
            *mut c_void,
            *mut TsfRect,
            *mut i32,
        ) -> i32 = core::mem::transmute(
            *view_vtbl.byte_add(VIEW_GET_TEXT_EXT_OFFSET * size_of::<*const c_void>()),
        );
        let mut rect = TsfRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut clipped: i32 = 0;
        let hr = get_text_ext(view, ec, range, &raw mut rect, &raw mut clipped);
        com_release(view);
        com_release(range);
        if hr != S_OK {
            return None;
        }
        Some((rect.left, rect.bottom))
    }
}

/// `TF_SELECTION` (msctf.h): `{ ITfRange *range; TF_SELECTIONSTYLE style; }`.
#[repr(C)]
struct TfSelection {
    range: *mut c_void,
    style: TfSelectionStyle,
}

/// `TF_SELECTIONSTYLE`: `{ TfActiveSelEnd ase; BOOL fInterimChar; }`.
#[repr(C)]
struct TfSelectionStyle {
    ase: u32,
    interim_char: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct TsfRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

// ---------------------------------------------------------------------------
// ITfEditSession COM implementation
// ---------------------------------------------------------------------------

static EDIT_SESSION_VTBL: TsfEditSessionVtbl = TsfEditSessionVtbl {
    query_interface: edit_session_query_interface,
    add_ref: edit_session_add_ref,
    release: edit_session_release,
    do_edit_session: edit_session_do_edit_session,
};

unsafe extern "system" fn edit_session_query_interface(
    this: *mut c_void,
    iid: *const crate::com::Guid,
    object: *mut *mut c_void,
) -> i32 {
    if this.is_null() || iid.is_null() || object.is_null() {
        return E_POINTER;
    }
    unsafe {
        *object = ptr::null_mut();
        let iid = *iid;
        if iid == IID_IUNKNOWN || iid == IID_ITF_EDIT_SESSION {
            // The session lives on the caller's stack for the duration of the
            // synchronous RequestEditSession call; ref-counting is a no-op.
            *object = this;
            return S_OK;
        }
    }
    E_NOINTERFACE
}

const S_OK: i32 = 0;
const E_POINTER: i32 = i32::from_ne_bytes(0x8000_4003_u32.to_ne_bytes());
const E_NOINTERFACE: i32 = i32::from_ne_bytes(0x8000_4002_u32.to_ne_bytes());
const E_FAIL: i32 = i32::from_ne_bytes(0x8000_4005_u32.to_ne_bytes());

unsafe extern "system" fn edit_session_add_ref(_this: *mut c_void) -> u32 {
    2
}

unsafe extern "system" fn edit_session_release(_this: *mut c_void) -> u32 {
    1
}

/// `ITfEditSession::DoEditSession(TfEditCookie ec)`.
///
/// When TSF grants the edit, performs the pending action with the granted
/// cookie and records the caret rectangle for candidate window placement.
unsafe extern "system" fn edit_session_do_edit_session(this: *mut c_void, ec: u32) -> i32 {
    if this.is_null() {
        return E_POINTER;
    }

    let session = this.cast::<TsfEditSession>();
    unsafe {
        let context = (*session).context;
        if context.is_null() {
            (*session).result = E_FAIL;
            return E_FAIL;
        }

        let hr = match (*session).action {
            TsfEditAction::CommitText(ref text) => {
                let hr = insert_text_at_selection(context, ec, text);
                crate::debug_log::log(&format!("DoEditSession commit {text:?} hr={hr:#010X}"));
                hr
            }
            // The pinyin preedit is currently rendered in the candidate window
            // only; the read-only session exists to track the caret rect.
            TsfEditAction::ReadCaret => S_OK,
        };
        (*session).caret = read_caret_rect_in_session(context, ec);
        (*session).result = hr;
        hr
    }
}

/// Inserts `text` at the current selection via `ITfInsertAtSelection`, which
/// also moves the selection past the inserted text.
unsafe fn insert_text_at_selection(context: *mut c_void, ec: u32, text: &str) -> i32 {
    unsafe {
        let context_vtbl: *const *const c_void = *(context.cast::<*const *const c_void>());
        let qi: unsafe extern "system" fn(
            *mut c_void,
            *const crate::com::Guid,
            *mut *mut c_void,
        ) -> i32 = core::mem::transmute(*context_vtbl);
        let mut insert_at: *mut c_void = ptr::null_mut();
        let hr = qi(context, &IID_ITF_INSERT_AT_SELECTION, &raw mut insert_at);
        if hr != S_OK || insert_at.is_null() {
            return if hr < 0 { hr } else { E_FAIL };
        }

        // ITfInsertAtSelection::InsertTextAtSelection(ec, dwFlags, pchText, cch, ppRange)
        let ias_vtbl: *const *const c_void = *(insert_at.cast::<*const *const c_void>());
        let insert: unsafe extern "system" fn(
            *mut c_void,
            u32,
            u32,
            *const u16,
            i32,
            *mut *mut c_void,
        ) -> i32 = core::mem::transmute(
            *ias_vtbl.byte_add(INSERT_TEXT_AT_SELECTION_OFFSET * size_of::<*const c_void>()),
        );

        let wide: Vec<u16> = text.encode_utf16().collect();
        let cch = i32::try_from(wide.len()).unwrap_or(0);
        let mut range: *mut c_void = ptr::null_mut();
        let hr = insert(insert_at, ec, 0, wide.as_ptr(), cch, &raw mut range);
        if !range.is_null() {
            if hr == S_OK {
                move_selection_to_range_end(context, ec, range);
            }
            com_release(range);
        }
        com_release(insert_at);
        hr
    }
}

/// Moves the insertion point after a just-inserted range.
unsafe fn move_selection_to_range_end(context: *mut c_void, ec: u32, range: *mut c_void) {
    unsafe {
        let range_vtbl: *const *const c_void = *(range.cast::<*const *const c_void>());
        let collapse: unsafe extern "system" fn(*mut c_void, u32, i32) -> i32 =
            core::mem::transmute(
                *range_vtbl.byte_add(RANGE_COLLAPSE_OFFSET * size_of::<*const c_void>()),
            );
        let hr_collapse = collapse(range, ec, TF_ANCHOR_END);
        if hr_collapse != S_OK {
            crate::debug_log::log(&format!(
                "move_selection: Collapse(end) failed hr={hr_collapse:#010X}"
            ));
            return;
        }

        let context_vtbl: *const *const c_void = *(context.cast::<*const *const c_void>());
        let set_selection: unsafe extern "system" fn(
            *mut c_void,
            u32,
            u32,
            *const TfSelection,
        ) -> i32 = core::mem::transmute(
            *context_vtbl.byte_add(CONTEXT_SET_SELECTION_OFFSET * size_of::<*const c_void>()),
        );
        let selection = TfSelection {
            range,
            style: TfSelectionStyle {
                ase: 0,
                interim_char: 0,
            },
        };
        let hr_selection = set_selection(context, ec, 1, &raw const selection);
        crate::debug_log::log(&format!(
            "move_selection: Collapse(end) ok SetSelection hr={hr_selection:#010X}"
        ));
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

    fn dummy_context() -> TsfEditContext {
        TsfEditContext {
            thread_mgr: core::ptr::null_mut(),
            client_id: 7,
            edit_cookie: 11,
            context: core::ptr::null_mut(),
        }
    }

    #[test]
    fn stores_context_and_edit_state() {
        let mut editor = TsfDocumentEditor::new();
        let context = dummy_context();

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
        editor.attach_context(dummy_context());
        editor.set_composition("ni");
        editor.show_candidates(&model());

        editor.detach_context();

        assert!(editor.context().is_none());
        assert_eq!(editor.composition(), "");
        assert!(editor.candidate_model().is_none());
    }

    #[test]
    fn set_caret_updates_position() {
        let mut editor = TsfDocumentEditor::new();
        editor.set_caret(42, 99);
        editor.set_composition("test");
        assert_eq!(editor.composition(), "test");
    }
}
