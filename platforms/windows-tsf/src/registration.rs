use crate::metadata::{PROFILE_DESCRIPTION, PROFILE_DISPLAY_NAME, TEXT_SERVICE_CLSID};
use crate::profile::{ProfileRegistrar, ProfileResult, TsfProfile, execute_registration_plan};
use std::error::Error;
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};

type RegistrationResult<T> = Result<T, Box<dyn Error>>;

const NOVATYPE_TSF_KEY: &str = "Software\\NovaType\\TSF";
const CLASSES_KEY: &str = "Software\\Classes\\CLSID";

pub fn register_server() -> RegistrationResult<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let module_path = module_path();

    let (marker, _) = hkcu.create_subkey_with_flags(NOVATYPE_TSF_KEY, KEY_ALL_ACCESS)?;
    marker.set_value("DisplayName", &PROFILE_DISPLAY_NAME)?;
    marker.set_value("Description", &PROFILE_DESCRIPTION)?;
    marker.set_value("Clsid", &TEXT_SERVICE_CLSID)?;
    marker.set_value("ModulePath", &module_path.to_string_lossy().to_string())?;
    let profile = TsfProfile::novatype();
    marker.set_value("LanguageTag", &profile.language_tag)?;
    marker.set_value("IconIndex", &profile.icon_index.cast_unsigned())?;
    let mut registrar = RegistryProfileRegistrar::new(&marker);
    execute_registration_plan(&mut registrar, &profile.registration_plan())
        .map_err(std::io::Error::other)?;

    let clsid_path = format!("{CLASSES_KEY}\\{TEXT_SERVICE_CLSID}");
    let (clsid, _) = hkcu.create_subkey_with_flags(&clsid_path, KEY_ALL_ACCESS)?;
    clsid.set_value("", &PROFILE_DESCRIPTION)?;

    let (inproc, _) = clsid.create_subkey_with_flags("InprocServer32", KEY_ALL_ACCESS)?;
    inproc.set_value("", &module_path.to_string_lossy().to_string())?;
    inproc.set_value("ThreadingModel", &"Apartment")?;

    Ok(())
}

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

pub fn unregister_server() -> RegistrationResult<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    delete_subkey_tree_if_exists(&hkcu, NOVATYPE_TSF_KEY)?;
    delete_subkey_tree_if_exists(&hkcu, &format!("{CLASSES_KEY}\\{TEXT_SERVICE_CLSID}"))?;
    Ok(())
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
