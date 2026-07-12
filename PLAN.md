# NovaType 方案计划书

> Rust 跨平台智能拼音输入法：完全离线可用 · 用户习惯自学习 · 可插拔 LLM Agent
>
> 版本：v1.0（2026-07-12）　状态：v0.2 进行中——自学习/联想/自动造词/daemon/IPC/Tauri 协议客户端已落地

---

## 1. 产品定位

| 维度 | 决策 |
|---|---|
| 形态 | 系统级输入法（非悬浮工具），常驻守护进程 + 各平台薄壳 |
| 桌面应用 | **Tauri 2**：设置界面、词库管理、Agent 控制台、引擎调试练习场（复用系统 WebView2，不打包 Chromium） |
| 平台 | **多平台是硬性目标**：Windows / Linux / macOS 全支持；实施顺序 **Windows 先行** → Linux/fcitx5 → macOS |
| 隐私 | 所有输入数据仅存本地，LLM 功能默认关闭、明示开关 |
| 安装包 | **硬约束 ≤ 35 MB**（压缩后目标 20–30 MB） |
| 离线 | 无网络、无 LLM 时功能 100% 完整，LLM 仅是增强层 |

---

## 2. 可行性论证（已核实的先例）

| 案例 | 证明了什么 | 我们借鉴什么 |
|---|---|---|
| [kime](https://github.com/Riey/kime)（622★，Rust 85%，45 贡献者） | 纯 Rust 输入法可达发行版级品质；小内存、多前端 | 引擎/前端分离的 crate 组织、Linux 各前端（XIM/Wayland/GTK/Qt）适配方式 |
| [rakukan](https://github.com/fukuyori/rakukan)（MIT，Rust 81%） | **Rust + TSF + 本地 LLM** 输入法真实可跑；out-of-process 架构解决宿主崩溃（v0.4.4 实证） | TSF DLL 与 engine-host 分进程的 RPC 设计、Inno Setup 打包、`cargo make` 构建流水线 |
| [azooKey-Windows](https://github.com/fkunn1326/azooKey-Windows) | Windows TSF 层的 Rust 实现范本 | TSF COM 注册、组合/候选窗协议的具体写法 |
| [Wisdom-Weasel](https://github.com/scukeqi/Wisdom-Weasel)（343★） | LLM 增强中文 IME 的交互模式成立 | **防抖 + 异步 + 超时熔断**，LLM 结果作为追加候选注入 |
| [RIME/librime](https://github.com/rime/librime) + weasel/squirrel | 引擎与平台前端分离是中文 IME 最成功架构 | 词库 schema 思路、用户词库快照/合并机制 |

结论：方案中每一个技术环节都有至少一个真实项目做过并可参考，无未验证的空想组件。

### 已排除的假依赖（前方案审查结论）

- `imekit`（9★ v0.1.1）与 `smartkey`（1★）为玩具级仓库，**不作为依赖**，仅可读源码参考。
- `Inputx` 查无此项目。
- **跨平台 IME 接入层没有现成 Rust 轮子，必须自研**——这是项目最大工作量所在，路线图已按此排期。

---

## 3. 总体架构

```
┌────────────┐ ┌────────────┐ ┌──────────────┐
│ Windows TSF │ │ macOS IMKit │ │ Linux fcitx5 │   平台薄壳：只转发按键/渲染候选
│  (DLL, 薄壳) │ │  (壳, 薄壳)  │ │ (addon, 薄壳) │
└──────┬─────┘ └──────┬─────┘ └──────┬───────┘
       └───── IPC: NamedPipe / UnixSocket, bincode ─────┐
                                                        ▼
┌─────────────────────────── novatyped（常驻守护进程）──────────────┐
│  会话管理  →  拼音切分(DAG)  →  词图+Viterbi  →  候选排序          │
│                    ↕                    ↕                        │
│  用户模型（自学习/时间衰减/自动造词）    Agent 层（可插拔，超时熔断）  │
└───────────────┬──────────────────────────┬───────────────────────┘
        ┌───────┴────────┐          ┌──────┴───────┐
        │ FST 系统词库(mmap)│          │ Ollama / API │
        │ 量化 bigram 模型  │          │ (永不打包)    │
        │ redb 用户词库     │          └──────────────┘
        └────────────────┘
```

关键决策（均有先例背书）：

1. **引擎独立进程**：三平台共享一份内存；引擎崩溃不拖垮宿主应用（rakukan 实证）。
2. **IPC 用长度前缀 + bincode**：本地进程间无兼容包袱，比 JSON-RPC 快。
3. **LLM 永不打包**：对接用户已装的 Ollama 或云端 API；本地内嵌模型（candle）留到 v2。
4. **Tauri 桌面应用是 novatyped 的另一个 IPC 客户端**：与平台薄壳平级，不在输入按键路径上。

### 3.1 跨平台策略（目标多平台，实施 Windows 先行）

多平台不是“以后再说”，而是**从第一天就约束架构**；但实施上一次只打一个平台：

| 规则 | 说明 |
|---|---|
| 平台无关性钢红线 | `core` / `dict` / `model` / `protocol` / `server` / `llm` / `agent` 七个 crate **禁止出现任何平台 API**（`#[cfg(windows)]` 等一律不得进入），平台代码只允许存在于 `platforms/` 薄壳 |
| IPC 抽象 | 传输层用 `interprocess` 统一封装（Windows NamedPipe / Unix Socket），协议层（bincode 消息）完全平台无关 |
| 数据格式可携 | FST 词库、bigram 模型、redb 用户库均为平台无关字节格式，用户换平台可直接迁移数据目录 |
| CI 保障 | 从 v0.2 起 CI 在 Windows/Linux/macOS 三平台跑 `cargo test`（引擎层），即使平台壳尚未实现，防止平台依赖潜入 |
| 实施顺序 | **Windows（v0.3）→ Linux/fcitx5（v0.5）→ macOS/IMKit（v1.0）**；后续平台只需新写薄壳，引擎/数据/桌面应用零改动 |
| Tauri 跨平台 | Tauri 2 本身支持三平台，桌面应用代码一份通吃（Linux 用 webkitgtk，macOS 用 WKWebView） |

---

## 4. 核心引擎（离线智能）

### 4.1 转换流水线

1. **拼音切分**：DAG + 动态规划，支持全拼/简拼/模糊音（`zh↔z`、`ang↔an` 等，配置化）。
2. **候选生成**：词图（lattice）+ Viterbi/Beam Search（beam=8），打分：

   `score = λ1·log P(词|FST词频) + λ2·log P(bigram) + λ3·用户权重`

3. **性能预算**：单次按键 → 候选刷新 < 5 ms（kime 同级指标）。

### 4.2 用户习惯自学习（无 LLM 的"智能"来源）

- **上屏即学**：记录用户 unigram/bigram 频次，**指数时间衰减**（半衰期 ≈ 30 天）。
- **自动造词**：相邻上屏组合出现 ≥ N 次 → 自动入用户词库。
- **自适应混合**：`最终分 = α·系统模型 + β·用户模型`，β 随用户数据量增长。
- **智能联想**：上屏后由 bigram + 用户历史预测下一词，即离线版"续写"。
- 存储：[`redb`](https://crates.io/crates/redb)（纯 Rust 嵌入式 KV，ACID，单文件）。

### 4.3 数据结构与尺寸预算

| 组件 | 技术 | 大小 |
|---|---|---|
| 核心二进制 + daemon | release + LTO + strip + `panic=abort` | 3–5 MB |
| 平台壳 DLL/插件 | 薄壳 | 1–2 MB |
| 系统词库（≈40 万词条） | [`fst`](https://crates.io/crates/fst) + mmap（启动零拷贝） | 8–12 MB |
| Bigram 语言模型 | u8 量化对数概率 + trie 压缩（借鉴 KenLM） | 12–18 MB |
| 桌面应用（Tauri 2） | 系统 WebView2，非 Electron；前端资源压缩内嵌 | 3–5 MB |
| **安装包（zstd）** | | **20–32 MB ✅** |

> Windows 10/11 自带 WebView2 Runtime，Tauri 不引入 Chromium 体积；万一目标机缺失，安装器走 WebView2 bootstrapper 在线补装（不计入包体）。

扩展词库（专业领域、大模型词库）一律做成**设置内可选下载**，不进主包。

---

## 5. 词库与授权

| 来源 | 许可 | 用法 |
|---|---|---|
| [rime-essay 八股文](https://github.com/rime/rime-essay) + luna_pinyin | LGPL | ✅ 打包进主安装包 |
| 自训 bigram（中文维基 + 新闻语料，jieba-rs 分词） | 自有 | ✅ 打包，M2 阶段训练 |
| [THUOCL 分领域词库](http://thuocl.thunlp.org/) | 开放 | ✅ 可选下载扩展包 |
| [rime-ice 雾凇拼音](https://github.com/iDvel/rime-ice) | **GPLv3** | ⚠️ 仅做"一键导入"，不随包分发（除非项目定 GPL） |
| 搜狗细胞词库 | 版权不明 | 仅提供用户自行导入的格式转换工具 |

> 项目许可证已定：**MIT**。GPL 词库（如 rime-ice）仅提供用户自行导入能力，不随安装包分发。

---

## 6. LLM / Agent 层（可插拔增强）

交互模式全部来自 Wisdom-Weasel / rakukan 的实战经验：

- **防抖触发**：输入停顿 > 200 ms 才发起 LLM 请求。
- **超时熔断**：400 ms 预算内未返回即丢弃，静默回退本地引擎，用户无感。
- **候选注入**：LLM 结果作为**追加候选**（带标识），永不阻塞常规候选。
- **后端抽象**：`trait LlmBackend` → Ollama（首发）/ OpenAI 兼容 API / candle 内嵌（v2）。
- **指令模式（Agent）**：`//翻译 hello`、`//润色`、`//邮件 催进度` 前缀触发，生成结果进候选栏；预留 tool-calling 接口。
- **上下文收集**：当前应用名、前文若干字符，默认关闭，设置中明示开关。

---

## 7. 桌面应用（Tauri 2）

### 7.1 职责边界（关键认知）

| 层 | 承担者 | 说明 |
|---|---|---|
| 按键输入通道 | **原生 TSF DLL / IMKit / fcitx5**（不可替代） | 操作系统输入法协议只认原生接口，Tauri 无法承担 |
| 候选窗 | **原生实现**（延迟敏感，随光标跟随） | Tauri 版候选窗仅作实验选项，不做默认 |
| 设置界面 | ✅ Tauri | 输入方案、模糊音、快捷键、隐私开关 |
| 词库管理 | ✅ Tauri | 导入（rime-ice/搜狗转换）、导出、用户词编辑、扩展包下载 |
| Agent 控制台 | ✅ Tauri | LLM 后端配置、指令模板管理、调用日志 |
| 引擎调试练习场 | ✅ Tauri | v0.1/v0.2 阶段的可视化 dogfood 界面：输入拼音实时看候选/打分/学习效果 |

### 7.2 集成方式

- Tauri 应用**不内嵌引擎**，作为 novatyped 的 IPC 客户端（与平台薄壳同一套 `protocol` crate），保证引擎单实例、数据单份。
- v0.1 过渡期（daemon 尚未就绪）：Tauri 通过 Rust command 直接调用 `novatype-core`，v0.2 切换到 IPC，接口由 `protocol` crate 统一，前端无感。
- 前端栈：Vite + TypeScript（框架任选 React/Svelte，倾向 Svelte 体积更小）；产物内嵌进二进制。
- 收益：练习场让引擎调优在 v0.1 就有可视化反馈，不必等 v0.3 的 TSF。

### 7.3 UI 设计规范（简化搜狗风格）

最终产品的输入体验对标搜狗，但只做**简化版**——去掉皮肤商城、广告、云推荐等，保留核心三件套：

**① 候选窗（核心，v0.3 原生实现，练习场先做像素级预览）**

```
┌──────────────────────────────────────────────┐
│ zhong'guo'ren                                 │  ← 拼音行（音节用 ' 分隔）
│ 1. 中国人  2. 中国  3. 种  4. 中  5. 忠   ‹ › │  ← 候选行 + 翻页
└──────────────────────────────────────────────┘
```

- 横排单行，默认 5 候选/页，数字键 1–5 上屏，`-`/`=` 或 `‹ ›` 翻页
- 首选项高亮（品牌色 #197B98），空格上屏首选
- 圆角 8px、白底、细边框、轻投影；紧凑（高约 90px），跟随光标
- LLM 追加候选带 ✦ 标识，排在常规候选之后

**② 状态条/托盘（v0.3）**：托盘图标 + 中/英标识，右键菜单（切换方案、打开设置、暂停）；不做搜狗式悬浮长条状态栏

**③ 设置中心（Tauri，v0.5）**：左侧导航（常用 / 按键 / 词库 / AI 助手 / 关于），单窗口，无皮肤系统（仅浅色/深色）

不做的部分：皮肤商城、资讯弹窗、云输入账号体系、表情包面板（v1 后再议）。


---

## 8. 工程结构

```
novatype/
├── Cargo.toml            # workspace
├── crates/
│   ├── core/             # 拼音切分、词图、Viterbi
│   ├── dict/             # FST 编译/加载 + 词库格式转换
│   ├── model/            # bigram + 用户学习模型（redb）
│   ├── protocol/         # IPC 消息定义（serde + bincode）
│   ├── server/           # novatyped 守护进程
│   ├── llm/              # LLM 后端抽象（Ollama/OpenAI 兼容）
│   └── agent/            # 指令解析、prompt 模板
├── platforms/
│   ├── windows-tsf/      # 参考 azooKey-Windows + weasel
│   ├── fcitx5-addon/     # C++ 薄壳 + FFI，参考 fcitx5-rime
│   └── macos-imk/        # objc2 桥接，参考 squirrel
├── apps/
│   └── desktop/          # Tauri 2 桌面应用（设置/词库/Agent/练习场）
│       ├── src-tauri/    # Rust 侧：IPC 客户端 + Tauri commands
│       └── src/          # 前端：Vite + TS (Svelte)
├── tools/                # 词库构建、语料训练、基准测试
├── data/                 # 种子词库源文件
└── installer/            # Inno Setup (Win) / deb / pkg
```

关键依赖：`fst`、`redb`、`serde`+`bincode`、`tokio`（仅 server/llm）、`windows-rs`、`objc2`、`interprocess`、`tauri` v2。

---

## 9. 路线图与验收标准

| 版本 | 范围 | 验收标准 |
|---|---|---|
| **v0.1** 引擎原型 | core + dict + model 骨架；CLI 输入拼音出候选；**Tauri 练习场骨架**（直连 core） | 转换正确率基准集跑通；响应 < 5 ms；纯离线；练习场可视化候选/打分 |
| **v0.2** 学习与联想 | 用户自学习、自动造词、联想；daemon + IPC 协议；**Tauri 切换到 IPC 客户端** | CLI 模拟连续输入，习惯调整可复现；重启不丢数据 |
| **v0.3** Windows 可用 | TSF 薄壳 + 原生候选窗 + 安装器 | 记事本/浏览器/VS Code 日常输入自用（dogfood）两周无崩溃 |
| **v0.4** LLM 接入 | Ollama 后端 + 指令模式 + 熔断降级；**Tauri Agent 控制台** | 断网/杀掉 Ollama 时体验零差异 |
| **v0.5** Linux + 设置 | fcitx5 addon；**Tauri 设置界面 + 词库管理**；词库导入 | fcitx5 下日常可用；rime-ice 一键导入 |
| **v1.0** 三平台正式版 | macOS IMKit；双拼/模糊音；打包合规 | 安装包 ≤ 35 MB；三平台 CI 出包 |
| **v2.0** Agent 深化 | candle 内嵌小模型（重排序）、插件 API、tool-calling | 无 GPU 机器上 rerank < 50 ms |

**开发纪律**：v0.1/v0.2 坚持 CLI 先行——引擎质量用基准测试打磁实之后，才碰平台层（TSF/IMKit 是全项目最大工作量，rakukan 的 README 也自证了这一点）。

---

## 10. 风险登记

| 风险 | 等级 | 缓解 |
|---|---|---|
| TSF/IMKit 适配工作量超预期 | 高 | 抄 azooKey-Windows/weasel/squirrel 实现；out-of-process 隔离故障域 |
| 自训 bigram 质量不足 | 中 | 先用 rime-essay 语言模型数据顶上；语料清洗迭代 |
| rime-ice GPL 传染 | 已解决 | 项目已定 MIT，rime-ice 仅走用户"一键导入"，不随包分发 |
| Windows 未签名 DLL 被杀软误报 | 中 | 预留代码签名预算；发布走 winget/GitHub Release |
| LLM 延迟破坏打字节奏 | 低 | 防抖 + 熔断已是实证方案（Wisdom-Weasel） |
| 目标机缺 WebView2 Runtime | 低 | Win10/11 基本自带；安装器带 bootstrapper 在线补装 |
| 误把候选窗做进 Tauri 导致延迟/跟随问题 | 中 | 职责边界已明确（§7.1）：候选窗保持原生实现 |

---

## 11. 已拍板事项

1. **项目许可证**：MIT。
2. **多平台**：Windows / Linux / macOS 全支持是硬性目标，架构按 §3.1 钢红线约束；**实施 Windows 先行**。
3. **当前执行**：v0.1 引擎原型。
4. **桌面应用**：Tauri 2（职责边界见 §7，待审核确认后实施）。
