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
    vtbl: *const TextServiceVtbl,
    ref_count: AtomicU32,
}

#[repr(C)]
struct TextServiceVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    activate: unsafe extern "system" fn(*mut c_void, *mut c_void, u32) -> i32,
    deactivate: unsafe extern "system" fn(*mut c_void) -> i32,
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
        vtbl: &raw const TEXT_SERVICE_VTBL,
        ref_count: AtomicU32::new(1),
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
    _this: *mut c_void,
    _thread_mgr: *mut c_void,
    _client_id: u32,
) -> i32 {
    S_OK
}

unsafe extern "system" fn text_service_deactivate(_this: *mut c_void) -> i32 {
    S_OK
}

#[cfg(test)]
mod tests {
    use super::{
        CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, CLSID_NOVATYPE_TEXT_SERVICE,
        E_NOINTERFACE, Guid, IID_ICLASS_FACTORY, IID_ITF_TEXT_INPUT_PROCESSOR, IID_IUNKNOWN, S_OK,
        can_unload, get_class_object,
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
        let activate_hr =
            unsafe { ((*(*service).vtbl).activate)(service.cast::<c_void>(), ptr::null_mut(), 0) };
        let deactivate_hr = unsafe { ((*(*service).vtbl).deactivate)(service.cast::<c_void>()) };
        let remaining = unsafe { ((*(*service).vtbl).release)(service.cast::<c_void>()) };

        assert_eq!(activate_hr, S_OK);
        assert_eq!(deactivate_hr, S_OK);
        assert_eq!(remaining, 0);
        assert!(can_unload());
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
}
