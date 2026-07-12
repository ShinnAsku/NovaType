# Windows TSF Registration Notes

This folder currently contains the testable input-session core used by the future TSF COM layer.

## Current State

- `InputSession` handles composition, paging, selection, backspace, escape, and daemon commits.
- `DaemonClient` talks to `novatyped` through `novatype-protocol`.
- The crate builds both `rlib` and `cdylib` (`novatype_tsf.dll`).
- The DLL exports `DllRegisterServer`, `DllUnregisterServer`, `DllCanUnloadNow`, and `DllGetClassObject`.
- `DllGetClassObject` now returns a minimal `IClassFactory` for `TEXT_SERVICE_CLSID`.
- `IClassFactory::CreateInstance` now returns a minimal object that supports `IUnknown` and `ITfTextInputProcessor` (`Activate` / `Deactivate` currently return `S_OK`).
- `DllCanUnloadNow` is backed by object and server-lock counters.
- `DllRegisterServer` writes minimal per-user COM metadata under `HKCU\Software\Classes\CLSID` and a NovaType marker key.
- Unit tests cover the state machine.

## Development Registration

Run from an elevated PowerShell when you want to exercise `regsvr32` manually:

```powershell
platforms\windows-tsf\register-dev.ps1
platforms\windows-tsf\unregister-dev.ps1
platforms\windows-tsf\verify-registration.ps1
```

The current skeleton registers COM metadata, exposes a class factory, and creates a minimal `ITfTextInputProcessor` object. The object does not yet connect to TSF sinks or edit sessions.

## Remaining TSF COM Work

1. Expand activation/deactivation to store `ITfThreadMgr` / client id and advise/unadvise sinks.
2. Register the TSF profile with `ITfInputProcessorProfiles`.
3. Implement key event sink and edit sessions.
4. Implement native candidate window HWND using the UI spec in `PLAN.md` §7.3.
5. Wire committed text back to the active document and call `InputSession::handle_key` for key events.

Reference projects:

- azooKey-Windows
- RIME weasel
- rakukan
