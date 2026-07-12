use crate::{
    DaemonClient, InputSession, key_event,
    sink::SinkState,
    tsf_document::{TsfDocumentEditor, TsfEditContext},
};

#[cfg(test)]
use crate::{Outcome, edit_session};
use core::ffi::c_void;
use core::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
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

static OBJECT_COUNT: AtomicU32 = AtomicU32::new(0);
static LOCK_COUNT: AtomicU32 = AtomicU32::new(0);

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
    data1: 0xAA80_E801,
    data2: 0x2021,
    data3: 0x11D2,
    data4: [0x93, 0xE0, 0x00, 0x60, 0xB0, 0x67, 0xB8, 0x6E],
};

const IID_ITF_KEY_EVENT_SINK: Guid = Guid {
    data1: 0xAA80_E7F5,
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

#[repr(C)]
struct TextService {
    processor_iface: TextProcessorInterface,
    key_sink_iface: KeyEventSinkInterface,
    ref_count: AtomicU32,
    thread_mgr: *mut c_void,
    client_id: u32,
    activated: bool,
    sinks: SinkState,
    session: InputSession<DaemonClient>,
    document: TsfDocumentEditor,
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
}

#[repr(C)]
struct KeyEventSinkVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    on_set_focus: unsafe extern "system" fn(*mut c_void, i32) -> i32,
    on_test_key_down:
        unsafe extern "system" fn(*mut c_void, *mut c_void, u32, i32, *mut i32) -> i32,
    on_key_down: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, i32, *mut i32) -> i32,
    on_test_key_up: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, i32, *mut i32) -> i32,
    on_key_up: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, i32, *mut i32) -> i32,
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
    if class_id.is_null() || interface_id.is_null() || object.is_null() {
        return E_POINTER;
    }

    // SAFETY: pointers were checked for null above; COM caller owns validity.
    unsafe {
        *object = ptr::null_mut();
        if *class_id != CLSID_NOVATYPE_TEXT_SERVICE {
            return CLASS_E_CLASSNOTAVAILABLE;
        }

        query_interface(
            ptr::addr_of!(CLASS_FACTORY).cast_mut().cast::<c_void>(),
            interface_id,
            object,
        )
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
    if interface_id.is_null() || object.is_null() {
        return E_POINTER;
    }
    if !outer.is_null() {
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
        session: InputSession::new(DaemonClient::new()),
        document: TsfDocumentEditor::new(),
    });
    OBJECT_COUNT.fetch_add(1, Ordering::SeqCst);

    let raw = Box::into_raw(service).cast::<c_void>();
    let hr = unsafe { text_service_query_interface(raw, interface_id, object) };
    unsafe {
        let _ = text_service_release(raw);
    }
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
    if interface_id.is_null() || object.is_null() || this.is_null() {
        return E_POINTER;
    }

    unsafe {
        *object = ptr::null_mut();
        let iid = *interface_id;
        if iid == IID_IUNKNOWN || iid == IID_ITF_TEXT_INPUT_PROCESSOR {
            *object = this;
            let _ = text_service_add_ref(this);
            return S_OK;
        }
        if iid == IID_ITF_KEY_EVENT_SINK {
            let service = this.cast::<TextService>();
            *object = ptr::addr_of_mut!((*service).key_sink_iface).cast::<c_void>();
            let _ = text_service_add_ref(this);
            return S_OK;
        }
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
        (*service).sinks.advise_key_event(1);
        (*service).document.attach_context(TsfEditContext {
            thread_mgr,
            client_id,
            edit_cookie: 0,
        });
    }
    S_OK
}

unsafe extern "system" fn text_service_deactivate(this: *mut c_void) -> i32 {
    if this.is_null() {
        return E_POINTER;
    }

    let service = this.cast::<TextService>();
    unsafe {
        (*service).thread_mgr = ptr::null_mut();
        (*service).client_id = 0;
        (*service).activated = false;
        (*service).sinks.unadvise_key_event();
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

unsafe extern "system" fn on_set_focus(_this: *mut c_void, _foreground: i32) -> i32 {
    S_OK
}

unsafe extern "system" fn on_test_key_down(
    this: *mut c_void,
    _context: *mut c_void,
    virtual_key: u32,
    _flags: i32,
    eaten: *mut i32,
) -> i32 {
    unsafe { set_eaten(this, virtual_key, eaten) }
}

unsafe extern "system" fn on_key_down(
    this: *mut c_void,
    _context: *mut c_void,
    virtual_key: u32,
    _flags: i32,
    eaten: *mut i32,
) -> i32 {
    if this.is_null() || eaten.is_null() {
        return E_POINTER;
    }

    let service = service_from_key_sink(this);
    let should_eat = unsafe {
        key_event::test_key_down(
            (*service).activated,
            (*service).sinks.key_event_active(),
            (*service).session.is_composing(),
            virtual_key,
        ) == key_event::KeyTestResult::Eat
    };
    unsafe {
        *eaten = i32::from(should_eat);
        if should_eat {
            let operations = key_event::key_down(&mut (*service).session, virtual_key);
            crate::edit_session::execute_operations(&mut (*service).document, &operations);
        }
    }
    S_OK
}

unsafe extern "system" fn on_test_key_up(
    _this: *mut c_void,
    _context: *mut c_void,
    _virtual_key: u32,
    _flags: i32,
    eaten: *mut i32,
) -> i32 {
    set_not_eaten(eaten)
}

unsafe extern "system" fn on_key_up(
    _this: *mut c_void,
    _context: *mut c_void,
    _virtual_key: u32,
    _flags: i32,
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

unsafe fn set_eaten(this: *mut c_void, virtual_key: u32, eaten: *mut i32) -> i32 {
    if this.is_null() || eaten.is_null() {
        return E_POINTER;
    }

    let service = service_from_key_sink(this);
    let should_eat = unsafe {
        key_event::test_key_down(
            (*service).activated,
            (*service).sinks.key_event_active(),
            (*service).session.is_composing(),
            virtual_key,
        ) == key_event::KeyTestResult::Eat
    };
    unsafe {
        *eaten = i32::from(should_eat);
    }
    S_OK
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
        if !self.activated || !self.sinks.key_event_active() {
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
        IID_ITF_TEXT_INPUT_PROCESSOR, IID_IUNKNOWN, S_OK, can_unload, get_class_object,
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
        assert!(!can_unload());

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
        assert!(can_unload());
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
            session: crate::InputSession::new(crate::DaemonClient::new()),
            document: crate::tsf_document::TsfDocumentEditor::new(),
        });
        let raw = (&raw mut *service).cast::<c_void>();
        let _ = unsafe { super::text_service_activate(raw, ptr::dangling_mut(), 9) };
        let sink = (&raw mut service.key_sink_iface).cast::<c_void>();

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
            session: crate::InputSession::new(crate::DaemonClient::new()),
            document: crate::tsf_document::TsfDocumentEditor::new(),
        });
        let raw = (&raw mut *service).cast::<c_void>();

        assert_eq!(service.on_key_down(0x4E), Outcome::PassThrough);
        let activate_hr = unsafe { super::text_service_activate(raw, ptr::dangling_mut(), 42) };
        assert_eq!(activate_hr, S_OK);
        assert!(service.activated);
        assert_eq!(service.client_id, 42);
        assert_eq!(service.sinks.key_event_cookie(), Some(1));
        assert!(service.document.context().is_some());
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
