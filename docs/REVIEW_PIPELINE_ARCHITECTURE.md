# 成像专项评审系统 · 架构设计

> 依据《成像专项评审流程图》抽象的**领域系统架构**，不绑定任何现有工程实现。  
> 目标：描述「现场采集 → 多通道上传 → 齐套校验 → 批量评审建缺陷 → 后台证据匹配 → 缺陷平台取证」所需的系统边界、组件、数据与关键约束。  
> 日期：2026-07-31  
>  
> **Word：** `docs/REVIEW_PIPELINE_ARCHITECTURE.docx`（含架构图）。  
> 重新生成：`python scripts/export_architecture_docx.py`

---

## 1. 设计目标与边界

### 1.1 要解决的问题

在双机（对比机 / 测试机）成像专项评审中：

1. **任务可达**：有网用在线任务，无网用离线包，机端得到同一套任务卡与场景清单。
2. **现场可采**：按场景采集图片/视频，旁路上传设备日志。
3. **链路可选**：网口直传、USB 共享网直传、PC 暂存再传三种上传路径收敛到同一服务端资产。
4. **齐套可判**：双机、版本、场景、日志齐套后才进入正式评审。
5. **评审可并行**：批量对比与建缺陷不阻塞；证据匹配在上传完成后由服务端后台完成。
6. **开发可取证**：在缺陷平台点问题图即可拉取「时段 log + 对应 dump」。

### 1.2 范围

| 在范围内 | 不在范围内 |
|----------|------------|
| 任务分发 / 离线包 | 手机相机 ISP / 算法训练 |
| 机端采集与上传编排 | 缺陷平台内部工作流引擎实现 |
| 评审对比与缺陷草稿 | 通用 CI / 源码构建系统 |
| 媒体与证据对象存储、匹配 | 组织级账号中心的产品选型细节 |
| 与缺陷平台的对接契约 | 具体 UI 框架、语言、仓库结构 |

### 1.3 设计原则

1. **入口可替换、下游统一**：在线 / 离线只影响「任务包来源」，加载完成后领域模型一致。
2. **通道可替换、落点统一**：A / B1 / B2 只影响传输路径，服务端资产模型一致。
3. **评审与匹配解耦**：匹配由「上传完成」事件触发，不绑在「建缺陷」按钮上。
4. **证据按图寻址**：开发侧以「问题图」为入口拉取关联 dump / log，匹配结果是索引而非拷贝附件。
5. **状态可回退**：齐套失败、通道失败、版本不一致均可回到明确步骤，不静默吞错。

---

## 2. 系统上下文

```mermaid
flowchart TB
  tester[测试同学]
  reviewer[评审同学]
  developer[开发同学]
  ops[现场/实验室支持]

  subgraph sys["成像专项评审系统"]
    capture[机端采集应用]
    pc_client[PC 汇聚工具]
    review[电脑评审端]
    platform[评审平台服务]
  end

  defect[缺陷管理平台]
  idp[账号/权限]
  net[现场网络与线材]

  tester --> capture
  tester --> pc_client
  reviewer --> review
  developer --> defect
  capture --> platform
  pc_client --> platform
  pc_client --> capture
  review --> platform
  platform --> defect
  platform --> idp
  developer -.->|点图取证| platform
  ops --> net
  capture --> net
```

### 外部依赖契约（摘要）

| 外部系统 | 本系统需要它提供 | 本系统向它提供 |
|----------|------------------|----------------|
| 缺陷管理平台 | 创建缺陷、模块字段、指派人、附件/外链、状态 API | 缺陷标题/描述、模块、指派人、媒体引用、证据下载入口 |
| 账号权限 | Token / SSO、下载鉴权 | 用户身份、操作审计主体 |
| 现场网络 | 可达上传入口的 L2/L3 通路 | 无（消费方） |

---

## 3. 逻辑架构（组件）

```mermaid
flowchart TB
  subgraph Clients["客户端层"]
    MA[机端采集 App]
    PC[PC 汇聚工具]
    WEB[任务门户 Web<br/>离线包下载]
    RC[电脑评审端]
  end

  subgraph Edge["接入与传输"]
    UG[上传网关<br/>断点续传 / 鉴权 / 限流]
    DG[设备网关协议<br/>ADB·设备通道抽象]
  end

  subgraph Core["领域服务层"]
    AUTH[认证适配]
    TASK[任务与专项包服务]
    DEVROLE[设备与角色服务]
    CAPPROG[采集进度服务]
    ASSET[资产入库服务]
    READY[齐套校验服务]
    REVIEW[评审会话服务]
    DEFECT[缺陷草稿与提交服务]
    MATCH[证据匹配编排]
  end

  subgraph Async["异步与计算"]
    Q[(任务队列)]
    MW[匹配 Worker]
    IDX[证据索引构建]
  end

  subgraph Data["数据层"]
    META[(元数据库)]
    OBJ[(对象存储<br/>图/视频/log/dump/包)]
    CACHE[(缓存 / 会话)]
  end

  subgraph Ext["外部"]
    DP[缺陷管理平台]
  end

  MA --> AUTH
  MA --> TASK
  MA --> DEVROLE
  MA --> CAPPROG
  MA --> UG
  WEB --> TASK
  PC --> DG
  PC --> UG
  RC --> REVIEW
  RC --> DEFECT
  UG --> ASSET
  ASSET --> READY
  ASSET --> Q
  Q --> MW
  MW --> IDX
  REVIEW --> ASSET
  DEFECT --> DP
  MATCH --> IDX
  TASK --> META
  TASK --> OBJ
  ASSET --> META
  ASSET --> OBJ
  READY --> META
  REVIEW --> META
  DEFECT --> META
  IDX --> META
  IDX --> OBJ
```

### 3.1 组件职责

| 组件 | 职责 | 关键产出 |
|------|------|----------|
| 机端采集 App | 在线/离线加载、标定 baseline/dut、场景采集、发起 A/B1 上传、显示本地进度 | 本地任务卡、本地媒体、上传请求 |
| 任务门户 Web | 浏览专项、导出离线 zip | 离线任务包 |
| PC 汇聚工具 | 设备发现、拉取、命名 album 包、选目录批量上传（B2） | 暂存包、上传批次 |
| 电脑评审端 | 选批次、图/视频对比、建缺陷（自动挂素材）、按模块指派批量提交 | 缺陷草稿、提交批次 |
| 任务与专项包服务 | 任务目录、版本、场景清单、在线下发、离线包签名与校验 | TaskPackage、SceneSpec |
| 设备与角色服务 | 机型/SN/build/ADB 标识、baseline/dut 绑定、双机版本一致性 | DeviceRoleBinding |
| 采集进度服务 | 场景 n/n、总进度、缺项记录 | CaptureProgress |
| 上传网关 | 多协议入口统一、鉴权、分片、幂等、旁路 log 队列 | UploadReceipt |
| 资产入库服务 | 媒体/log/dump 落对象存储、写元数据、关联任务/设备/场景 | Asset、Batch |
| 齐套校验服务 | 双机上传、版本一致、场景齐/认可、log 队列完成 | ReadinessReport |
| 评审会话服务 | 批次视图、对比布局、通过/跳过、问题场景标记 | ReviewSession |
| 缺陷草稿与提交服务 | 自动挂素材、模块归类、指派人、批量推缺陷平台 | DefectDraft → ExternalTicket |
| 证据匹配编排 + Worker | 上传完成后入队；图↔dump、图↔时段 log 等关联 | EvidenceIndex |
| 对象存储 / 元数据库 | 大文件与结构化索引 | 持久化事实源 |

---

## 4. 领域模型（核心实体）

```mermaid
erDiagram
  SPECIALTY ||--o{ TASK_PACKAGE : contains
  TASK_PACKAGE ||--o{ SCENE_SPEC : defines
  TASK_PACKAGE ||--o{ DEVICE_BINDING : loaded_on
  DEVICE_BINDING ||--o{ CAPTURE_ITEM : produces
  SCENE_SPEC ||--o{ CAPTURE_ITEM : satisfies
  BATCH ||--o{ CAPTURE_ITEM : groups
  BATCH ||--o{ UPLOAD_RECEIPT : tracks
  BATCH ||--o| READINESS : checked_by
  BATCH ||--o| MATCH_JOB : triggers
  CAPTURE_ITEM ||--o{ ASSET : materializes
  ASSET ||--o{ EVIDENCE_LINK : indexed_as
  REVIEW_SESSION ||--o{ DEFECT_DRAFT : creates
  DEFECT_DRAFT }o--|| ASSET : attaches
  DEFECT_DRAFT }o--o| EXTERNAL_TICKET : submits_as
  MATCH_JOB ||--o{ EVIDENCE_LINK : outputs

  SPECIALTY {
    string id
    string name
    string module_taxonomy
  }
  TASK_PACKAGE {
    string id
    string version
    string checksum
    enum source "online|offline_zip"
  }
  SCENE_SPEC {
    string id
    int seq
    int required_count
  }
  DEVICE_BINDING {
    string device_id
    string sn
    string model
    string build
    enum role "baseline|dut"
  }
  CAPTURE_ITEM {
    string id
    string scene_id
    enum media_type "image|video"
    datetime captured_at
  }
  BATCH {
    string id
    string specialty_id
    enum status
  }
  ASSET {
    string id
    string storage_key
    enum kind "image|video|log|dump|package"
    string content_hash
  }
  READINESS {
    bool both_devices
    bool version_aligned
    bool scenes_complete_or_waived
    bool log_queue_done
  }
  MATCH_JOB {
    string id
    enum status "queued|running|done|failed"
  }
  EVIDENCE_LINK {
    string asset_id
    string related_asset_id
    enum relation "image_to_dump|image_to_period_log"
    string time_window
  }
  DEFECT_DRAFT {
    string id
    string module
    string assignee
    enum state "draft|submitted"
  }
```

### 4.1 批次状态机（服务端）

```mermaid
stateDiagram-v2
  [*] --> Collecting: 创建批次
  Collecting --> Uploading: 开始上传
  Uploading --> Uploading: 分片/续传/旁路log
  Uploading --> UploadComplete: 本批媒体+log 收齐事件
  UploadComplete --> Matching: 自动入队匹配
  UploadComplete --> ReadyCheck: 齐套检查
  ReadyCheck --> Collecting: 缺采/缺传/版本不一致
  ReadyCheck --> Reviewable: 齐套通过
  Matching --> MatchReady: 索引完成
  Matching --> MatchFailed: 可重试
  Reviewable --> Reviewing: 打开评审会话
  Reviewing --> Reviewing: 对比/建草稿
  Reviewing --> Submitting: 批量提交
  Submitting --> Submitted: 缺陷平台确认
  MatchReady --> EvidenceAvailable: 开发可点图下载
  Submitted --> Closed: 归档
  Submitted --> Collecting: 打回重采
```

要点：**UploadComplete 同时扇出到 Matching 与 ReadyCheck/Review**；Reviewable 不依赖 MatchReady，但 EvidenceAvailable 依赖 MatchReady。

---

## 5. 端到端数据流

```mermaid
flowchart LR
  subgraph Load["① 任务加载"]
    OL[在线: 登录→列表→下发]
    OF[离线: zip→解压]
  end
  subgraph Field["②③ 现场"]
    ROLE[标定 baseline/dut]
    CAP[场景采集 + 本地进度]
  end
  subgraph Up["④ 上传"]
    A[通道 A 网口]
    B1[通道 B1 USB 共享网]
    B2[通道 B2 PC 暂存]
  end
  subgraph Svr["服务端"]
    IN[资产入库]
    RD[齐套]
    RV[评审/缺陷草稿]
    MQ[匹配队列]
  end
  subgraph Out["⑦ 出口"]
    DP[缺陷平台]
    DL[点图下载证据]
  end

  OL --> ROLE
  OF --> ROLE
  ROLE --> CAP
  CAP --> A & B1 & B2
  A & B1 & B2 --> IN
  IN --> RD
  IN --> MQ
  RD --> RV
  RV --> DP
  MQ --> DL
  DP --> DL
```

### 5.1 上传通道作为架构模式

| 通道 | 拓扑 | 客户端职责 | 服务端视角 |
|------|------|------------|------------|
| **A** | Phone → Dock Ethernet → Upload Gateway | 机端排队上传 + 旁路 log | 直收；无 PC 落盘 |
| **B1** | Phone → USB tethering → Gateway | 同 A；PC 仅网关 | 同 A；元数据可记 `via=tether` |
| **B2** | Phone → Device Gateway → PC Staging → Gateway | PC 负责拉取、命名、批量传 | 收的是「已打包 album」；需校验 SN/机型命名 |

统一约束：

- 入库后 **Asset 模型相同**（kind、hash、scene、device、batch）。
- 上传网关提供 **幂等键**（content_hash + batch_id + device_id）。
- 旁路 log 与主媒体可并行队列，齐套时分别判定。

---

## 6. 关键时序

### 6.1 上传完成 → 后台匹配（不阻塞评审）

```mermaid
sequenceDiagram
  participant C as 机端/PC
  participant UG as 上传网关
  participant AS as 资产入库
  participant Q as 队列
  participant W as 匹配 Worker
  participant R as 评审端
  participant D as 缺陷服务

  C->>UG: 最后一包 ACK / 队列清空
  UG->>AS: BatchUploadComplete(batch_id)
  AS->>Q: Enqueue(MatchJob)
  AS-->>R: 批次可开评审(若齐套已过)
  R->>D: 建缺陷草稿(挂 Asset)
  Note over R,D: 不等待 MatchJob
  Q->>W: MatchJob
  W->>W: 图↔dump / 图↔时段log
  W-->>AS: EvidenceIndex Ready
  Note over D: 开发下载时读索引；未就绪则等待/提示
```

### 6.2 缺陷提交与取证

```mermaid
sequenceDiagram
  participant R as 评审端
  participant D as 缺陷服务
  participant DP as 缺陷平台
  participant Dev as 开发
  participant API as 证据 API

  R->>D: 按模块筛选 + 指派 + 批量提交
  D->>DP: CreateTickets(media refs)
  DP-->>D: ticket ids
  Dev->>DP: 打开缺陷 · 点击问题图
  DP->>API: ResolveEvidence(asset_id)
  alt 匹配已完成
    API-->>Dev: 时段 log URL + dump URL
  else 匹配未完成
    API-->>Dev: 202 / 提示等待
  end
```

---

## 7. 部署拓扑（逻辑）

```mermaid
flowchart TB
  subgraph FieldSite["现场 / 实验室"]
    Phones[对比机 + 测试机]
    Dock[拓展坞有线网]
    Laptop[评审/汇聚 PC]
    Phones --> Dock
    Phones --> Laptop
  end

  subgraph DMZ["接入区"]
    LB[负载均衡]
    UG[上传网关集群]
    LB --> UG
  end

  subgraph AppZone["应用区"]
    API[领域 API 集群]
    WRK[匹配 Worker 池]
    API --> WRK
  end

  subgraph DataZone["数据区"]
    DB[(元数据库主从)]
    OS[(对象存储)]
    MQ[(消息队列)]
  end

  subgraph SaaS["外部 SaaS/内网平台"]
    DEF[缺陷管理平台]
  end

  Dock --> LB
  Laptop --> LB
  Laptop --> API
  UG --> OS
  UG --> API
  API --> DB
  API --> OS
  API --> MQ
  WRK --> MQ
  WRK --> OS
  WRK --> DB
  API --> DEF
```

### 环境建议

| 环境 | 说明 |
|------|------|
| 现场弱网 | 优先离线包 + B2；上传网关支持断点续传与队列持久化 |
| 实验室有线 | 优先 A；B1 作备用 |
| 多机并行 | B2 适合 PC 侧编排多设备；服务端按 device_id 隔离批次进度 |

---

## 8. 接口与事件（架构级契约）

### 8.1 同步 API（示意）

| 能力 | 方法示意 | 说明 |
|------|----------|------|
| 任务列表/详情 | `GET /tasks` | 在线加载 |
| 下发/重载包 | `POST /tasks/{id}/load` | 机端拉取场景 |
| 离线包 | `GET /tasks/{id}/package.zip` | 门户下载；含 checksum |
| 上传会话 | `POST /uploads/sessions` | 返回分片策略 |
| 分片/完成 | `PUT /uploads/...` · `POST .../complete` | 幂等 |
| 齐套报告 | `GET /batches/{id}/readiness` | 结构化失败原因 |
| 评审对比集 | `GET /batches/{id}/compare` | baseline vs dut 对齐 |
| 缺陷草稿 | `POST /defects` | 自动附 asset_ids |
| 批量提交 | `POST /defects/submit` | 调缺陷平台 |
| 证据解析 | `GET /assets/{id}/evidence` | 依赖匹配索引 |

### 8.2 领域事件

| 事件 | 触发 | 订阅方 |
|------|------|--------|
| `TaskPackageLoaded` | 机端加载成功 | 设备角色、进度 |
| `CaptureProgressUpdated` | 场景拍摄 | 齐套预检、UI |
| `BatchUploadComplete` | 媒体+约定 log 收齐 | 匹配入队、齐套 |
| `ReadinessPassed` | 齐套服务 | 评审端解锁 |
| `MatchJobCompleted` | Worker | 证据 API、通知 |
| `DefectsSubmitted` | 提交服务 | 审计、报表 |

---

## 9. 非功能需求

| 类别 | 要求 |
|------|------|
| 可靠性 | 上传分片可续传；匹配失败可重试；提交缺陷平台失败可重放且幂等 |
| 一致性 | 双机 TaskPackage.version 不一致则不可进入 Reviewable |
| 性能 | 大批量图/视频对比走对象存储直链或 CDN；匹配 Worker 水平扩展 |
| 安全 | 下载证据鉴权；离线包校验 checksum/签名；SN 缺失包拒绝入库（对应异常回退） |
| 可观测 | 批次维度指标：上传剩余、齐套失败原因分布、匹配队列深度、提交成功率 |
| 离线 | 机端在无业务网时可完成加载与采集；上传仍需对应通道可达 |

---

## 10. 与流程图步骤的映射

| 流程步骤 | 架构落点 |
|----------|----------|
| ① 在线/离线加载 | 任务服务 + 机端加载器 +（可选）任务门户 |
| ② 标定角色 | 设备与角色服务 |
| ③ 场景采集 | 机端 + 采集进度服务 |
| ④ A/B1/B2 | 上传网关 +（B2）PC 汇聚 + 设备网关 |
| ⑤ 齐套 | 齐套校验服务 |
| ⑥ 对比建缺陷 | 评审端 + 评审会话 + 缺陷草稿 |
| ⑥c 后台匹配 | 事件 → 队列 → Worker → 证据索引 |
| ⑦ 模块指派提交 | 缺陷提交服务 → 缺陷平台 |
| 开发点图下载 | 证据 API（经缺陷平台深链） |
| 异常回退 | 各服务返回可操作错误码 + 客户端引导回 ①～④ |

---

## 11. 演进与裁剪建议

**MVP（可先落地）**

1. 在线任务 + 单通道上传（A 或 B2 二选一）  
2. 齐套（双机 + 版本 + 场景）  
3. 评审对比 + 缺陷草稿挂图  
4. 提交缺陷平台  
5. 匹配可先做「弱关联 / 人工时段」再换强算法  

**完整形态**

- 三通道齐全、旁路 log 齐套、自动图↔dump/log 匹配、批量模块指派、全链路可观测与审计。

**刻意不做的事**

- 不要把匹配绑在「建缺陷」事务里。  
- 不要让 B1 在 PC 落业务盘（与 B2 职责混淆）。  
- 不要让在线/离线加载产生两套互不兼容的场景模型。

---

## 12. 文档关系

| 文档 | 内容 |
|------|------|
| `REVIEW_PIPELINE_FLOW.md` | 业务操作流程与决策（What / When） |
| **本文** | 系统边界、组件、数据、事件、部署（How 的架构层） |
| 后续可增补 | API OpenAPI、匹配算法设计、威胁模型、容量规划 |

---

*本架构为流程驱动的系统设计蓝图，实现时可替换技术栈，但应保持：统一任务模型、统一资产模型、上传完成触发匹配、评审与匹配并行。*
