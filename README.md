# NovaType

NovaType is a Rust-first, offline-capable Chinese input method prototype.

Current milestone: **v0.1 engine prototype**.

Project decisions:

- License: MIT
- Platform goal: Windows, Linux, and macOS; implementation starts on Windows
- Desktop app: Tauri 2 practice/settings shell
- Current implementation target: local CLI + desktop practice engine before TSF integration

```powershell
cargo run -p novatype-cli -- nihao
cargo run -p novatype-cli -- zhongguoren
cargo test

# v0.2 daemon protocol loop
cargo run -p novatype-server
cargo run -p novatype-cli -- --server zhongguoren

# dictionary conversion
cargo run -p novatype-dict --bin rime-to-tsv -- path\to\input.dict.yaml output.tsv

# Windows TSF DLL skeleton
cargo build -p novatype-tsf --release
platforms\windows-tsf\check-exports.ps1
platforms\windows-tsf\verify-registration.ps1  # registers/unregisters via regsvr32

cd apps/desktop
npm install
npm run tauri dev
```

The desktop app prefers the running `novatyped` daemon and falls back to local core/model when the daemon is unavailable. The v0.2 scope proves this IPC loop before adding Windows TSF integration.