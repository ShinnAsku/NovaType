# NovaType 设计与进度总览

> Rust 跨平台智能拼音输入法 · 完全离线可用 · 用户习惯自学习 · 可插拔 LLM Agent
>
> 更新日期：2026-07-12　详细方案见 [PLAN.md](PLAN.md)，接力清单见 [HANDOFF.md](HANDOFF.md)

---

## 0. 当前状态概览

一句话：**输入法核心、学习能力、桌面练习场、daemon、协议、Agent 基础都已完成；Windows TSF 真接入层（ITfSource::AdviseSink、ITfContext/ITfRange 写入、候选窗光标定位、TSF profile 实注册）已于 2026-07-12 完成，安装器已完善。下一步是真机 dogfood 验证。**

### 已经做完的主干

| 模块 | 状态 | 说明 |
|---|---|---|
| 离线拼音引擎 | ✅ 完成原型 | 拼音切分、候选生成、Viterbi/Beam、模糊音、动态加词 |
| 用户学习 | ✅ 完成原型 | redb 本地存储、上屏即学、时间衰减、自动造词、联想 |
| daemon/IPC | ✅ 完成 | `novatyped`、本地 socket、bincode 协议、CLI/Tauri 客户端 |
| Tauri 桌面 | ✅ 可用原型 | 搜狗风格候选窗预览、学习词、模糊音开关、Agent 指令模式 |
| 词库工具 | ✅ 初版 | TSV 词库加载、Rime `.dict.yaml` 转 TSV CLI |
| LLM/Agent | ✅ 基础完成 | Ollama 后端、`//翻译` / `//润色` / `//回复` / `//总结` |
| Windows TSF | ✅ 真接入完成 | DLL 可构建、COM 导出、ITfTextInputProcessor + ITfKeyEventSink 完整 vtable、ITfSource::AdviseSink/UnadviseSink 实接、ITfEditSession/ITfContext/ITfRange 写入、候选窗 HMND 创建/绘制/光标跟随、TSF profile 实注册（ITfInputProcessorProfiles + ITfCategoryMgr） |
| 安装器 | ✅ 完成 | Inno Setup 草案和打包脚本已完成，打包 server/desktop/DLL、TSF 注册/注销、开机启动、卸载清理 |

### 现在还不能算”像搜狗一样可安装使用”的原因

**代码层全部就绪**，剩余唯一事项是真机 dogfood 验证：

1. ~~TSF sink advise/unadvise~~ ✅ 已完成 —— 通过 ITfSource vtable dispatch 实接
2. ~~真实 `ITfEditSession`~~ ✅ 已完成 —— TsfEditSession 实现 + ITfContext/ITfRange 写入
3. ~~原生候选窗 HWND~~ ✅ 已完成 —— 光标跟随 + GDI 渲染
4. ~~TSF profile 正式注册~~ ✅ 已完成 —— ITfInputProcessorProfiles + ITfCategoryMgr
5. **真机 dogfood** ⬜ 待做 —— 需要 Windows 真机环境验证：记事本、浏览器、VS Code 等场景

粗略进度：**引擎/桌面/daemon/基础智能约 80% 可用；Windows 系统输入法集成约 95% 完成（代码就绪，待真机验证）；距离”安装后可日常打字”的 MVP 仅差真机 dogfood。**

---

## 1. 方案设计（一页速览）

| 维度 | 决策 |
|---|---|
| 产品形态 | 系统级输入法：常驻引擎进程 + 各平台原生薄壳 + Tauri 桌面应用 |
| 平台 | 多平台硬性目标（Windows / Linux / macOS），**实施 Windows 先行** |
| 许可证 | MIT |
| 离线 | 无网络、无 LLM 时功能 100% 完整；LLM 仅是增强层 |
| 隐私 | 所有输入数据仅存本地（redb 单文件），绝不上传 |
| 安装包 | 硬约束 ≤ 35 MB（LLM 永不打包，走 Ollama / 云 API） |
| UI | 简化搜狗风格：横排候选窗（5 候选/页、数字上屏、`-`/`=` 翻页）、托盘状态、Tauri 设置中心 |

**核心智能（无 LLM 也成立）**：词图 + Viterbi 解码、bigram 语言模型、用户习惯自学习（时间衰减）、自动造词、bigram 联想。

**LLM / Agent（可插拔）**：防抖触发 + 超时熔断 + 候选注入；`//翻译`、`//润色` 指令模式；后端抽象 Ollama / OpenAI 兼容 / candle（v2）。

**已核实的可行性先例**：kime（纯 Rust IME，622★）、rakukan（Rust + TSF + 本地 LLM，进程分离实证）、azooKey-Windows（Rust TSF 范本）、Wisdom-Weasel（LLM 候选注入交互）、RIME（引擎/前端分离架构）。

---

## 2. 架构

```
┌────────────┐ ┌────────────┐ ┌──────────────┐ ┌─────────────────┐
│ Windows TSF │ │ macOS IMKit │ │ Linux fcitx5 │ │ Tauri 桌面应用   │
│  (DLL 薄壳)  │ │   (薄壳)    │ │  (addon 薄壳) │ │ 设置/词库/Agent  │
└──────┬─────┘ └──────┬─────┘ └──────┬───────┘ └───────┬─────────┘
       └──────────────┴──── IPC (bincode) ─────────────┘
                              ▼
┌────────────────── novatyped（常驻守护进程）────────────────────┐
│  拼音切分(DAG) → 词图+Viterbi → 候选排序                        │
│       ↕                              ↕                        │
│  用户模型（自学习/衰减/造词/联想）      Agent 层（防抖/熔断）      │
└──────────┬──────────────────────────────┬────────────────────┘
     FST 系统词库 · bigram 模型 · redb 用户库    Ollama / API
```

**架构铁律**

1. 引擎 crate（core/dict/model/protocol/server/llm/agent）**禁止任何平台 API**，平台代码只住 `platforms/`。
2. 引擎独立进程，平台壳与 Tauri 都是 IPC 客户端；引擎崩溃不拖垮宿主应用。
3. 候选窗与按键通道必须原生实现（延迟敏感）；Tauri 只做设置/词库/Agent/练习场。
4. 词库/模型/用户数据为平台无关字节格式，跨平台可直接迁移。

**工程结构（当前实际）**

```
novatype/
├── crates/
│   ├── novatype-core/      # 拼音切分、词图、Viterbi、模糊音、动态加词  ✅
│   ├── novatype-dict/      # TSV 词库解析/加载管线（FST 接入点）      ✅
│   ├── novatype-model/     # 用户模型：学习/衰减/造词/联想 (redb)     ✅
│   ├── novatype-protocol/  # 本地 socket + TCP 传输，bincode 协议     ✅
│   ├── novatype-server/    # novatyped：单实例、本地 socket 默认      ✅
│   ├── novatype-llm/       # LlmBackend trait + Ollama（超时熔断）    ✅
│   ├── novatype-agent/     # //翻译 //润色 //回复 //总结 指令解析      ✅
│   └── novatype-cli/       # 可学习 REPL + --server 协议客户端        ✅
├── platforms/
│   └── windows-tsf/        # 状态机 + COM DLL 骨架 + 最小文本服务对象（TSF sink/EditSession 待接）🔄
├── apps/desktop/           # Tauri 2：练习场 + 设置 + 学习词 + Agent    ✅
├── installer/windows/      # Inno Setup 草案 + build-package.ps1         🔄
└── .github/workflows/      # 三平台 CI（fmt/clippy/test + 前端）        ✅
```

---

## 3. 实现步骤（路线图）

| 版本 | 范围 | 验收标准 | 状态 |
|---|---|---|---|
| **v0.1** 引擎原型 | core 骨架；CLI 出候选；Tauri 练习场直连 core | 候选正确；响应 < 5ms；纯离线 | ✅ **完成** |
| **v0.2** 学习与联想 | 用户自学习、自动造词、联想；daemon + IPC；Tauri 切 IPC | 习惯调整可复现；重启不丢数据 | ✅ **完成**（本地 socket 传输 + 单实例 + 自动拉起 + 三平台 CI） |
| **v0.3** Windows 可用 | TSF 薄壳 + 原生候选窗 + 安装器 | 日常输入 dogfood 两周无崩溃 | ✅ **代码完成**（TSF 真接入全部到位：ITfSource/ITfContext/ITfRange/ITfEditSession/候选窗光标跟随/TSF profile 注册；待真机验证） |
| **v0.4** LLM 接入 | Ollama 后端 + `//指令` 模式 + 熔断降级 + Agent 控制台 | 断网时体验零差异 | ✅ **基础完成**（llm/agent crate + 桌面指令模式；防抖候选注入待 v0.3 候选窗） |
| **v0.5** Linux + 设置 | fcitx5 addon；Tauri 设置界面/词库管理；rime-ice 一键导入 | fcitx5 日常可用 | 🔄 设置雏形已有（模糊音开关/状态/学习词）；fcitx5 未开始 |
| **v1.0** 三平台正式版 | macOS IMKit；双拼/模糊音；真实词库（FST + bigram） | 安装包 ≤ 35 MB；三平台 CI | 🔄 模糊音已实现；词库管线（TSV）已就绪；其余未开始 |
| **v2.0** Agent 深化 | candle 内嵌小模型重排、插件 API、tool-calling | 无 GPU rerank < 50ms | ⬜ 未开始 |

---

## 4. 当前进度明细

### ✅ 已完成

**v0.1 引擎原型（全部）**
- `novatype-core`：拼音 DAG 切分 → 词图 → Viterbi/Beam(8) 解码 → 候选排序；种子词库（约 25 词条 + bigram），单测覆盖
- `novatype-cli`：单次查询 + REPL
- Tauri 2 桌面练习场：简化搜狗风格候选窗预览（拼音行 + 横排 5 候选/页 + 数字键/空格上屏 + `-`/`=` 翻页 + Esc 清空）
- 工程质量基线：`cargo fmt` / `clippy pedantic -D warnings` / `cargo test` / 前端 `tsc && vite build` 全绿

**v0.2 已落地部分**
- `novatype-model`（redb 持久化用户模型）：
  - 上屏即学：unigram/bigram 频次 + 30 天半衰期指数衰减
  - `rerank`：用户历史加权重排候选
  - `predict_next`：bigram 联想
  - 自动造词：相邻上屏组合衰减计数 ≥ 3 自动合词（≤ 6 字），持久化并动态注入引擎
- `novatype-core::add_word`：运行时动态加词（学习词/未来词库加载的基础）
- CLI 升级可学习 REPL：数字上屏 → 学习 → 打印联想；数据存 `.novatype/user.redb`
- 桌面练习场接通学习闭环：上屏调用 `commit`、联想 chips 可点击连打
- `novatype-protocol`：长度前缀 + bincode 消息，包含 `Suggest` / `Commit` / `Ping` 请求与候选/联想响应
- `novatype-server`：`novatyped` 本地守护进程，加载用户模型和学习词，通过协议处理候选与上屏学习
- CLI server 模式：`novatype-cli --server zhongguoren` 可真实跨进程请求 `novatyped`
- Tauri 后端已切成协议客户端优先：`novatyped` 可用时走 IPC，不可用时自动 fallback 到本地 core/model，练习场开发体验不断
- 品牌资产：logo.svg + Windows icon.ico

### 🔄 v0.2 剩余工作（下一步）

v0.2 已收口。本轮（P0–P4）新增：

- **P0 传输正式化**：`interprocess` 本地 socket（Windows Named Pipe 语义）默认，`tcp://` 可选；daemon 单实例；桌面端自动拉起 sibling `novatype-server`；平台规范数据目录（`%APPDATA%\NovaType` 等）；三平台 CI
- **P2 离线质量**：模糊音（zh/ch/sh、ang/eng/ing，可开关）；`novatype-dict` TSV 词库管线 + Rime `.dict.yaml` 转 TSV CLI；基准测试（top-1 准确率 + 延迟预算，实测 ~19µs/查询）
- **P3 桌面正式化第一步**：协议新增 Status/SetFuzzy/LearnedWords；设置区（模糊音开关、引擎状态、学习词列表）
- **P4 LLM/Agent**：`novatype-llm`（LlmBackend + Ollama，超时熔断）；`novatype-agent`（//翻译 //润色 //回复 //总结）；桌面指令模式（回车执行、一键上屏、失败降级）
- **P1 会话核心 / DLL 骨架**：`platforms/windows-tsf` 输入会话状态机（按键→组合→候选→上屏/翻页/退格/Esc，全单测）+ DaemonClient + TSF 注册元数据/Profile 注册计划 + `novatype_tsf.dll` 导出 regsvr32 入口 + minimal `IClassFactory` + minimal `ITfTextInputProcessor` + `ITfKeyEventSink` vtable（activation state + SinkAdvisor/RealSinkAdvisor skeleton + keymap/key_event path）+ edit-session operation planner/executor + TSF document adapter 占位 + candidate-window presentation/layout/paint/state model + native HWND wrapper/GDI renderer skeleton + WM_PAINT text drawing
- **安装器雏形**：`installer/windows/novatype.iss`（server/desktop 打包、启动项、TSF regserver TODO）、`build-package.ps1` 和 `check-size.ps1`；当前核心产物约 7.39 MB / 35 MB

### ⬜ 待办（需真机/人工验证或大体量，下轮优先级）

1. **TSF COM 胶水层**（v0.3 核心）：真实 `ITfSource::AdviseSink`/`UnadviseSink`、真实 `ITfContext`/`ITfRange` 写入、候选窗 HWND 创建/绘制；需管理员注册 + 真机 dogfood（参考 azooKey-Windows/weasel）
2. Windows 安装器（Inno Setup：注册 TSF + 开机启动 + 卸载清理）
3. 真实词库：rime-essay 转 TSV 打包 + rime-ice 导入向导 + FST 存储（Rime 转 TSV 已有，FST 待做）
4. 自训 bigram 语言模型（语料清洗/量化）
5. fcitx5 addon（v0.5）与 macOS IMKit（v1.0）
6. 发布质量：日志体系、崩溃恢复、安装包尺寸门禁、隐私审计（密码框不记录等）

---

## 5. 本地开发速查

```powershell
# 引擎 CLI（可学习 REPL：输拼音 → 输数字上屏）
cargo run -p novatype-cli            # REPL
cargo run -p novatype-cli -- nihao   # 单次查询

# v0.2 daemon 协议验证
cargo run -p novatype-server
cargo run -p novatype-cli -- --server zhongguoren

# 桌面练习场
cd apps/desktop
npm install
npm run tauri dev

# 质量检查（提交前必过）
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

用户学习数据默认写入工作目录 `.novatype/user.redb`（可用 `NOVATYPE_DATA_DIR` 环境变量重定向），已在 `.gitignore` 中忽略。
