use crate::metadata::{
    ICON_INDEX, LANGUAGE_TAG, PROFILE_DESCRIPTION, PROFILE_DISPLAY_NAME, TEXT_SERVICE_CLSID,
};

/// Immutable TSF profile metadata shared by installer and COM registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsfProfile {
    pub clsid: &'static str,
    pub language_tag: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub icon_index: i32,
}

/// A deterministic registration step for the future real TSF profile API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileRegistrationStep {
    RegisterTextService {
        clsid: String,
    },
    RegisterLanguageProfile {
        clsid: String,
        language_tag: String,
        description: String,
        icon_index: i32,
    },
    EnableLanguageProfile {
        clsid: String,
        language_tag: String,
    },
}

pub type ProfileResult<T> = Result<T, String>;

/// Adapter for TSF profile registration APIs.
///
/// The production implementation will call `ITfInputProcessorProfiles`.
/// Tests use an in-memory recorder.
pub trait ProfileRegistrar {
    /// Registers the text service COM class with TSF.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform registration call fails.
    fn register_text_service(&mut self, clsid: &str) -> ProfileResult<()>;

    /// Registers a language profile for this text service.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform registration call fails.
    fn register_language_profile(
        &mut self,
        clsid: &str,
        language_tag: &str,
        description: &str,
        icon_index: i32,
    ) -> ProfileResult<()>;

    /// Enables the language profile.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform registration call fails.
    fn enable_language_profile(&mut self, clsid: &str, language_tag: &str) -> ProfileResult<()>;
}

/// Executes a profile registration plan.
///
/// # Errors
///
/// Propagates the first registrar error.
pub fn execute_registration_plan(
    registrar: &mut impl ProfileRegistrar,
    steps: &[ProfileRegistrationStep],
) -> ProfileResult<()> {
    for step in steps {
        match step {
            ProfileRegistrationStep::RegisterTextService { clsid } => {
                registrar.register_text_service(clsid)?;
            }
            ProfileRegistrationStep::RegisterLanguageProfile {
                clsid,
                language_tag,
                description,
                icon_index,
            } => registrar.register_language_profile(
                clsid,
                language_tag,
                description,
                *icon_index,
            )?,
            ProfileRegistrationStep::EnableLanguageProfile {
                clsid,
                language_tag,
            } => {
                registrar.enable_language_profile(clsid, language_tag)?;
            }
        }
    }
    Ok(())
}

impl TsfProfile {
    #[must_use]
    pub fn novatype() -> Self {
        Self {
            clsid: TEXT_SERVICE_CLSID,
            language_tag: LANGUAGE_TAG,
            display_name: PROFILE_DISPLAY_NAME,
            description: PROFILE_DESCRIPTION,
            icon_index: ICON_INDEX,
        }
    }

    #[must_use]
    pub fn registration_plan(&self) -> Vec<ProfileRegistrationStep> {
        vec![
            ProfileRegistrationStep::RegisterTextService {
                clsid: self.clsid.to_string(),
            },
            ProfileRegistrationStep::RegisterLanguageProfile {
                clsid: self.clsid.to_string(),
                language_tag: self.language_tag.to_string(),
                description: self.description.to_string(),
                icon_index: self.icon_index,
            },
            ProfileRegistrationStep::EnableLanguageProfile {
                clsid: self.clsid.to_string(),
                language_tag: self.language_tag.to_string(),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ProfileRegistrar, ProfileRegistrationStep, ProfileResult, TsfProfile,
        execute_registration_plan,
    };

    #[derive(Default)]
    struct RecordingRegistrar {
        calls: Vec<String>,
    }

    impl ProfileRegistrar for RecordingRegistrar {
        fn register_text_service(&mut self, clsid: &str) -> ProfileResult<()> {
            self.calls.push(format!("service:{clsid}"));
            Ok(())
        }

        fn register_language_profile(
            &mut self,
            clsid: &str,
            language_tag: &str,
            description: &str,
            icon_index: i32,
        ) -> ProfileResult<()> {
            self.calls.push(format!(
                "profile:{clsid}:{language_tag}:{description}:{icon_index}"
            ));
            Ok(())
        }

        fn enable_language_profile(
            &mut self,
            clsid: &str,
            language_tag: &str,
        ) -> ProfileResult<()> {
            self.calls.push(format!("enable:{clsid}:{language_tag}"));
            Ok(())
        }
    }

    #[test]
    fn builds_novatype_profile() {
        let profile = TsfProfile::novatype();

        assert_eq!(profile.display_name, "NovaType");
        assert_eq!(profile.language_tag, "zh-CN");
        assert!(profile.clsid.starts_with('{'));
    }

    #[test]
    fn registration_plan_has_expected_order() {
        let plan = TsfProfile::novatype().registration_plan();

        assert!(matches!(
            plan[0],
            ProfileRegistrationStep::RegisterTextService { .. }
        ));
        assert!(matches!(
            plan[1],
            ProfileRegistrationStep::RegisterLanguageProfile { .. }
        ));
        assert!(matches!(
            plan[2],
            ProfileRegistrationStep::EnableLanguageProfile { .. }
        ));
    }

    #[test]
    fn executes_registration_plan_in_order() {
        let plan = TsfProfile::novatype().registration_plan();
        let mut registrar = RecordingRegistrar::default();

        execute_registration_plan(&mut registrar, &plan).expect("execute plan");

        assert_eq!(registrar.calls.len(), 3);
        assert!(registrar.calls[0].starts_with("service:"));
        assert!(registrar.calls[1].starts_with("profile:"));
        assert!(registrar.calls[2].starts_with("enable:"));
    }
}
