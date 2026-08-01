# 通义千问（百炼）场景识别接入计划

> 目标：把 ImgForge「场景列表 → 云端看图分类 → 文件名前缀」接到阿里云百炼 **OpenAI 兼容** 多模态接口。  
> 日期：2026-07-31  
> 现状：识别链路已实现（[`src/review/scene_recognize/`](../src/review/scene_recognize/)），默认仍指向 OpenAI；本计划改为千问并完成联调验收。

---

## 1. 接入结论（已拍板）

| 项 | 选择 |
|----|------|
| 平台 | 阿里云百炼 DashScope |
| 协议 | OpenAI Compatible：`…/compatible-mode/v1/chat/completions` |
| 日常模型 | `qwen3-vl-flash`（最便宜看图） |
| 高精度备选 | `qwen-vl-plus` 或 `qwen-vl-max` |
| Key | 环境变量 `IMGFORGE_VISION_API_KEY` = 百炼 API Key |
| 默认 Base URL | `https://dashscope.aliyuncs.com/compatible-mode/v1` |

价格参考（中国内地，元/百万 Token，以[官方价目](https://help.aliyun.com/zh/model-studio/model-pricing)为准）：

- `qwen3-vl-flash`：输入约 0.15 / 输出约 1.5（≤32K）
- `qwen-vl-plus`：输入约 0.8 / 输出约 2
- `qwen-vl-max`：输入约 1.6 / 输出约 4

---

## 2. 账号与密钥（人工步骤）

1. 开通 [阿里云百炼](https://bailian.console.aliyun.com/)
2. 确认可用视觉模型（至少开通 `qwen3-vl-flash`）
3. 创建 API Key（华北2 北京地域常用）
4. 本机设置：

```bash
export IMGFORGE_VISION_API_KEY='sk-...'
```

5. （可选）写入 shell profile，避免每次打开终端丢失

**注意**：Key 不进仓库、不写 `scene_recognize_config.json`。

---

## 3. 产品配置（用户/设置页）

评审 → **场景识别设置**：

| 字段 | 值 |
|------|-----|
| 启用场景识别 | 开 |
| API Base URL | `https://dashscope.aliyuncs.com/compatible-mode/v1` |
| Model | `qwen3-vl-flash`（抽检可改 `qwen-vl-max`） |
| 导入后自动识别 | 按需 |
| 场景列表 | 表格维护或外部导入 CSV/Excel |

保存后落盘：`~/.imgforge/scene_recognize_config.json` + `scene_catalog.json`。

---

## 4. 工程改造（代码）

### 4.1 默认值切换（必做）

[`src/review/scene_recognize/config.rs`](../src/review/scene_recognize/config.rs)：

- `default_base_url` → 百炼 compatible-mode
- `default_model` → `qwen3-vl-flash`

同步改 UI 初始占位文案与 [`docs/SCENE_RECOGNIZE.md`](SCENE_RECOGNIZE.md)。

### 4.2 预设快捷选项（建议）

设置页增加下拉「服务商预设」：

| 预设 | base_url | model |
|------|----------|-------|
| 通义千问·VL 32B Thinking | dashscope compatible-mode/v1 | `qwen3-vl-32b-thinking` |
| 通义千问·VL 235B Thinking | 同上 | `qwen3-vl-235b-a22b-thinking` |
| 通义千问·Flash | dashscope compatible-mode/v1 | `qwen3-vl-flash` |
| 通义千问·Plus | 同上 | `qwen-vl-plus` |
| 通义千问·Max | 同上 | `qwen-vl-max` |
| 自定义 | 用户填写 | 用户填写 |

选预设只改 draft 字段，仍要点「保存设置」。

### 4.3 兼容性核对（必做）

现有客户端已发：

- `POST {base_url}/chat/completions`
- `Bearer` + `response_format: json_object`
- `image_url` data URL（JPEG base64）

联调时验证百炼是否完整支持 `response_format=json_object`；若不支持：

- 去掉该字段，依赖 prompt「只输出 JSON」+ 现有 `extract_json_object` 容错（已有）

### 4.4 非目标（本期不做）

- 原生 DashScope SDK / 多厂商自动 failover
- 思考模式（thinking）默认开启（成本高；场景分类不需要）
- 国际站新加坡 endpoint（默认走国内）

---

## 5. 联调与验收

### 5.1 冒烟（1～3 张图）

1. 配置 3 条场景（如 night/夜景、portrait/人像、indoor/室内）
2. 导入样例图 → **场景识别命名**
3. 期望：文件变为 `夜景_xxx.jpg` 等；备注含 `[场景]`；失败数 = 0

### 5.2 批量（建议 50 张）

- 统计匹配率、耗时、百炼控制台 Token/费用
- Flash 不准时，同批抽 10 张用 `qwen-vl-max` 对比

### 5.3 失败路径

| 现象 | 处理 |
|------|------|
| 未设置 Key | UI 红字提示 `IMGFORGE_VISION_API_KEY` |
| HTTP 401 | Key / 地域错误 |
| 模型不存在 | 控制台开通对应 Model ID |
| JSON 解析失败 | 关掉 json_object 或换 plus/max；看日志截断响应 |

Host 可选验收：

```json
{ "method": "scene.config_set", "params": {
  "enabled": true,
  "base_url": "https://dashscope.aliyuncs.com/compatible-mode/v1",
  "model": "qwen3-vl-flash"
}}
{ "method": "review.recognize_scenes", "params": { "batch_id": 1 }}
```

---

## 6. 实施顺序

| 步骤 | 内容 | 产出 |
|------|------|------|
| A | 账号 + Key + 本机环境变量 | 可调通 chat/completions |
| B | 改默认 base_url/model + 文档 | ✅ 已完成（默认千问） |
| C | 设置页「通义预设」下拉 | ✅ Flutter 设置对话框已含预设 |
| D | 样例图冒烟 + 50 张批量 | 准确率/费用记录 |
| E | （可选）json_object 兼容兜底开关 | 适配百炼参数差异 |

预估工程量：B+C 约 0.5～1 人日；D 依赖样例与账号。

---

## 7. 回滚

把设置改回：

- `base_url=https://api.openai.com/v1`
- `model=gpt-4o-mini`
- Key 换成 OpenAI

或删除 `~/.imgforge/scene_recognize_config.json` 后按新默认重建（改默认后即千问）。

---

## 8. 文档与对外说明

- 更新 `SCENE_RECOGNIZE.md`：千问默认配置、价目链接、预设表
- README / 评审帮助：一句「场景识别默认通义千问 VL，需配置百炼 API Key」

---

*确认本计划后，按步骤 B→C→D 改代码并联调。*
