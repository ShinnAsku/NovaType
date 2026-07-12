# NovaType 设计与进度总览

> Rust 跨平台智能拼音输入法 · 完全离线可用 · 用户习惯自学习 · 可插拔 LLM Agent
>
> 更新日期：2026-07-12　详细方案见 [PLAN.md](PLAN.md)

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
│   ├── novatype-core/     # 拼音切分、词图、Viterbi、动态加词    ✅ 已实现
│   ├── novatype-model/    # 用户模型：学习/衰减/造词/联想 (redb)  ✅ 已实现
│   └── novatype-cli/      # 可学习 REPL（dogfood 入口）          ✅ 已实现
├── apps/desktop/          # Tauri 2 练习场（候选窗设计稿预览）    ✅ 已实现
│   ├── src/               # Vite + TypeScript 前端
│   └── src-tauri/         # Rust 后端：suggest / commit 命令
└── (规划中) crates/novatype-{dict,protocol,server,llm,agent}
    (规划中) platforms/{windows-tsf,fcitx5-addon,macos-imk}
```

---

## 3. 实现步骤（路线图）

| 版本 | 范围 | 验收标准 | 状态 |
|---|---|---|---|
| **v0.1** 引擎原型 | core 骨架；CLI 出候选；Tauri 练习场直连 core | 候选正确；响应 < 5ms；纯离线 | ✅ **完成** |
| **v0.2** 学习与联想 | 用户自学习、自动造词、联想；daemon + IPC；Tauri 切 IPC | 习惯调整可复现；重启不丢数据 | 🔄 **进行中（约 60%）** |
| **v0.3** Windows 可用 | TSF 薄壳 + 原生候选窗（按 §7.3 设计稿）+ 安装器 | 日常输入 dogfood 两周无崩溃 | ⬜ 未开始 |
| **v0.4** LLM 接入 | Ollama 后端 + `//指令` 模式 + 熔断降级 + Agent 控制台 | 断网时体验零差异 | ⬜ 未开始 |
| **v0.5** Linux + 设置 | fcitx5 addon；Tauri 设置界面/词库管理；rime-ice 一键导入 | fcitx5 日常可用 | ⬜ 未开始 |
| **v1.0** 三平台正式版 | macOS IMKit；双拼/模糊音；真实词库（FST + bigram） | 安装包 ≤ 35 MB；三平台 CI | ⬜ 未开始 |
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
- 品牌资产：logo.svg + Windows icon.ico

### 🔄 v0.2 剩余工作（下一步）

1. `novatype-protocol`：IPC 消息定义（serde + bincode）
2. `novatype-server`：novatyped 守护进程（`interprocess` 传输）
3. Tauri 后端从直连 core 切换为 IPC 客户端
4. 三平台 CI（引擎层 `cargo test`），防平台依赖潜入

### ⬜ 更远的关键项

- `novatype-dict`：FST 词库编译/加载，接入真实词库（rime-essay 打包 + rime-ice 导入）
- 自训 bigram 语言模型（量化压缩）
- Windows TSF 薄壳 + 原生候选窗（参考 azooKey-Windows / weasel）

---

## 5. 本地开发速查

```powershell
# 引擎 CLI（可学习 REPL：输拼音 → 输数字上屏）
cargo run -p novatype-cli            # REPL
cargo run -p novatype-cli -- nihao   # 单次查询

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
