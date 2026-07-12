use crate::{
    DaemonClient, InputSession, key_event,
    sink::{SinkCookie, SinkState},
    tsf_document::{TsfDocumentEditor, TsfEditContext},
};

#[cfg(test)]
use crate::{Outcome, edit_session};
use core::ffi::c_void;
use core::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl Guid {
    #[cfg(test)]
    pub const fn for_test(data1: u32) -> Self {
        Self {
            data1,
            data2: 0,
            data3: 0,
            data4: [0; 8],
        }
    }
}

const S_OK: i32 = 0;
const E_POINTER: i32 = i32::from_ne_bytes(0x8000_4003_u32.to_ne_bytes());
const E_NOINTERFACE: i32 = i32::from_ne_bytes(0x8000_4002_u32.to_ne_bytes());
const CLASS_E_CLASSNOTAVAILABLE: i32 = i32::from_ne_bytes(0x8004_0111_u32.to_ne_bytes());
const CLASS_E_NOAGGREGATION: i32 = i32::from_ne_bytes(0x8004_0110_u32.to_ne_bytes());
const WH_KEYBOARD: i32 = 2;
const WH_GETMESSAGE: i32 = 3;
const HC_ACTION: i32 = 0;
const WM_NULL: u32 = 0x0000;
const WM_KEYDOWN: u32 = 0x0100;
const WM_SYSKEYDOWN: u32 = 0x0104;

static OBJECT_COUNT: AtomicU32 = AtomicU32::new(0);
static LOCK_COUNT: AtomicU32 = AtomicU32::new(0);
static HOOK_SERVICE: AtomicPtr<TextService> = AtomicPtr::new(ptr::null_mut());

const IID_IUNKNOWN: Guid = Guid {
    data1: 0x0000_0000,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

const IID_ICLASS_FACTORY: Guid = Guid {
    data1: 0x0000_0001,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

const IID_ITF_TEXT_INPUT_PROCESSOR: Guid = Guid {
    data1: 0xAA80_E7F7,
    data2: 0x2021,
    data3: 0x11D2,
    data4: [0x93, 0xE0, 0x00, 0x60, 0xB0, 0x67, 0xB8, 0x6E],
};

const IID_ITF_TEXT_INPUT_PROCESSOR_EX: Guid = Guid {
    data1: 0x6E4E_2102,
    data2: 0xF9CD,
    data3: 0x433D,
    data4: [0xB4, 0x96, 0x30, 0x3C, 0xE0, 0x3A, 0x65, 0x07],
};

const IID_ITF_KEY_EVENT_SINK: Guid = Guid {
    data1: 0xAA80_E7F5,
    data2: 0x2021,
    data3: 0x11D2,
    data4: [0x93, 0xE0, 0x00, 0x60, 0xB0, 0x67, 0xB8, 0x6E],
};

/// IID for `ITfKeystrokeMgr`: {AA80E7F0-2021-11D2-93E0-0060B067B86E}
///
/// Key-event sinks are advised on the thread manager through this interface
/// (`AdviseKeyEventSink`), not through `ITfSource` on a document manager.
const IID_ITF_KEYSTROKE_MGR: Guid = Guid {
    data1: 0xAA80_E7F0,
    data2: 0x2021,
    data3: 0x11D2,
    data4: [0x93, 0xE0, 0x00, 0x60, 0xB0, 0x67, 0xB8, 0x6E],
};

#[cfg(test)]
pub const IID_ICLASS_FACTORY_FOR_TEST: Guid = IID_ICLASS_FACTORY;

#[cfg(test)]
pub const CLSID_NOVATYPE_TEXT_SERVICE_FOR_TEST: Guid = CLSID_NOVATYPE_TEXT_SERVICE;

const CLSID_NOVATYPE_TEXT_SERVICE: Guid = Guid {
    data1: 0x7E4B_71B0,
    data2: 0x5C48,
    data3: 0x45E8,
    data4: [0x9E, 0x4E, 0x4D, 0xFD, 0x16, 0xFE, 0x5E, 0x95],
};

#[repr(C)]
struct ClassFactory {
    vtbl: *const ClassFactoryVtbl,
}

// SAFETY: the class factory is immutable and all methods are stateless.
unsafe impl Sync for ClassFactory {}

#[repr(C)]
struct ClassFactoryVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    create_instance:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *const Guid, *mut *mut c_void) -> i32,
    lock_server: unsafe extern "system" fn(*mut c_void, i32) -> i32,
}

/// Releases a raw COM interface pointer (calls `IUnknown::Release` at vtable slot 2).
unsafe fn com_release(object: *mut c_void) {
    unsafe {
        let vtbl: *const *const c_void = *(object.cast::<*const *const c_void>());
        let release: unsafe extern "system" fn(*mut c_void) -> u32 =
            core::mem::transmute(*vtbl.byte_add(2 * size_of::<*const c_void>()));
        let _ = release(object);
    }
}

/// Offsets into `TextService` for the key-sink interface pointer.
///
/// Used by `service_from_key_sink` to reconstruct the owning `TextService` from
/// the `key_sink_iface` that TSF passes as `this` to key-sink callbacks.
#[allow(dead_code)]
const KEY_SINK_IFACE_OFFSET: usize = core::mem::offset_of!(TextService, key_sink_iface);

#[repr(C)]
struct TextService {
    processor_iface: TextProcessorInterface,
    key_sink_iface: KeyEventSinkInterface,
    ref_count: AtomicU32,
    thread_mgr: *mut c_void,
    client_id: u32,
    activated: bool,
    sinks: SinkState,
    keyboard_hook: *mut c_void,
    message_hook: *mut c_void,
    test_key_down_pending: bool,
    session: InputSession<DaemonClient>,
    document: TsfDocumentEditor,
}

unsafe extern "system" {
    fn SetWindowsHookExW(
        id_hook: i32,
        hook_proc: unsafe extern "system" fn(i32, usize, isize) -> isize,
        module: *mut c_void,
        thread_id: u32,
    ) -> *mut c_void;
    fn CallNextHookEx(hook: *mut c_void, code: i32, wparam: usize, lparam: isize) -> isize;
    fn UnhookWindowsHookEx(hook: *mut c_void) -> i32;
    fn GetCurrentThreadId() -> u32;
}

#[repr(C)]
struct TextProcessorInterface {
    vtbl: *const TextServiceVtbl,
}

#[repr(C)]
struct KeyEventSinkInterface {
    vtbl: *const KeyEventSinkVtbl,
}

#[repr(C)]
struct TextServiceVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    activate: unsafe extern "system" fn(*mut c_void, *mut c_void, u32) -> i32,
    deactivate: unsafe extern "system" fn(*mut c_void) -> i32,
    activate_ex: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, u32) -> i32,
}

#[repr(C)]
struct KeyEventSinkVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    on_set_focus: unsafe extern "system" fn(*mut c_void, i32) -> i32,
    on_test_key_down:
        unsafe extern "system" fn(*mut c_void, *mut c_void, usize, isize, *mut i32) -> i32,
    on_key_down: unsafe extern "system" fn(*mut c_void, *mut c_void, usize, isize, *mut i32) -> i32,
    on_test_key_up:
        unsafe extern "system" fn(*mut c_void, *mut c_void, usize, isize, *mut i32) -> i32,
    on_key_up: unsafe extern "system" fn(*mut c_void, *mut c_void, usize, isize, *mut i32) -> i32,
    on_preserved_key:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *const c_void, *mut i32) -> i32,
}

static CLASS_FACTORY_VTBL: ClassFactoryVtbl = ClassFactoryVtbl {
    query_interface,
    add_ref,
    release,
    create_instance,
    lock_server,
};

static CLASS_FACTORY: ClassFactory = ClassFactory {
    vtbl: &raw const CLASS_FACTORY_VTBL,
};

static TEXT_SERVICE_VTBL: TextServiceVtbl = TextServiceVtbl {
    query_interface: text_service_query_interface,
    add_ref: text_service_add_ref,
    release: text_service_release,
    activate: text_service_activate,
    deactivate: text_service_deactivate,
    activate_ex: text_service_activate_ex,
};

static KEY_EVENT_SINK_VTBL: KeyEventSinkVtbl = KeyEventSinkVtbl {
    query_interface: key_sink_query_interface,
    add_ref: key_sink_add_ref,
    release: key_sink_release,
    on_set_focus,
    on_test_key_down,
    on_key_down,
    on_test_key_up,
    on_key_up,
    on_preserved_key,
};

/// Whether COM can unload this DLL.
#[must_use]
pub fn can_unload() -> bool {
    OBJECT_COUNT.load(Ordering::SeqCst) == 0 && LOCK_COUNT.load(Ordering::SeqCst) == 0
}

/// Returns an `IClassFactory` for the `NovaType` text service CLSID.
///
/// # Safety
///
/// `class_id`, `interface_id`, and `object` must be either valid COM pointers
/// from the caller or null. Null pointers are reported as HRESULT failures.
pub unsafe fn get_class_object(
    class_id: *const Guid,
    interface_id: *const Guid,
    object: *mut *mut c_void,
) -> i32 {
    crate::debug_log::log(&format!(
        "DllGetClassObject class_id={class_id:p} interface_id={interface_id:p}"
    ));
    if class_id.is_null() || interface_id.is_null() || object.is_null() {
        crate::debug_log::log("DllGetClassObject -> E_POINTER");
        return E_POINTER;
    }

    // SAFETY: pointers were checked for null above; COM caller owns validity.
    unsafe {
        *object = ptr::null_mut();
        if *class_id != CLSID_NOVATYPE_TEXT_SERVICE {
            crate::debug_log::log(&format!(
                "DllGetClassObject unexpected clsid={:?}",
                *class_id
            ));
            return CLASS_E_CLASSNOTAVAILABLE;
        }

        let hr = query_interface(
            ptr::addr_of!(CLASS_FACTORY).cast_mut().cast::<c_void>(),
            interface_id,
            object,
        );
        crate::debug_log::log(&format!("DllGetClassObject -> hr={hr:#010X}"));
        hr
    }
}

unsafe extern "system" fn query_interface(
    this: *mut c_void,
    interface_id: *const Guid,
    object: *mut *mut c_void,
) -> i32 {
    if interface_id.is_null() || object.is_null() {
        return E_POINTER;
    }

    // SAFETY: pointers are null-checked; COM caller owns validity.
    unsafe {
        *object = ptr::null_mut();
        let iid = *interface_id;
        if iid == IID_IUNKNOWN || iid == IID_ICLASS_FACTORY {
            *object = this;
            let _ = add_ref(this);
            return S_OK;
        }
    }

    E_NOINTERFACE
}

unsafe extern "system" fn add_ref(_this: *mut c_void) -> u32 {
    2
}

unsafe extern "system" fn release(_this: *mut c_void) -> u32 {
    1
}

unsafe extern "system" fn create_instance(
    _this: *mut c_void,
    outer: *mut c_void,
    interface_id: *const Guid,
    object: *mut *mut c_void,
) -> i32 {
    crate::debug_log::log(&format!(
        "ClassFactory::CreateInstance outer={outer:p} iid={interface_id:p}"
    ));
    if interface_id.is_null() || object.is_null() {
        crate::debug_log::log("CreateInstance -> E_POINTER");
        return E_POINTER;
    }
    if !outer.is_null() {
        crate::debug_log::log("CreateInstance -> CLASS_E_NOAGGREGATION");
        return CLASS_E_NOAGGREGATION;
    }

    unsafe {
        *object = ptr::null_mut();
    }

    let service = Box::new(TextService {
        processor_iface: TextProcessorInterface {
            vtbl: &raw const TEXT_SERVICE_VTBL,
        },
        key_sink_iface: KeyEventSinkInterface {
            vtbl: &raw const KEY_EVENT_SINK_VTBL,
        },
        ref_count: AtomicU32::new(1),
        thread_mgr: ptr::null_mut(),
        client_id: 0,
        activated: false,
        sinks: SinkState::default(),
        keyboard_hook: ptr::null_mut(),
        message_hook: ptr::null_mut(),
        test_key_down_pending: false,
        session: InputSession::new(DaemonClient::new()),
        document: TsfDocumentEditor::new(),
    });
    OBJECT_COUNT.fetch_add(1, Ordering::SeqCst);

    let raw = Box::into_raw(service).cast::<c_void>();
    let hr = unsafe { text_service_query_interface(raw, interface_id, object) };
    unsafe {
        let _ = text_service_release(raw);
    }
    crate::debug_log::log(&format!(
        "CreateInstance -> hr={hr:#010X} object={:p}",
        unsafe { *object }
    ));
    hr
}

unsafe extern "system" fn lock_server(_this: *mut c_void, lock: i32) -> i32 {
    if lock == 0 {
        LOCK_COUNT
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                Some(value.saturating_sub(1))
            })
            .ok();
    } else {
        LOCK_COUNT.fetch_add(1, Ordering::SeqCst);
    }
    S_OK
}

unsafe extern "system" fn text_service_query_interface(
    this: *mut c_void,
    interface_id: *const Guid,
    object: *mut *mut c_void,
) -> i32 {
    crate::debug_log::log(&format!(
        "TextService::QueryInterface this={this:p} iid={interface_id:p}"
    ));
    if interface_id.is_null() || object.is_null() || this.is_null() {
        return E_POINTER;
    }

    unsafe {
        *object = ptr::null_mut();
        let iid = *interface_id;
        crate::debug_log::log(&format!("TextService::QI requested iid={iid:?}"));
        if iid == IID_IUNKNOWN
            || iid == IID_ITF_TEXT_INPUT_PROCESSOR
            || iid == IID_ITF_TEXT_INPUT_PROCESSOR_EX
        {
            *object = this;
            let _ = text_service_add_ref(this);
            crate::debug_log::log(&format!("TextService::QI -> processor iid={iid:?}"));
            return S_OK;
        }
        if iid == IID_ITF_KEY_EVENT_SINK {
            let service = this.cast::<TextService>();
            *object = ptr::addr_of_mut!((*service).key_sink_iface).cast::<c_void>();
            let _ = text_service_add_ref(this);
            crate::debug_log::log("TextService::QI -> ITfKeyEventSink");
            return S_OK;
        }
    }

    unsafe {
        crate::debug_log::log(&format!(
            "TextService::QI -> E_NOINTERFACE iid={:?}",
            *interface_id
        ));
    }
    E_NOINTERFACE
}

unsafe extern "system" fn text_service_add_ref(this: *mut c_void) -> u32 {
    if this.is_null() {
        return 0;
    }

    let service = this.cast::<TextService>();
    unsafe { (*service).ref_count.fetch_add(1, Ordering::SeqCst) + 1 }
}

unsafe extern "system" fn text_service_release(this: *mut c_void) -> u32 {
    if this.is_null() {
        return 0;
    }

    let service = this.cast::<TextService>();
    let remaining = unsafe { (*service).ref_count.fetch_sub(1, Ordering::SeqCst) - 1 };
    if remaining == 0 {
        OBJECT_COUNT.fetch_sub(1, Ordering::SeqCst);
        unsafe {
            drop(Box::from_raw(service));
        }
    }
    remaining
}

unsafe extern "system" fn text_service_activate(
    this: *mut c_void,
    thread_mgr: *mut c_void,
    client_id: u32,
) -> i32 {
    if this.is_null() {
        return E_POINTER;
    }

    let service = this.cast::<TextService>();
    unsafe {
        (*service).thread_mgr = thread_mgr;
        (*service).client_id = client_id;
        (*service).activated = true;

        crate::debug_log::log(&format!(
            "Activate: thread_mgr={thread_mgr:p} client_id={client_id}"
        ));

        // Advise the key-event sink through ITfKeystrokeMgr on the thread manager.
        if let Ok(cookie) = advise_key_sink_from_thread_mgr(thread_mgr, service) {
            (*service).sinks.advise_key_event(cookie);
        }
        install_keyboard_hook(service);

        (*service).document.attach_context(TsfEditContext {
            thread_mgr,
            client_id,
            edit_cookie: 0,
            context: core::ptr::null_mut(),
        });
    }
    S_OK
}

unsafe extern "system" fn text_service_activate_ex(
    this: *mut c_void,
    thread_mgr: *mut c_void,
    client_id: u32,
    flags: u32,
) -> i32 {
    crate::debug_log::log(&format!(
        "ActivateEx: thread_mgr={thread_mgr:p} client_id={client_id} flags={flags:#X}"
    ));
    unsafe { text_service_activate(this, thread_mgr, client_id) }
}

unsafe extern "system" fn text_service_deactivate(this: *mut c_void) -> i32 {
    if this.is_null() {
        return E_POINTER;
    }

    let service = this.cast::<TextService>();
    unsafe {
        if let Some(cookie) = (*service).sinks.key_event_cookie() {
            unadvise_key_sink_from_thread_mgr((*service).thread_mgr, cookie);
            (*service).sinks.unadvise_key_event();
        }
        (*service).thread_mgr = ptr::null_mut();
        (*service).client_id = 0;
        (*service).activated = false;
        uninstall_keyboard_hook(service);
        (*service).document.detach_context();
    }
    S_OK
}

#[allow(clippy::cast_ptr_alignment)]
fn service_from_key_sink(this: *mut c_void) -> *mut TextService {
    let base = this.cast::<u8>();
    let offset = core::mem::offset_of!(TextService, key_sink_iface);
    base.wrapping_sub(offset).cast::<TextService>()
}

unsafe extern "system" fn key_sink_query_interface(
    this: *mut c_void,
    interface_id: *const Guid,
    object: *mut *mut c_void,
) -> i32 {
    if this.is_null() {
        return E_POINTER;
    }
    let service = service_from_key_sink(this).cast::<c_void>();
    unsafe { text_service_query_interface(service, interface_id, object) }
}

unsafe extern "system" fn key_sink_add_ref(this: *mut c_void) -> u32 {
    if this.is_null() {
        return 0;
    }
    let service = service_from_key_sink(this).cast::<c_void>();
    unsafe { text_service_add_ref(service) }
}

unsafe extern "system" fn key_sink_release(this: *mut c_void) -> u32 {
    if this.is_null() {
        return 0;
    }
    let service = service_from_key_sink(this).cast::<c_void>();
    unsafe { text_service_release(service) }
}

unsafe extern "system" fn on_set_focus(_this: *mut c_void, foreground: i32) -> i32 {
    crate::debug_log::log(&format!("OnSetFocus foreground={foreground}"));
    S_OK
}

unsafe extern "system" fn on_test_key_down(
    this: *mut c_void,
    context: *mut c_void,
    virtual_key: usize,
    flags: isize,
    eaten: *mut i32,
) -> i32 {
    if this.is_null() || eaten.is_null() {
        return E_POINTER;
    }
    let virtual_key = u32::try_from(virtual_key).unwrap_or(0);
    let service = service_from_key_sink(this);
    let eaten_value = unsafe {
        let eaten_now = process_virtual_key(service, context, virtual_key, "OnTestKeyDown");
        (*service).test_key_down_pending = eaten_now;
        *eaten = i32::from(eaten_now);
        *eaten
    };
    crate::debug_log::log(&format!(
        "OnTestKeyDown vk={virtual_key} flags={flags:#X} ctx={context:p} eaten={eaten_value}"
    ));
    S_OK
}

unsafe extern "system" fn on_key_down(
    this: *mut c_void,
    context: *mut c_void,
    virtual_key: usize,
    _flags: isize,
    eaten: *mut i32,
) -> i32 {
    if this.is_null() || eaten.is_null() {
        return E_POINTER;
    }

    let virtual_key = u32::try_from(virtual_key).unwrap_or(0);
    let service = service_from_key_sink(this);
    let should_eat = unsafe {
        if (*service).test_key_down_pending {
            (*service).test_key_down_pending = false;
            true
        } else {
            process_virtual_key(service, context, virtual_key, "OnKeyDown")
        }
    };
    unsafe {
        *eaten = i32::from(should_eat);
    }
    S_OK
}

unsafe fn process_virtual_key(
    service: *mut TextService,
    context: *mut c_void,
    virtual_key: u32,
    source: &str,
) -> bool {
    if service.is_null() {
        return false;
    }

    let should_eat = unsafe {
        key_event::test_key_down(
            (*service).activated,
            true,
            (*service).session.is_composing(),
            virtual_key,
        ) == key_event::KeyTestResult::Eat
    };
    if !should_eat {
        unsafe {
            crate::debug_log::log(&format!(
                "{source} vk={virtual_key} ctx={context:p} pass composing={} active={}",
                (*service).session.is_composing(),
                (*service).activated
            ));
        }
        return false;
    }

    unsafe {
        let mut resolved_context = context;
        let mut owns_resolved_context = false;
        if resolved_context.is_null()
            && let Some(focus_context) = focused_context_from_thread_mgr((*service).thread_mgr)
        {
            resolved_context = focus_context;
            owns_resolved_context = true;
        }

        (*service).document.attach_context(TsfEditContext {
            thread_mgr: (*service).thread_mgr,
            client_id: (*service).client_id,
            edit_cookie: 0,
            context: resolved_context,
        });
        let operations = key_event::key_down(&mut (*service).session, virtual_key);
        crate::debug_log::log(&format!(
            "{source} vk={virtual_key} ctx={context:p} resolved_ctx={resolved_context:p} ops={operations:?}"
        ));
        crate::edit_session::execute_operations(&mut (*service).document, &operations);
        if owns_resolved_context {
            com_release(resolved_context);
        }
    }
    true
}

unsafe extern "system" fn on_test_key_up(
    _this: *mut c_void,
    _context: *mut c_void,
    _virtual_key: usize,
    _flags: isize,
    eaten: *mut i32,
) -> i32 {
    set_not_eaten(eaten)
}

unsafe extern "system" fn on_key_up(
    _this: *mut c_void,
    _context: *mut c_void,
    _virtual_key: usize,
    _flags: isize,
    eaten: *mut i32,
) -> i32 {
    set_not_eaten(eaten)
}

unsafe extern "system" fn on_preserved_key(
    _this: *mut c_void,
    _context: *mut c_void,
    _key: *const c_void,
    eaten: *mut i32,
) -> i32 {
    set_not_eaten(eaten)
}

fn set_not_eaten(eaten: *mut i32) -> i32 {
    if eaten.is_null() {
        return E_POINTER;
    }
    unsafe {
        *eaten = 0;
    }
    S_OK
}

unsafe fn install_keyboard_hook(service: *mut TextService) {
    if service.is_null() {
        return;
    }
    unsafe {
        let thread_id = GetCurrentThreadId();
        if (*service).keyboard_hook.is_null() {
            let hook =
                SetWindowsHookExW(WH_KEYBOARD, keyboard_hook_proc, ptr::null_mut(), thread_id);
            (*service).keyboard_hook = hook;
            crate::debug_log::log(&format!(
                "keyboard_hook install thread_id={thread_id} hook={hook:p}"
            ));
        }
        if std::env::var_os("NOVATYPE_TSF_ENABLE_MESSAGE_HOOK").is_some()
            && (*service).message_hook.is_null()
        {
            let hook = SetWindowsHookExW(
                WH_GETMESSAGE,
                get_message_hook_proc,
                ptr::null_mut(),
                thread_id,
            );
            (*service).message_hook = hook;
            crate::debug_log::log(&format!(
                "message_hook install thread_id={thread_id} hook={hook:p}"
            ));
        } else {
            crate::debug_log::log(
                "message_hook disabled (set NOVATYPE_TSF_ENABLE_MESSAGE_HOOK=1 to enable debug fallback)",
            );
        }
        if !(*service).keyboard_hook.is_null() || !(*service).message_hook.is_null() {
            HOOK_SERVICE.store(service, Ordering::SeqCst);
        }
    }
}

unsafe fn uninstall_keyboard_hook(service: *mut TextService) {
    if service.is_null() {
        return;
    }
    unsafe {
        let hook = (*service).keyboard_hook;
        if !hook.is_null() {
            let result = UnhookWindowsHookEx(hook);
            crate::debug_log::log(&format!(
                "keyboard_hook uninstall hook={hook:p} result={result}"
            ));
            (*service).keyboard_hook = ptr::null_mut();
        }
        let hook = (*service).message_hook;
        if !hook.is_null() {
            let result = UnhookWindowsHookEx(hook);
            crate::debug_log::log(&format!(
                "message_hook uninstall hook={hook:p} result={result}"
            ));
            (*service).message_hook = ptr::null_mut();
        }
        if HOOK_SERVICE.load(Ordering::SeqCst) == service {
            HOOK_SERVICE.store(ptr::null_mut(), Ordering::SeqCst);
        }
    }
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: usize, lparam: isize) -> isize {
    if code == HC_ACTION {
        let is_key_up = (lparam & (1_isize << 31)) != 0;
        if !is_key_up {
            let service = HOOK_SERVICE.load(Ordering::SeqCst);
            let virtual_key = u32::try_from(wparam).unwrap_or(0);
            let eaten = unsafe {
                process_virtual_key(service, ptr::null_mut(), virtual_key, "KeyboardHook")
            };
            crate::debug_log::log(&format!(
                "KeyboardHook vk={virtual_key} lparam={lparam:#X} eaten={eaten}"
            ));
            if eaten {
                return 1;
            }
        }
    }
    unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn get_message_hook_proc(code: i32, wparam: usize, lparam: isize) -> isize {
    if code == HC_ACTION && lparam != 0 {
        let message = lparam as *mut HookMessage;
        unsafe {
            if (*message).message == WM_KEYDOWN || (*message).message == WM_SYSKEYDOWN {
                let service = HOOK_SERVICE.load(Ordering::SeqCst);
                let virtual_key = u32::try_from((*message).wparam).unwrap_or(0);
                let eaten =
                    process_virtual_key(service, ptr::null_mut(), virtual_key, "MessageHook");
                crate::debug_log::log(&format!(
                    "MessageHook vk={virtual_key} msg={:#X} remove={} eaten={eaten}",
                    (*message).message,
                    wparam
                ));
                if eaten {
                    (*message).message = WM_NULL;
                    (*message).wparam = 0;
                    (*message).lparam = 0;
                }
            }
        }
    }
    unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) }
}

#[repr(C)]
struct HookMessage {
    hwnd: *mut c_void,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    point: HookPoint,
}

#[repr(C)]
struct HookPoint {
    x: i32,
    y: i32,
}

/// Returns the top `ITfContext` for the thread manager's focused document.
///
/// The `ITfKeyEventSink` callback may pass a null `ITfContext` on some hosts;
/// in that case we can still obtain the active context through
/// `ITfThreadMgr::GetFocus` followed by `ITfDocumentMgr::GetTop`.
///
/// The returned context has a COM reference; the caller must release it.
unsafe fn focused_context_from_thread_mgr(thread_mgr: *mut c_void) -> Option<*mut c_void> {
    if thread_mgr.is_null() || thread_mgr as usize <= 4096 {
        return None;
    }

    unsafe {
        let mgr_vtbl: *const *const c_void = *(thread_mgr.cast::<*const *const c_void>());
        let get_focus: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 =
            core::mem::transmute(*mgr_vtbl.byte_add(7 * size_of::<*const c_void>()));
        let mut doc_mgr: *mut c_void = ptr::null_mut();
        let hr = get_focus(thread_mgr, &raw mut doc_mgr);
        if hr != S_OK || doc_mgr.is_null() {
            crate::debug_log::log(&format!(
                "focused_context: GetFocus failed hr={hr:#010X} doc_mgr={doc_mgr:p}"
            ));
            return None;
        }

        let doc_vtbl: *const *const c_void = *(doc_mgr.cast::<*const *const c_void>());
        // ITfDocumentMgr::GetTop is vtable slot 6 (IUnknown + CreateContext, Push, Pop).
        let get_top: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 =
            core::mem::transmute(*doc_vtbl.byte_add(6 * size_of::<*const c_void>()));
        let mut context: *mut c_void = ptr::null_mut();
        let hr = get_top(doc_mgr, &raw mut context);
        com_release(doc_mgr);
        if hr != S_OK || context.is_null() {
            crate::debug_log::log(&format!(
                "focused_context: GetTop failed hr={hr:#010X} context={context:p}"
            ));
            return None;
        }
        crate::debug_log::log(&format!("focused_context: context={context:p}"));
        Some(context)
    }
}

/// Advises the `NovaType` key-event sink through `ITfKeystrokeMgr` on the
/// thread manager.
///
/// # Safety
///
/// `thread_mgr` must be a valid `ITfThreadMgr` pointer or null. When null or
/// when any intermediate COM call fails, an `Err(())` is returned and the
/// caller continues with activation (key events simply won't be intercepted).
///
/// The advise path:
/// 1. QI the thread manager for `ITfKeystrokeMgr`.
/// 2. Call `AdviseKeyEventSink(client_id, sink, TRUE)`.
///
/// The returned cookie is the `TfClientId`, which `UnadviseKeyEventSink` takes.
unsafe fn advise_key_sink_from_thread_mgr(
    thread_mgr: *mut c_void,
    service: *mut TextService,
) -> Result<SinkCookie, ()> {
    if thread_mgr.is_null()
        || service.is_null()
        // Guard against sentinel pointers used by tests (e.g. ptr::dangling_mut).
        || thread_mgr as usize <= 4096
    {
        return Err(());
    }

    // SAFETY: caller guarantees `thread_mgr` is a valid ITfThreadMgr pointer.
    unsafe {
        let mgr_vtbl: *const *const c_void = *(thread_mgr.cast::<*const *const c_void>());
        let qi: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32 =
            core::mem::transmute(*mgr_vtbl);
        let mut keystroke_mgr: *mut c_void = ptr::null_mut();
        let hr = qi(thread_mgr, &IID_ITF_KEYSTROKE_MGR, &raw mut keystroke_mgr);
        if hr != S_OK || keystroke_mgr.is_null() {
            crate::debug_log::log(&format!("advise: QI ITfKeystrokeMgr failed hr={hr:#010X}"));
            return Err(());
        }

        // ITfKeystrokeMgr vtable:
        //   [0-2] IUnknown
        //   [3] AdviseKeyEventSink(TfClientId, ITfKeyEventSink*, BOOL fForeground)
        //   [4] UnadviseKeyEventSink(TfClientId)
        let ks_vtbl: *const *const c_void = *(keystroke_mgr.cast::<*const *const c_void>());
        let advise: unsafe extern "system" fn(*mut c_void, u32, *mut c_void, i32) -> i32 =
            core::mem::transmute(*ks_vtbl.byte_add(3 * size_of::<*const c_void>()));

        let sink_ptr = ptr::addr_of!((*service).key_sink_iface)
            .cast_mut()
            .cast::<c_void>();
        let client_id = (*service).client_id;
        let hr = advise(keystroke_mgr, client_id, sink_ptr, 1);
        com_release(keystroke_mgr);
        if hr != S_OK {
            crate::debug_log::log(&format!("advise: AdviseKeyEventSink failed hr={hr:#010X}"));
            return Err(());
        }
        crate::debug_log::log(&format!("advise: key sink advised tid={client_id}"));
        Ok(client_id)
    }
}

/// Unadvises a previously advised key-event sink.
///
/// # Safety
///
/// `thread_mgr` must be the same `ITfThreadMgr` pointer used during
/// `advise_key_sink_from_thread_mgr`, or null. `cookie` is the `TfClientId`
/// returned by the advise call.
unsafe fn unadvise_key_sink_from_thread_mgr(thread_mgr: *mut c_void, cookie: SinkCookie) {
    if thread_mgr.is_null() || cookie == 0 || thread_mgr as usize <= 4096 {
        return;
    }

    unsafe {
        let mgr_vtbl: *const *const c_void = *(thread_mgr.cast::<*const *const c_void>());
        let qi: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32 =
            core::mem::transmute(*mgr_vtbl);
        let mut keystroke_mgr: *mut c_void = ptr::null_mut();
        if qi(thread_mgr, &IID_ITF_KEYSTROKE_MGR, &raw mut keystroke_mgr) != S_OK
            || keystroke_mgr.is_null()
        {
            return;
        }

        // ITfKeystrokeMgr::UnadviseKeyEventSink at vtable slot 4.
        let ks_vtbl: *const *const c_void = *(keystroke_mgr.cast::<*const *const c_void>());
        let unadvise: unsafe extern "system" fn(*mut c_void, u32) -> i32 =
            core::mem::transmute(*ks_vtbl.byte_add(4 * size_of::<*const c_void>()));
        let _ = unadvise(keystroke_mgr, cookie);
        com_release(keystroke_mgr);
    }
}

#[cfg(test)]
impl TextService {
    fn on_key_down(&mut self, vk: u32) -> Outcome {
        match self.on_key_down_operations(vk).as_slice() {
            [edit_session::EditOperation::PassThrough] => Outcome::PassThrough,
            [edit_session::EditOperation::CommitText(text), ..] => Outcome::Commit(text.clone()),
            [
                edit_session::EditOperation::ClearComposition,
                edit_session::EditOperation::HideCandidates,
            ] => Outcome::Dismissed,
            _ => Outcome::Updated,
        }
    }

    fn on_key_down_operations(&mut self, vk: u32) -> Vec<edit_session::EditOperation> {
        if !self.activated {
            return edit_session::plan_operations(crate::SessionAction::PassThrough);
        }
        key_event::key_down(&mut self.session, vk)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, CLSID_NOVATYPE_TEXT_SERVICE,
        E_NOINTERFACE, Guid, IID_ICLASS_FACTORY, IID_ITF_KEY_EVENT_SINK,
        IID_ITF_TEXT_INPUT_PROCESSOR, IID_IUNKNOWN, S_OK, get_class_object,
    };
    use crate::{
        Outcome,
        edit_session::{DocumentEditor, EditOperation, execute_operations},
    };
    use core::ffi::c_void;
    use core::ptr;

    const UNKNOWN_IID: Guid = Guid {
        data1: 0x1111_1111,
        data2: 0x2222,
        data3: 0x3333,
        data4: [0x44; 8],
    };

    #[test]
    fn returns_class_factory_for_iunknown() {
        let mut object: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            get_class_object(&CLSID_NOVATYPE_TEXT_SERVICE, &IID_IUNKNOWN, &raw mut object)
        };

        assert_eq!(hr, S_OK);
        assert!(!object.is_null());
    }

    #[test]
    fn returns_class_factory_for_iclass_factory() {
        let mut object: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            get_class_object(
                &CLSID_NOVATYPE_TEXT_SERVICE,
                &IID_ICLASS_FACTORY,
                &raw mut object,
            )
        };

        assert_eq!(hr, S_OK);
        assert!(!object.is_null());
    }

    #[test]
    fn rejects_unknown_clsid() {
        let mut object: *mut c_void = ptr::null_mut();
        let hr = unsafe { get_class_object(&UNKNOWN_IID, &IID_ICLASS_FACTORY, &raw mut object) };

        assert_eq!(hr, CLASS_E_CLASSNOTAVAILABLE);
        assert!(object.is_null());
    }

    #[test]
    fn rejects_unknown_interface() {
        let mut object: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            get_class_object(&CLSID_NOVATYPE_TEXT_SERVICE, &UNKNOWN_IID, &raw mut object)
        };

        assert_eq!(hr, E_NOINTERFACE);
        assert!(object.is_null());
    }

    #[test]
    fn class_factory_creates_text_processor() {
        let mut factory: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            get_class_object(
                &CLSID_NOVATYPE_TEXT_SERVICE,
                &IID_ICLASS_FACTORY,
                &raw mut factory,
            )
        };
        assert_eq!(hr, S_OK);

        let factory = factory.cast::<super::ClassFactory>();
        let mut service: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            ((*(*factory).vtbl).create_instance)(
                factory.cast::<c_void>(),
                ptr::null_mut(),
                &IID_ITF_TEXT_INPUT_PROCESSOR,
                &raw mut service,
            )
        };

        assert_eq!(hr, S_OK);
        assert!(!service.is_null());

        let service = service.cast::<super::TextService>();
        let activate_hr = unsafe {
            ((*(*service).processor_iface.vtbl).activate)(
                service.cast::<c_void>(),
                ptr::null_mut(),
                0,
            )
        };
        let deactivate_hr =
            unsafe { ((*(*service).processor_iface.vtbl).deactivate)(service.cast::<c_void>()) };
        let remaining =
            unsafe { ((*(*service).processor_iface.vtbl).release)(service.cast::<c_void>()) };

        assert_eq!(activate_hr, S_OK);
        assert_eq!(deactivate_hr, S_OK);
        assert_eq!(remaining, 0);
    }

    #[test]
    fn text_processor_queries_key_event_sink() {
        let mut factory: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            get_class_object(
                &CLSID_NOVATYPE_TEXT_SERVICE,
                &IID_ICLASS_FACTORY,
                &raw mut factory,
            )
        };
        assert_eq!(hr, S_OK);

        let factory = factory.cast::<super::ClassFactory>();
        let mut service: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            ((*(*factory).vtbl).create_instance)(
                factory.cast::<c_void>(),
                ptr::null_mut(),
                &IID_ITF_TEXT_INPUT_PROCESSOR,
                &raw mut service,
            )
        };
        assert_eq!(hr, S_OK);

        let mut sink: *mut c_void = ptr::null_mut();
        let service = service.cast::<super::TextService>();
        let hr = unsafe {
            ((*(*service).processor_iface.vtbl).query_interface)(
                service.cast::<c_void>(),
                &IID_ITF_KEY_EVENT_SINK,
                &raw mut sink,
            )
        };

        assert_eq!(hr, S_OK);
        assert!(!sink.is_null());
        let remaining =
            unsafe { ((*(*service).processor_iface.vtbl).release)(service.cast::<c_void>()) };
        assert_eq!(remaining, 1);
        let remaining =
            unsafe { ((*(*service).processor_iface.vtbl).release)(service.cast::<c_void>()) };
        assert_eq!(remaining, 0);
    }

    #[test]
    fn key_event_sink_tests_and_handles_key_down() {
        let mut service = Box::new(super::TextService {
            processor_iface: super::TextProcessorInterface {
                vtbl: &raw const super::TEXT_SERVICE_VTBL,
            },
            key_sink_iface: super::KeyEventSinkInterface {
                vtbl: &raw const super::KEY_EVENT_SINK_VTBL,
            },
            ref_count: std::sync::atomic::AtomicU32::new(1),
            thread_mgr: ptr::null_mut(),
            client_id: 0,
            activated: false,
            sinks: crate::sink::SinkState::default(),
            keyboard_hook: ptr::null_mut(),
            message_hook: ptr::null_mut(),
            test_key_down_pending: false,
            session: crate::InputSession::new(crate::DaemonClient::new()),
            document: crate::tsf_document::TsfDocumentEditor::new(),
        });
        let raw = (&raw mut *service).cast::<c_void>();
        let _ = unsafe { super::text_service_activate(raw, ptr::dangling_mut(), 9) };
        let sink = (&raw mut service.key_sink_iface).cast::<c_void>();

        // test_key_down should eat 'n' because we are activated.
        // (The sentinel thread_mgr prevents real ITfSource advise, but key
        // events still route through because on_key_down checks `activated`.)
        let mut eaten = 0;
        let hr = unsafe {
            ((*service.key_sink_iface.vtbl).on_test_key_down)(
                sink,
                ptr::null_mut(),
                0x4E,
                0,
                &raw mut eaten,
            )
        };
        assert_eq!(hr, S_OK);
        assert_eq!(eaten, 1);

        eaten = 0;
        let hr = unsafe {
            ((*service.key_sink_iface.vtbl).on_key_down)(
                sink,
                ptr::null_mut(),
                0x4E,
                0,
                &raw mut eaten,
            )
        };
        assert_eq!(hr, S_OK);
        assert_eq!(eaten, 1);
        // Key events work when activated even without a real ITfSource sink.
        assert_eq!(service.document.composition(), "n");
    }

    #[test]
    fn class_factory_rejects_aggregation() {
        let mut factory: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            get_class_object(
                &CLSID_NOVATYPE_TEXT_SERVICE,
                &IID_ICLASS_FACTORY,
                &raw mut factory,
            )
        };
        assert_eq!(hr, S_OK);

        let factory = factory.cast::<super::ClassFactory>();
        let mut service: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            ((*(*factory).vtbl).create_instance)(
                factory.cast::<c_void>(),
                factory.cast::<c_void>(),
                &IID_ITF_TEXT_INPUT_PROCESSOR,
                &raw mut service,
            )
        };

        assert_eq!(hr, CLASS_E_NOAGGREGATION);
        assert!(service.is_null());
    }

    #[test]
    fn text_service_activation_tracks_state() {
        let mut service = Box::new(super::TextService {
            processor_iface: super::TextProcessorInterface {
                vtbl: &raw const super::TEXT_SERVICE_VTBL,
            },
            key_sink_iface: super::KeyEventSinkInterface {
                vtbl: &raw const super::KEY_EVENT_SINK_VTBL,
            },
            ref_count: std::sync::atomic::AtomicU32::new(1),
            thread_mgr: ptr::null_mut(),
            client_id: 0,
            activated: false,
            sinks: crate::sink::SinkState::default(),
            keyboard_hook: ptr::null_mut(),
            message_hook: ptr::null_mut(),
            test_key_down_pending: false,
            session: crate::InputSession::new(crate::DaemonClient::new()),
            document: crate::tsf_document::TsfDocumentEditor::new(),
        });
        let raw = (&raw mut *service).cast::<c_void>();

        assert_eq!(service.on_key_down(0x4E), Outcome::PassThrough);
        // Note: text_service_activate uses a test sentinel (ptr::dangling_mut)
        // that cannot be dereferenced as an ITfThreadMgr. The advise path
        // correctly skips it, so no key-event sink cookie gets recorded.
        // With the local advisor this test previously asserted Some(1), but
        // the real ITfSource path requires a live thread manager.
        let activate_hr = unsafe { super::text_service_activate(raw, ptr::dangling_mut(), 42) };
        assert_eq!(activate_hr, S_OK);
        assert!(service.activated);
        assert_eq!(service.client_id, 42);
        assert!(service.document.context().is_some());
        // Key events work when activated even without a real ITfSource.
        assert_eq!(service.on_key_down(0x4E), Outcome::Updated);

        let deactivate_hr = unsafe { super::text_service_deactivate(raw) };
        assert_eq!(deactivate_hr, S_OK);
        assert!(!service.activated);
        assert_eq!(service.client_id, 0);
        assert!(!service.sinks.key_event_active());
        assert!(service.document.context().is_none());
    }

    #[test]
    fn text_service_key_path_produces_edit_operations() {
        let mut service = Box::new(super::TextService {
            processor_iface: super::TextProcessorInterface {
                vtbl: &raw const super::TEXT_SERVICE_VTBL,
            },
            key_sink_iface: super::KeyEventSinkInterface {
                vtbl: &raw const super::KEY_EVENT_SINK_VTBL,
            },
            ref_count: std::sync::atomic::AtomicU32::new(1),
            thread_mgr: ptr::null_mut(),
            client_id: 0,
            activated: false,
            sinks: crate::sink::SinkState::default(),
            keyboard_hook: ptr::null_mut(),
            message_hook: ptr::null_mut(),
            test_key_down_pending: false,
            session: crate::InputSession::new(crate::DaemonClient::new()),
            document: crate::tsf_document::TsfDocumentEditor::new(),
        });
        let raw = (&raw mut *service).cast::<c_void>();
        let _ = unsafe { super::text_service_activate(raw, ptr::dangling_mut(), 7) };

        let operations = service.on_key_down_operations(0x4E); // N

        assert!(matches!(operations[0], EditOperation::SetComposition(_)));
        assert!(matches!(operations[1], EditOperation::ShowCandidates(_)));
    }

    #[derive(Default)]
    struct FakeDocument {
        composition: String,
        committed: String,
        candidates_visible: bool,
    }

    impl DocumentEditor for FakeDocument {
        fn pass_through(&mut self) {}

        fn set_composition(&mut self, text: &str) {
            self.composition = text.to_string();
        }

        fn commit_text(&mut self, text: &str) {
            self.committed.push_str(text);
        }

        fn show_candidates(&mut self, _model: &crate::edit_session::CandidateWindowModel) {
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
    fn text_service_operations_execute_against_document_adapter() {
        let mut service = Box::new(super::TextService {
            processor_iface: super::TextProcessorInterface {
                vtbl: &raw const super::TEXT_SERVICE_VTBL,
            },
            key_sink_iface: super::KeyEventSinkInterface {
                vtbl: &raw const super::KEY_EVENT_SINK_VTBL,
            },
            ref_count: std::sync::atomic::AtomicU32::new(1),
            thread_mgr: ptr::null_mut(),
            client_id: 0,
            activated: false,
            sinks: crate::sink::SinkState::default(),
            keyboard_hook: ptr::null_mut(),
            message_hook: ptr::null_mut(),
            test_key_down_pending: false,
            session: crate::InputSession::new(crate::DaemonClient::new()),
            document: crate::tsf_document::TsfDocumentEditor::new(),
        });
        let raw = (&raw mut *service).cast::<c_void>();
        let _ = unsafe { super::text_service_activate(raw, ptr::dangling_mut(), 7) };

        let mut document = FakeDocument::default();
        let operations = service.on_key_down_operations(0x4E); // N
        execute_operations(&mut document, &operations);

        assert_eq!(document.composition, "n");
        assert!(document.candidates_visible);
    }

    #[test]
    fn text_service_can_execute_operations_to_internal_document() {
        let mut service = Box::new(super::TextService {
            processor_iface: super::TextProcessorInterface {
                vtbl: &raw const super::TEXT_SERVICE_VTBL,
            },
            key_sink_iface: super::KeyEventSinkInterface {
                vtbl: &raw const super::KEY_EVENT_SINK_VTBL,
            },
            ref_count: std::sync::atomic::AtomicU32::new(1),
            thread_mgr: ptr::null_mut(),
            client_id: 0,
            activated: false,
            sinks: crate::sink::SinkState::default(),
            keyboard_hook: ptr::null_mut(),
            message_hook: ptr::null_mut(),
            test_key_down_pending: false,
            session: crate::InputSession::new(crate::DaemonClient::new()),
            document: crate::tsf_document::TsfDocumentEditor::new(),
        });
        let raw = (&raw mut *service).cast::<c_void>();
        let _ = unsafe { super::text_service_activate(raw, ptr::dangling_mut(), 9) };

        let operations = service.on_key_down_operations(0x4E); // N
        execute_operations(&mut service.document, &operations);

        assert_eq!(service.document.composition(), "n");
        assert!(service.document.candidate_model().is_some());
    }
}
