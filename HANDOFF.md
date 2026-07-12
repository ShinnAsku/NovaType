# NovaType 交接说明

> 给后续 AI/开发者使用。最后校验人：GitHub Copilot。

## 当前结论

NovaType 已经完成输入法核心、学习系统、daemon/IPC、Tauri 练习场、Agent 基础和 Windows TSF 大部分可测试骨架。还不能作为 Windows 输入法日常打字，原因是 TSF 真接入还剩 `ITfSource::AdviseSink`、`ITfContext/ITfRange` 写入、候选窗光标定位和真机注册联调。

当前 Windows 集成粗略进度：约 63%。

## 已完成模块

- `crates/novatype-core`
  - 拼音切分、候选生成、Viterbi/Beam、模糊音、动态加词。
- `crates/novatype-model`
  - redb 用户学习、时间衰减、自动造词、联想。
- `crates/novatype-protocol`
  - 本地 socket + TCP dev fallback，bincode 协议。
- `crates/novatype-server`
  - `novatyped` 守护进程。
- `crates/novatype-dict`
  - TSV 词库加载，Rime `.dict.yaml` 转 TSV。
- `crates/novatype-llm` / `crates/novatype-agent`
  - Ollama 后端，`//翻译` / `//润色` / `//回复` / `//总结` 指令。
- `apps/desktop`
  - Tauri 练习场、候选条预览、学习词、模糊音开关、Agent 指令模式。
- `platforms/windows-tsf`
  - `novatype_tsf.dll` 可构建。
  - COM 导出：`DllRegisterServer` / `DllUnregisterServer` / `DllCanUnloadNow` / `DllGetClassObject`。
  - minimal `IClassFactory`。
  - minimal `ITfTextInputProcessor`。
  - `ITfKeyEventSink` vtable。
  - `key_event` 行为层：OnTestKeyDown / OnKeyDown。
  - `edit_session` operation planner/executor。
  - `tsf_document` adapter 占位。
  - candidate window presentation/layout/paint/state model。
  - `NativeCandidateWindow` HWND wrapper + `WM_PAINT`/GDI renderer 骨架。
  - `candidate-window-smoke` 可运行。
- `installer/windows`
  - Inno Setup 草案。
  - `build-package.ps1`。
  - `check-size.ps1`。

## 继续任务，按优先级

### 1. 真实 `ITfSource::AdviseSink / UnadviseSink`

文件：

- `platforms/windows-tsf/src/sink.rs`
- `platforms/windows-tsf/src/com.rs`

现状：

- `SinkAdvisor` trait 已有。
- `LocalSinkAdvisor` 当前用于模型测试。
- `RealSinkAdvisor` 是 Windows-only 占位。

目标：

- 用 `windows-rs` 的 TSF `ITfSource` 接口调用 `AdviseSink(IID_ITfKeyEventSink, sink, &mut cookie)`。
- `Deactivate` 时调用 `UnadviseSink(cookie)`。
- 让 `TextService::Activate/Deactivate` 使用真实 advisor。

注意：不要破坏现有 `key_event` 行为层，它已经可测。

### 2. 真实 `ITfContext / ITfRange` 写入

文件：

- `platforms/windows-tsf/src/tsf_document.rs`
- `platforms/windows-tsf/src/edit_session.rs`

现状：

- `DocumentEditor` trait 已有。
- `execute_operations()` 已有。
- `TsfDocumentEditor` 当前只保存 opaque context 和 fake state。

目标：

- 实现真实 composition/preedit 更新。
- 实现 commit text 写入宿主应用。
- 处理 ClearComposition / HideCandidates。

### 3. 候选窗真实光标定位

文件：

- `platforms/windows-tsf/src/native_window.rs`
- `platforms/windows-tsf/src/window.rs`
- `platforms/windows-tsf/src/tsf_document.rs`

现状：

- HWND wrapper 可创建/显示/隐藏/移动/销毁。
- WM_PAINT 已接 GDI renderer。
- `candidate-window-smoke` 可运行。

目标：

- 从 TSF context/range 获取 caret rect。
- 调用 `NativeCandidateWindow::update_view(view, caret, metrics)`。
- 真机验证候选窗跟随光标。

### 4. TSF profile 实注册

文件：

- `platforms/windows-tsf/src/profile.rs`
- `platforms/windows-tsf/src/registration.rs`

现状：

- `ProfileRegistrar` trait 已有。
- 注册 plan 顺序已固定。
- 当前写 registry marker。

目标：

- 调用 `ITfInputProcessorProfiles` 注册并启用语言配置。
- 让 Windows 输入法列表中出现 NovaType。

### 5. 安装器完善

文件：

- `installer/windows/novatype.iss`
- `installer/windows/build-package.ps1`
- `installer/windows/check-size.ps1`

现状：

- Inno Setup 草案已存在。
- release 核心产物目前约 7.40 MB / 35 MB。

目标：

- 安装 server/desktop/TSF DLL。
- 注册/注销 TSF。
- 开机启动 daemon。
- 卸载清理。

## 每次修改后必须验证

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build -p novatype-tsf --release
platforms\windows-tsf\check-exports.ps1
installer\windows\check-size.ps1
Push-Location apps\desktop
npm run build
Pop-Location
```

如果改了候选窗，还要跑：

```powershell
cargo run -p novatype-tsf --bin candidate-window-smoke
```

## 当前最后一次已知验证

- fmt = 0
- clippy = 0
- tests = 0
- TSF release = 0
- COM exports OK
- size = 7.40 MB / 35 MB
- frontend build = 0

## 不要做的事

- 不要把候选窗搬到 Tauri，正式输入候选窗必须是原生 HWND。
- 不要把 Windows API 泄漏到 `crates/novatype-core/model/protocol/server/llm/agent`。
- 不要打包 LLM 模型，LLM 只走 Ollama/API。
- 不要随包分发 GPL 词库（如 rime-ice），只做用户导入。
