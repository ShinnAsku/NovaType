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

cd apps/desktop
npm install
npm run tauri dev
```

The v0.1 scope is intentionally small: prove the local core path from pinyin segmentation to candidate ranking, then expose it through a Tauri practice window before adding Windows TSF integration.