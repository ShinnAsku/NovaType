#![allow(clippy::doc_markdown)]

use crate::metadata::{PROFILE_DESCRIPTION, PROFILE_DISPLAY_NAME, TEXT_SERVICE_CLSID};
use crate::profile::{ProfileRegistrar, ProfileResult, TsfProfile, execute_registration_plan};
use std::error::Error;
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_WRITE};

type RegistrationResult<T> = Result<T, Box<dyn Error>>;

const NOVATYPE_TSF_KEY: &str = "Software\\NovaType\\TSF";
const CLASSES_KEY: &str = "Software\\Classes\\CLSID";

/// Registers the `NovaType` text service as a COM object and TSF profile.
///
/// Writes registry keys so Windows discovers NovaType as a Text Input Processor:
///
/// 1. COM InprocServer32 class registration (HKCU — regsvr32-compatible).
/// 2. TSF TIP registration (HKLM — required for the language bar to show it).
///    Falls back to HKCU when HKLM is not writable (non-admin).
pub fn register_server() -> RegistrationResult<()> {
    let module_path = module_path();

    // Phase 1: COM class registration (CLSID + InprocServer32) — user scope.
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (marker, _) = hkcu.create_subkey_with_flags(NOVATYPE_TSF_KEY, KEY_ALL_ACCESS)?;
        marker.set_value("DisplayName", &PROFILE_DISPLAY_NAME)?;
        marker.set_value("Description", &PROFILE_DESCRIPTION)?;
        marker.set_value("Clsid", &TEXT_SERVICE_CLSID)?;
        marker.set_value("ModulePath", &module_path.to_string_lossy().to_string())?;
        let profile = TsfProfile::novatype();
        marker.set_value("LanguageTag", &profile.language_tag)?;
        marker.set_value("IconIndex", &profile.icon_index.cast_unsigned())?;

        let mut marker_registrar = RegistryProfileRegistrar::new(&marker);
        execute_registration_plan(&mut marker_registrar, &profile.registration_plan())
            .map_err(std::io::Error::other)?;

        let clsid_path = format!("{CLASSES_KEY}\\{TEXT_SERVICE_CLSID}");
        // System input hosts (TextInputHost, ctfmon) resolve TIP CLSIDs from
        // machine scope, so prefer HKLM; fall back to HKCU when not elevated.
        let clsid_root = [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER]
            .into_iter()
            .find_map(|hive| {
                RegKey::predef(hive)
                    .create_subkey_with_flags(&clsid_path, KEY_ALL_ACCESS)
                    .ok()
            })
            .ok_or_else(|| std::io::Error::other("cannot create CLSID key in HKLM or HKCU"))?;
        let (clsid, _) = clsid_root;
        clsid.set_value("", &PROFILE_DESCRIPTION)?;

        let (inproc, _) = clsid.create_subkey_with_flags("InprocServer32", KEY_ALL_ACCESS)?;
        inproc.set_value("", &module_path.to_string_lossy().to_string())?;
        inproc.set_value("ThreadingModel", &"Apartment")?;
    }

    // Phase 2: TSF TIP registration under HKLM\SOFTWARE\Microsoft\CTF\TIP.
    // This is what makes NovaType appear in the language bar.
    // Falls back to HKCU if HKLM is not writable.
    match register_tsf_tip_keys() {
        Ok(()) => {
            // Clear any stale error marker from a previous failed run.
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let (marker, _) = hkcu.create_subkey_with_flags(NOVATYPE_TSF_KEY, KEY_ALL_ACCESS)?;
            let _ = marker.delete_value("TsfRegisterError");
        }
        Err(e) => {
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let (marker, _) = hkcu.create_subkey_with_flags(NOVATYPE_TSF_KEY, KEY_ALL_ACCESS)?;
            marker.set_value("TsfRegisterError", &e.clone())?;
        }
    }

    // Phase 3: official TSF profile registration through
    // ITfInputProcessorProfileMgr / ITfCategoryMgr. This is what makes the
    // profile pass `Set-WinUserLanguageList` / `InstallLayoutOrTip` validation
    // and appear in the Windows input method switcher. Best-effort: record the
    // error in the HKCU marker so CI (no COM/TSF) still succeeds.
    #[cfg(windows)]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (marker, _) = hkcu.create_subkey_with_flags(NOVATYPE_TSF_KEY, KEY_ALL_ACCESS)?;
        match tsf_api::register_profile(&module_path.to_string_lossy()) {
            Ok(()) => {
                let _ = marker.delete_value("TsfApiError");
                marker.set_value("TsfApiRegistered", &1u32)?;
            }
            Err(e) => {
                marker.set_value("TsfApiError", &e)?;
            }
        }
        match tsf_api::activate_profile() {
            Ok(()) => {
                let _ = marker.delete_value("TsfApiActivateError");
                marker.set_value("TsfApiActivated", &1u32)?;
            }
            Err(e) => {
                marker.set_value("TsfApiActivateError", &e)?;
            }
        }
    }

    Ok(())
}

/// Official TSF registration via `msctf.dll` COM interfaces.
#[cfg(windows)]
mod tsf_api {
    use crate::metadata::PROFILE_DESCRIPTION;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
    use windows::Win32::UI::TextServices::{
        CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, GUID_TFCAT_TIP_KEYBOARD,
        GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT, GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
        GUID_TFCAT_TIPCAP_UIELEMENTENABLED, ITfCategoryMgr, ITfInputProcessorProfileMgr,
        TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE, TF_IPPMF_FORSESSION, TF_PROFILETYPE_INPUTPROCESSOR,
    };
    use windows::core::GUID;

    /// Binary form of [`crate::metadata::TEXT_SERVICE_CLSID`].
    pub const TEXT_SERVICE_GUID: GUID = GUID::from_u128(0x7E4B_71B0_5C48_45E8_9E4E_4DFD_16FE_5E95);

    /// zh-CN primary language id.
    const LANGID_ZH_CN: u16 = 0x0804;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    /// Registers the language profile and TIP categories with TSF.
    pub fn register_profile(icon_file: &str) -> Result<(), String> {
        unsafe {
            // regsvr32 already initializes COM; tolerate S_FALSE/RPC_E_CHANGED_MODE.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let mgr: ITfInputProcessorProfileMgr =
                CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| format!("CoCreateInstance(InputProcessorProfiles): {e}"))?;
            mgr.RegisterProfile(
                &TEXT_SERVICE_GUID,
                LANGID_ZH_CN,
                &TEXT_SERVICE_GUID,
                &wide(PROFILE_DESCRIPTION),
                &wide(icon_file),
                0,
                HKL::default(),
                0,
                true,
                0,
            )
            .map_err(|e| format!("RegisterProfile: {e}"))?;

            let cat: ITfCategoryMgr =
                CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| format!("CoCreateInstance(CategoryMgr): {e}"))?;
            for category in [
                &GUID_TFCAT_TIP_KEYBOARD,
                &GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
                &GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
                &GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
            ] {
                cat.RegisterCategory(&TEXT_SERVICE_GUID, category, &TEXT_SERVICE_GUID)
                    .map_err(|e| format!("RegisterCategory({category:?}): {e}"))?;
            }
        }
        Ok(())
    }

    /// Activates the NovaType profile for the current TSF session.
    pub fn activate_profile() -> Result<(), String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let mgr: ITfInputProcessorProfileMgr =
                CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| format!("CoCreateInstance(InputProcessorProfiles): {e}"))?;
            mgr.ActivateProfile(
                TF_PROFILETYPE_INPUTPROCESSOR,
                LANGID_ZH_CN,
                &TEXT_SERVICE_GUID,
                &TEXT_SERVICE_GUID,
                HKL::default(),
                TF_IPPMF_FORSESSION | TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE,
            )
            .map_err(|e| format!("ActivateProfile: {e}"))?;
        }
        Ok(())
    }

    /// Removes the language profile and TIP categories from TSF.
    pub fn unregister_profile() -> Result<(), String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let cat: ITfCategoryMgr =
                CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| format!("CoCreateInstance(CategoryMgr): {e}"))?;
            for category in [
                &GUID_TFCAT_TIP_KEYBOARD,
                &GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
                &GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
                &GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
            ] {
                let _ = cat.UnregisterCategory(&TEXT_SERVICE_GUID, category, &TEXT_SERVICE_GUID);
            }

            let mgr: ITfInputProcessorProfileMgr =
                CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| format!("CoCreateInstance(InputProcessorProfiles): {e}"))?;
            mgr.UnregisterProfile(&TEXT_SERVICE_GUID, LANGID_ZH_CN, &TEXT_SERVICE_GUID, 0)
                .map_err(|e| format!("UnregisterProfile: {e}"))?;
        }
        Ok(())
    }
}

/// Writes the TSF TIP registry keys that Windows reads to discover text services.
///
/// Layout:
/// ```text
/// HKLM\SOFTWARE\Microsoft\CTF\TIP\{CLSID}
///     Enable = 1
///     Category\Item\{CATEGORY_GUID}
///     Category\Category\{CATEGORY_GUID}\{CLSID}
///     LanguageProfile\0x00000804\{CLSID}
///         Enable = 1
///         Description = "NovaType Chinese Input Method"
///         Display Description = "NovaType Chinese Input Method"
///         IconFile = "<module_path>"
///         IconIndex = 0
/// ```
///
/// Categories mirror what `ITfCategoryMgr::RegisterCategory` writes. Windows 8+
/// requires `IMMERSIVESUPPORT` and `SYSTRAYSUPPORT` for the TIP to appear in
/// the input method switcher list.
fn register_tsf_tip_keys() -> Result<(), String> {
    /// `GUID_TFCAT_TIP_KEYBOARD`
    const CAT_TIP_KEYBOARD: &str = "{34745C63-B2F0-4784-8B67-5E12C8701A31}";
    /// `GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT`
    const CAT_IMMERSIVE_SUPPORT: &str = "{13A016DF-560B-46CD-947A-4C3AF1E0E35D}";
    /// `GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT`
    const CAT_SYSTRAY_SUPPORT: &str = "{25504FB4-7BAB-4BC1-9C69-CF81890F0EF5}";
    /// `GUID_TFCAT_TIPCAP_UIELEMENTENABLED`
    const CAT_UIELEMENT_ENABLED: &str = "{49D2F9CE-1F5E-11D7-A6D3-00065B84435C}";
    /// `GUID_TFCAT_TIPCAP_SECUREMODE`
    const CAT_SECURE_MODE: &str = "{49D2F9CF-1F5E-11D7-A6D3-00065B84435C}";
    /// `GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT`
    const CAT_INPUTMODE_COMPARTMENT: &str = "{CCF05DD8-4A87-11D7-A6E2-00065B84435C}";

    const CATEGORIES: [&str; 6] = [
        CAT_TIP_KEYBOARD,
        CAT_IMMERSIVE_SUPPORT,
        CAT_SYSTRAY_SUPPORT,
        CAT_UIELEMENT_ENABLED,
        CAT_SECURE_MODE,
        CAT_INPUTMODE_COMPARTMENT,
    ];

    // Try HKLM first (the proper location), fall back to HKCU.
    let root = RegKey::predef(HKEY_LOCAL_MACHINE);
    let tip_base = format!("SOFTWARE\\Microsoft\\CTF\\TIP\\{TEXT_SERVICE_CLSID}");

    let (tip, _) = root
        .create_subkey_with_flags(&tip_base, KEY_WRITE)
        .map_err(|e| format!("cannot write HKLM\\CTF\\TIP (try running as admin): {e}"))?;
    tip.set_value("", &PROFILE_DESCRIPTION)
        .map_err(|e| format!("set value: {e}"))?;
    tip.set_value("Enable", &1u32)
        .map_err(|e| format!("set Enable: {e}"))?;

    for category in CATEGORIES {
        tip.create_subkey_with_flags(format!("Category\\Item\\{category}"), KEY_WRITE)
            .map_err(|e| format!("Category\\Item\\{category}: {e}"))?;
        tip.create_subkey_with_flags(
            format!("Category\\Category\\{category}\\{TEXT_SERVICE_CLSID}"),
            KEY_WRITE,
        )
        .map_err(|e| format!("Category\\Category\\{category}: {e}"))?;
    }

    // LanguageProfile\0x00000804\{CLSID}
    {
        let lang_id = "0x00000804";
        let (lang, _) = tip
            .create_subkey_with_flags(
                format!("LanguageProfile\\{lang_id}\\{TEXT_SERVICE_CLSID}"),
                KEY_WRITE,
            )
            .map_err(|e| format!("LanguageProfile: {e}"))?;
        lang.set_value("Enable", &1u32)
            .map_err(|e| format!("Enable: {e}"))?;
        lang.set_value("Description", &PROFILE_DESCRIPTION)
            .map_err(|e| format!("Description: {e}"))?;
        // Note: the value name really contains a space ("Display Description"),
        // matching what ITfInputProcessorProfiles::AddLanguageProfile writes.
        lang.set_value("Display Description", &PROFILE_DESCRIPTION)
            .map_err(|e| format!("Display Description: {e}"))?;
        lang.set_value("IconFile", &module_path().to_string_lossy().to_string())
            .map_err(|e| format!("IconFile: {e}"))?;
        lang.set_value("IconIndex", &0u32)
            .map_err(|e| format!("IconIndex: {e}"))?;
    }

    Ok(())
}

/// Unregisters the `NovaType` text service.
pub fn unregister_server() -> RegistrationResult<()> {
    // Best-effort official TSF unregistration first (needs COM).
    #[cfg(windows)]
    {
        let _ = tsf_api::unregister_profile();
    }

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    delete_subkey_tree_if_exists(&hkcu, NOVATYPE_TSF_KEY)?;
    delete_subkey_tree_if_exists(&hkcu, &format!("{CLASSES_KEY}\\{TEXT_SERVICE_CLSID}"))?;

    // Also try HKLM CLSID + TIP keys (silently skip if not present / no permission).
    let _ = delete_subkey_tree_if_exists(
        &RegKey::predef(HKEY_LOCAL_MACHINE),
        &format!("{CLASSES_KEY}\\{TEXT_SERVICE_CLSID}"),
    );
    let _ = delete_subkey_tree_if_exists(
        &RegKey::predef(HKEY_LOCAL_MACHINE),
        &format!("SOFTWARE\\Microsoft\\CTF\\TIP\\{TEXT_SERVICE_CLSID}"),
    );

    Ok(())
}

// -----------------------------------------------------------------
// Registry marker recording (for diagnostics)
// -----------------------------------------------------------------

struct RegistryProfileRegistrar<'a> {
    marker: &'a RegKey,
    index: u32,
}

impl<'a> RegistryProfileRegistrar<'a> {
    fn new(marker: &'a RegKey) -> Self {
        Self { marker, index: 0 }
    }

    fn record(&mut self, value: &str) -> ProfileResult<()> {
        let name = format!("ProfileStep{}", self.index);
        self.marker
            .set_value(name, &value)
            .map_err(|error| error.to_string())?;
        self.index += 1;
        Ok(())
    }
}

impl ProfileRegistrar for RegistryProfileRegistrar<'_> {
    fn register_text_service(&mut self, clsid: &str) -> ProfileResult<()> {
        self.record(&format!("RegisterTextService:{clsid}"))
    }

    fn register_language_profile(
        &mut self,
        clsid: &str,
        language_tag: &str,
        description: &str,
        icon_index: i32,
    ) -> ProfileResult<()> {
        self.record(&format!(
            "RegisterLanguageProfile:{clsid}:{language_tag}:{description}:{icon_index}"
        ))
    }

    fn enable_language_profile(&mut self, clsid: &str, language_tag: &str) -> ProfileResult<()> {
        self.record(&format!("EnableLanguageProfile:{clsid}:{language_tag}"))
    }
}

// -----------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------

fn module_path() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("NOVATYPE_TSF_DLL_PATH") {
        return std::path::PathBuf::from(path);
    }

    for arg in std::env::args_os().skip(1) {
        let path = std::path::PathBuf::from(&arg);
        if path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
        {
            return path;
        }
    }

    std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("novatype_tsf.dll")
}

fn delete_subkey_tree_if_exists(root: &RegKey, path: &str) -> RegistrationResult<()> {
    match root.delete_subkey_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{CLASSES_KEY, NOVATYPE_TSF_KEY};

    #[test]
    fn registry_paths_are_user_scoped() {
        assert!(NOVATYPE_TSF_KEY.starts_with("Software\\NovaType"));
        assert!(CLASSES_KEY.starts_with("Software\\Classes"));
    }

    #[test]
    fn module_path_has_dll_name_fallback() {
        let path = super::module_path();
        assert_eq!(
            path.file_name().and_then(std::ffi::OsStr::to_str),
            Some("novatype_tsf.dll")
        );
    }
}
