#![cfg(windows)]

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    CLSID_TF_InputProcessorProfiles, GUID_TFCAT_TIP_KEYBOARD, ITfInputProcessorProfileMgr,
    TF_INPUTPROCESSORPROFILE, TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE, TF_IPPMF_FORSESSION,
    TF_PROFILETYPE_INPUTPROCESSOR,
};
use windows::core::{GUID, IUnknown};

const NOVATYPE_GUID: GUID = GUID::from_u128(0x7E4B_71B0_5C48_45E8_9E4E_4DFD_16FE_5E95);
const LANGID_ZH_CN: u16 = 0x0804;

fn main() -> windows::core::Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let mgr: ITfInputProcessorProfileMgr =
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?;

        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|arg| arg == "activate") {
            mgr.ActivateProfile(
                TF_PROFILETYPE_INPUTPROCESSOR,
                LANGID_ZH_CN,
                &NOVATYPE_GUID,
                &NOVATYPE_GUID,
                HKL::default(),
                TF_IPPMF_FORSESSION | TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE,
            )?;
            println!("activated NovaType");
        }

        if args.iter().any(|arg| arg == "cocreate") {
            let _unknown: IUnknown = CoCreateInstance(&NOVATYPE_GUID, None, CLSCTX_INPROC_SERVER)?;
            println!("CoCreateInstance(NovaType) OK");
        }

        let mut active = TF_INPUTPROCESSORPROFILE::default();
        mgr.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &raw mut active)?;
        println!("active langid=0x{:04X}", active.langid);
        println!("active clsid={:?}", active.clsid);
        println!("active profile={:?}", active.guidProfile);
        println!("novatype clsid={NOVATYPE_GUID:?}");
    }
    Ok(())
}
