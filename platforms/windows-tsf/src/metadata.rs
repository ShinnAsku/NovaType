//! Static metadata for the future Windows TSF profile registration.

/// Display name shown in Windows language/input settings.
pub const PROFILE_DISPLAY_NAME: &str = "NovaType";

/// Profile description shown by TSF hosts.
pub const PROFILE_DESCRIPTION: &str = "NovaType Chinese Input Method";

/// BCP-47 language tag for the initial Chinese profile.
pub const LANGUAGE_TAG: &str = "zh-CN";

/// Preferred icon resource index inside the future TSF DLL.
pub const ICON_INDEX: i32 = 0;

/// Placeholder CLSID. The COM implementation must use this exact value for
/// registration, installer scripts, and TSF profile registration.
pub const TEXT_SERVICE_CLSID: &str = "{7E4B71B0-5C48-45E8-9E4E-4DFD16FE5E95}";

#[cfg(test)]
mod tests {
    use super::{LANGUAGE_TAG, PROFILE_DISPLAY_NAME, TEXT_SERVICE_CLSID};

    #[test]
    fn metadata_is_stable() {
        assert_eq!(PROFILE_DISPLAY_NAME, "NovaType");
        assert_eq!(LANGUAGE_TAG, "zh-CN");
        assert!(TEXT_SERVICE_CLSID.starts_with('{'));
        assert!(TEXT_SERVICE_CLSID.ends_with('}'));
    }
}
