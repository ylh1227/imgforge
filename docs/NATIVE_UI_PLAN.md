# ImgForge Native UI 更换计划

> 分支：`feat/native-ui`  
> 目标：**一次换壳，完成度高**——新壳上线时五大战场功能可用，而不是 demo。  
> 原则：**Rust 核心不动业务语义；UI 只换壳；先 Host API，再壳。**

---

## 0. 结论（先定死）

### 推荐栈

| 层 | 选择 | 理由 |
|---|---|---|
| 核心 | 现有 Rust crate（抽 `imgforge-host`） | 转换 / 评审 / 视频 / 提取 / SQLite / ffmpeg / mpv 全在这 |
| 壳 | **Flutter Desktop（macOS + Windows）** | 跨平台一次做完；控件精致；比双端原生开发量小很多 |
| 桥接 | **本地 JSON-RPC（stdio 或 Unix/TCP socket）** | 比 FFI 稳、可调试、可双开 egui 对照；视频帧/纹理另走专用通道 |
| 过渡 | egui 与新壳并行，**Host API 验收通过后再切默认入口** | 避免半吊子；回归有对照物 |

**不选 SwiftUI-only**：Windows 会长期落在 egui，谈不上「一次完成」。  
**不选纯 FFI cdylib 作为第一刀**：视频/异步/取消/大对象传参太容易翻车；RPC 先落地，热点再 FFI。  
**不选 Electron**：包体与性能不适合图/视频工具。  
**Tauri 备选**：若团队更熟 Web 前端，可用 Tauri 2 替代 Flutter，Host 仍同一套；本计划默认 Flutter。

### 「完成」定义（验收门槛）

新壳 **Default 可发版** 当且仅当：

1. 五模式齐：格式转换 / 图片评审 / 视频评审 / 数据提取 / 任务中心  
2. 与现 egui **功能清单 1:1**（见 §3），允许视觉重排，不允许缺主路径  
3. macOS arm64 + Windows x64 可打包双击运行  
4. 视频：可导入、列表/卡片、多路对比、播放/暂停/scrub、偏移校准、宫格导出、示波器至少波形+直方图  
5. 转换：批量跑完、进度/取消、日志、预设、设备导入、JIRA/远端开关可用  
6. 数据不丢：SQLite 评审库、GuiPrefs、抽帧缓存路径与现网兼容  
7. `imgforge doctor` + 关键 smoke 用例通过  

---

## 1. 现状摘要

### 入口与模式

- GUI 入口：`src/bin/imgforge-app.rs`（eframe/egui + Glow）  
- 主模式：`AppMode` = Convert | Review | VideoReview | DataExtract | TaskCenter（`src/gui/app_types.rs`）  
- 已有部分原生：`src/gui/native/` macOS Glass 底栏（证明「壳可嵌原生」方向正确）

### UI 体量（约）

| 区域 | 量级 | 耦合点 |
|---|---|---|
| `gui/app/convert.rs` | ~2.3k | 设置状态机、远端、JIRA、ADB、队列 |
| `review/ui/*` | ~7k+ | 标注画布、对比、缩略图纹理 |
| `video_review/ui/*` | ~8k+ | mpv/glow、多路对比、scopes |
| `data_extract/ui/*` | ~2.3k | 表格/导出交互 |
| 合计 UI | **~20k LOC** | 大量逻辑可下沉，但面板内仍混业务 |

### 已相对干净的服务层（应成为 Host）

- `review/service/*`：批次、导出、截图、缩略图、转换桥  
- `video_review/service/*`：ffmpeg、对齐、抽帧缓存、宫格/拼接导出、defect pack  
- `data_extract/service/*`：扫描、汇总、对比、阈值、导出  
- `processing/*`、`scheduler/*`、`mobile/*`、`jira/*`、`remote/*`  
- `gui/prefs.rs`：偏好持久化（壳侧只读写 DTO）

### 难换点（计划必须单独啃）

1. **视频播放**：`video_review/playback/*` 依赖 egui Glow + libmpv  
2. **图片标注画布**：egui 自绘 + 纹理缓存  
3. **示波器**：wgpu/shader + 面板  
4. **后台任务模型**：`gui/async_job.rs` 绑 egui `Context::request_repaint`

---

## 2. 目标架构

```
┌──────────────────────────────────────────────┐
│ Flutter Shell (feat/native-ui)               │
│ 导航 · 表单 · 列表 · 设置 · 进度 · 对话框      │
│ 平台：文件选择 / DnD / 开文件夹 / 通知         │
└─────────────────┬────────────────────────────┘
                  │ JSON-RPC 请求/响应 + 事件流
                  │ (progress / log / job state)
┌─────────────────▼────────────────────────────┐
│ imgforge-host (Rust)                         │
│ 命令：convert / review / video / extract /   │
│       tasks / prefs / doctor / mobile / jira │
│ 媒体：mpv 会话、抽帧、导出（不经 Flutter 解码） │
└─────────────────┬────────────────────────────┘
                  │
     ┌────────────┼────────────┐
     ▼            ▼            ▼
  SQLite      ffmpeg/mpv    ADB/fs
  prefs       frame cache   remote/JIRA
```

### 进程模型（推荐）

1. **Flutter 主进程**启动后拉起 **`imgforge-host` 子进程**（或同包 sidecar）  
2. Host 生命周期与 App 绑定；崩溃可重启并回报  
3. 开发期：egui 也可连同一 Host（对照回归）——可选，Phase 2 再做  

### 媒体特例

| 能力 | 策略 |
|---|---|
| 列表封面 / 悬停预览 | Host 抽帧 → JPEG/PNG 路径或 bytes → Flutter `Image` |
| 审片主预览 / 多路同步 | Host 管 mpv；Flutter 用 **纹理桥**（macOS IOSurface / Windows D3D/shared handle）或先用「逐帧贴图」保功能，再升纹理桥 |
| 示波器 | Host 算好 bitmap/mesh 数据 → Flutter CustomPainter；或 Host 出 PNG |
| 图片评审标注 | Phase A：Flutter `CustomPainter` 重做标注交互，调用 Host 存盘；ROI/导出仍走 service |

**完成度优先顺序**：先「功能正确」（帧贴图/文件预览），再「丝滑纹理桥」。验收允许第一版主预览 ≥15fps 可 scrub，不要求 egui 同级 GPU 合成。

---

## 3. 功能清单（1:1 验收表）

### 3.1 格式转换

- [ ] 输入/输出目录选择、拖放文件夹  
- [ ] 格式、质量、递归、目录结构、覆盖、去元数据  
- [ ] RAW/Bayer、亮度匹配（模式/参考图/预览）  
- [ ] 重命名模板与预览、目标体积  
- [ ] 预设增删改用  
- [ ] 开始 / 取消 / 进度 / 日志 / 打开输出目录  
- [ ] 失败重试、转换队列 → 图片评审  
- [ ] 设备导入（自动/挂载/ADB 多设备）  
- [ ] 远端优先执行、远端状态快照  
- [ ] JIRA 配置探测与批量相关入口  

### 3.2 图片评审

- [ ] 批次导入、侧栏列表、状态/标签/备注  
- [ ] 主画布缩放平移、标注（至少线/框/文字与现主路径一致）  
- [ ] 多图对比、ROI、导出 CSV/JSON  
- [ ] 与转换队列联动、烧录标注选项（若现有）  
- [ ] 快捷键主路径  

### 3.3 视频评审

- [ ] 文件夹导入、元数据、列表/卡片  
- [ ] 时间轴、播放/暂停、scrub  
- [ ] 2–6 路对比布局、Solo/Wipe/叠化（按现有能力）  
- [ ] 偏移校准（快速/标准/精细）  
- [ ] 宫格 PNG、对比拼接 MP4（高质量/无损）  
- [ ] 状态/标签/时间点/片段备注、批量操作  
- [ ] 示波器：波形 + 直方图（矢量示波器可列为 P1 但尽量同发）  
- [ ] 导出报告 CSV/JSON、缓存清理入口  

### 3.4 数据提取

- [ ] 扫描 Imatest 结果、模块汇总/对比/阈值  
- [ ] 导出 CSV/JSON/HTML  
- [ ] OCR 入口（系统有 tesseract 时）  

### 3.5 任务中心

- [ ] 转换历史、失败重试、模块操作日志（与 `GuiPrefs` / task_center 一致）  

### 3.6 全局

- [ ] 偏好持久化兼容  
- [ ] Doctor / 依赖缺失友好提示（ffmpeg、mpv、adb）  
- [ ] 崩溃日志路径与现行为接近  
- [ ] 中文 UI 完整（系统/Flutter 字体，无 tofu）  

---

## 4. Host API 草图

统一信封：

```json
{ "id": "uuid", "method": "video.import_folder", "params": { ... } }
```

事件（单向）：

```json
{ "event": "job.progress", "job_id": "...", "current": 3, "total": 10, "message": "..." }
{ "event": "log.append", "line": "..." }
{ "event": "video.frame", "slot": 0, "pts_ms": 1234, "path": "/tmp/..." }
```

### 方法分组（实现时拆 crate 模块，不拆语义）

| 前缀 | 职责 |
|---|---|
| `app.*` | 版本、doctor、打开路径、取消作业 |
| `prefs.*` | get/set GuiPrefs DTO |
| `convert.*` | preview、run、cancel、presets |
| `mobile.*` | list_devices、pull |
| `remote.*` / `jira.*` | status、submit、probe |
| `review.*` | batches、items、annotations、export、queue_import |
| `video.*` | import、list、playback、compare、align、export、scopes、cache |
| `extract.*` | scan、query、compare、export |
| `tasks.*` | history、retry |

**硬规则**：面板里的业务判断迁到 Host；Flutter 只发意图、渲染状态。

---

## 5. 实施阶段（每阶段可验收，整体一次合并发版）

> 「一次做完」= **一条发版列车**，中间可合并子 PR，但 **不切换用户默认入口**，直到 §0 验收全绿。

### Phase 0 — Host 抽取（约 1–1.5 周）★ 阻塞项

1. 新增 `src/bin/imgforge-host.rs` + `src/host/`（JSON-RPC loop）  
2. 从 `ImgforgeApp` / 各 Panel 抽出纯函数式 facade（不引用 egui）  
3. 作业模型：`JobHandle` + progress/cancel 事件（替换对 egui Context 的依赖）  
4. 契约测试：用脚本打 `convert.run` 小样本、`video.import_folder`、`review` 读写  
5. **egui 暂改调用 facade**（仍 egui 渲染）——证明解耦，降低切壳风险  

**出口**：无 UI 也能跑通转换 + 视频导入 + 评审读写。

### Phase 1 — Flutter 壳骨架（约 3–5 天）

1. `ui_flutter/`（或独立 repo 子目录）桌面工程  
2. 启动 Host、重连、全局错误条  
3. 五 Tab 导航空壳 + 设计 tokens（间距/字体/色，避开通用 AI 紫白风；工具密度）  
4. 打包脚本：macOS / Windows 带上 host 二进制与说明  

**出口**：空壳可切换 Tab，Host doctor 显示正常。

### Phase 2 — 转换 + 任务中心（约 1 周）

完整实现 §3.1 + §3.5。  
**出口**：日常转换工作流可完全离开 egui。

### Phase 3 — 数据提取（约 3–4 天）

§3.4 全做。表格用 Flutter DataTable/自定义，导出走 Host。  
**出口**：提取模块可离开 egui。

### Phase 4 — 图片评审（约 1.5–2 周）

§3.2；标注画布是关键路径。  
**出口**：评审主路径可离开 egui（高级手感可迭代，功能不能缺）。

### Phase 5 — 视频评审（约 2–3 周）★ 最长

§3.3；先文件帧预览保验收，并行做纹理桥增强。  
**出口**：视频模块达 §0 验收；示波器至少 2 种。

### Phase 6 — 打磨与切流（约 3–5 天）

1. 快捷键、拖放、空态、错误文案、依赖缺失引导  
2. 性能：大目录列表虚拟滚动、缩略图缓存上限  
3. `imgforge-app` 改为启动 Flutter 壳（或保留 `--legacy-egui`）  
4. README / 发布物更新；CI 增加 host 契约测试 + Flutter build  

**出口**：默认用户路径 = 新壳；egui 降级为 legacy。

---

## 6. 仓库与协作

| 项 | 约定 |
|---|---|
| 分支 | `feat/native-ui`（本计划） |
| 子 PR | `feat/native-ui-host`、`...-flutter-shell`、`...-convert`… 合入本分支 |
| 禁止 | 在未抽 Host 前直接大面积写 Flutter 调内部 Rust 结构体 |
| 保留 | egui 代码直到 Phase 6；删除另开清理 PR |
| 不提交 | 调试图 `cand_*.png` / `tone_*` / `.cursor/` |

---

## 7. 风险与缓释

| 风险 | 缓释 |
|---|---|
| 视频播放达不到 egui 手感 | 验收定「可用 scrub」；纹理桥作增强项不挡发版 |
| UI  entangle 低估 | Phase 0 强制 egui 改走 facade，暴露隐藏依赖 |
| 双端打包复杂 | 早期 CI 就编 Flutter+host；文档写清 ffmpeg/mpv 依赖 |
| 范围膨胀 | §3 清单外需求进 backlog，不进首发列车 |
| 一人带宽 | 严格按 Phase 串行主路径；Host 与 Flutter 可两人并行但契约先冻 |

---

## 8. 工作量粗估

| 阶段 | 人周（熟手） |
|---|---|
| Phase 0 Host | 1–1.5 |
| Phase 1 壳 | 0.5–1 |
| Phase 2 转换+任务 | 1 |
| Phase 3 提取 | 0.5 |
| Phase 4 图片评审 | 1.5–2 |
| Phase 5 视频 | 2–3 |
| Phase 6 切流 | 0.5–1 |
| **合计** | **约 7–10 人周** |

「一次做完、完成度高」≈ **两个月内一条列车发版**（单人偏上限；双人可压到 ~5–7 周）。

---

## 9. 立即下一步（本周）

1. **确认栈**：Flutter（默认）还是改 Tauri —— 确认后不回头  
2. 冻结 §3 清单（有异议现在改）  
3. 开工 Phase 0：`src/host/` + `imgforge-host` + 3 个契约测试  
4. 同步删除或忽略无关分支 `cursor/setup-dev-environment-c22b`（可选）  

---

## 10. 决策记录

| 日期 | 决策 |
|---|---|
| 2026-07-28 | 分支 `feat/native-ui`；核心 Rust 保留；换壳；追求高完成度一次发版 |
| 2026-07-28 | 计划默认壳 = Flutter Desktop；桥 = JSON-RPC Host；egui 对照至切流 |
| 2026-07-28 | **壳选型锁定 Flutter Desktop**（Mac+Win 一次做完、控件精致、适合工具密度 UI）；Tauri 仅作团队强 Web 时的备选 |
