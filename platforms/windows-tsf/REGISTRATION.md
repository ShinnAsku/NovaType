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
- `Activate` / `Deactivate` now track thread manager pointer, client id, and activation state.
- `sink` tracks key-event sink cookie lifecycle; `Activate` advises the modeled key sink and `Deactivate` unadvises it.
- `keymap` maps Windows virtual keys to the shared `InputSession` state machine for the future key event sink.
- `key_event` models `OnTestKeyDown` / `OnKeyDown`: it decides whether to eat a key and produces edit operations.
- `TextService` now exposes an `ITfKeyEventSink` vtable; `QueryInterface(IID_ITfKeyEventSink)` works, and `OnTestKeyDown` / `OnKeyDown` call the `key_event` behavior layer.
- `profile` builds the ordered TSF profile registration plan: register text service, register language profile, then enable it.
- `window` defines the native candidate window class name, size metrics, caret-relative positioning, paint commands, and visibility/bounds state holder.
- `native_window` wraps HWND lifecycle (register class, create popup, show/hide/move/destroy) and wires `WM_PAINT` to a GDI paint-command renderer skeleton.
- `TsfDocumentEditor` uses `NativeCandidateWindow` on Windows when `ShowCandidates`/`HideCandidates` operations execute.
- `edit_session` translates session actions into deterministic operations (`SetComposition`, `CommitText`, `ShowCandidates`, etc.).
- `edit_session` also exposes a `DocumentEditor` trait and executor, so future real `ITfEditSession` code only needs to implement the adapter.
- `tsf_document` provides the first TSF document adapter boundary; it stores opaque TSF context/edit-cookie state and executes operations against a testable document model.
- `candidate_window` builds the simplified Sogou-style presentation model consumed by the future native HWND renderer.
- `DllRegisterServer` writes minimal per-user COM metadata under `HKCU\Software\Classes\CLSID` and a NovaType marker key.
- Unit tests cover the state machine.

## Development Registration

Run from an elevated PowerShell when you want to exercise `regsvr32` manually:

```powershell
platforms\windows-tsf\register-dev.ps1
platforms\windows-tsf\unregister-dev.ps1
platforms\windows-tsf\verify-registration.ps1
```

The current skeleton registers COM metadata, exposes a class factory, creates a minimal `ITfTextInputProcessor` object, and exposes a tested `ITfKeyEventSink` vtable feeding `InputSession`. It also produces edit-session operation plans, executes them through a `DocumentEditor` boundary, has a TSF document adapter placeholder, and on Windows can create/show/hide/move and paint a candidate HWND wrapper. The object does not yet call real TSF `ITfSource` / `ITfContext` / `ITfRange` APIs.

## Remaining TSF COM Work

1. Advise/unadvise the real `ITfKeyEventSink` through `ITfSource` during activation/deactivation.
2. Execute the `profile` plan through real `ITfInputProcessorProfiles` calls.
3. Replace `TsfDocumentEditor` opaque context placeholders with real `ITfContext` / `ITfRange` edit-session execution.
4. Implement native candidate window HWND renderer for `CandidateWindowView`.

Reference projects:

- azooKey-Windows
- RIME weasel
- rakukan
