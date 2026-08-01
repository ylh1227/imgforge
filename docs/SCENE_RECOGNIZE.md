# 场景自动识别命名

依据场景列表，用云端视觉 / 大模型 API 识别评审导入图片所属场景，并把**场景名作为前缀**写到原文件名上。

## 命名规则

```text
{场景名}_{原文件名}
```

示例：`IMG_0001.jpg` + 夜景 → `夜景_IMG_0001.jpg`

- 幂等：已是同一场景前缀时不再叠加  
- 换场景：先剥掉已知场景前缀，再加新前缀  
- 未匹配：默认不改名；可在设置中开启「未识别_」前缀  
- 同目录冲突：追加 `__2`、`__3`…

同时会创建/绑定同名评审标签，并在备注写入 `[场景] … | conf=…`。

## 配置

| 项 | 位置 |
|----|------|
| 场景列表 | `~/.imgforge/scene_catalog.json` |
| 识别设置 | `~/.imgforge/scene_recognize_config.json` |
| API Key | 系统钥匙串（优先）或环境变量 `IMGFORGE_VISION_API_KEY`（回退）；**不写配置文件、RPC 不回传明文** |


### 场景列表（表格）

设置页以表格编辑（列：`id` / `name` / `description`），并支持：

- **添加一行 / 删除**
- **外部导入表格**：Excel（`.xlsx` / `.xls`）、CSV、TSV、TXT（自动处理 UTF-8 / UTF-16 BOM）
- **导入模式**：替换当前列表，或合并（同 `id` 覆盖、新 id 追加）
- **导出 CSV**
- 粘贴表格文本再解析

CSV / Excel 首行可为表头，列顺序：`id,name,description`。

Host RPC：

```json
{ "method": "scene.catalog_import_table", "params": { "path": "/path/to/scenes.xlsx", "merge": false } }
```

或 `{ "text": "id,name,description\\nnight,夜景,\\n", "merge": true }`。

底层仍落盘为 `~/.imgforge/scene_catalog.json`。

### 识别设置字段

- `enabled`：是否启用  
- `base_url`：OpenAI 兼容根地址（默认 `https://dashscope.aliyuncs.com/compatible-mode/v1`）  
- `model`：默认 `qwen3-vl-flash`；预设另含 `qwen3-vl-32b-thinking` / `qwen3-vl-235b-a22b-thinking` / `qwen-vl-plus` / `qwen-vl-max`  
- `auto_on_import`：导入后自动识别  
- `prefix_unknown`：未匹配是否加 `未识别_`  
- `timeout_secs` / `max_edge`：超时与缩略图边长  

默认走阿里云百炼通义 VL；也可改成任意 OpenAI Chat Completions 多模态接口（含自建代理）。  
API Base URL **必须 HTTPS**（本地可用 `http://127.0.0.1`）。  
Key 写入 macOS 钥匙串（设置页密码框 →「写入钥匙串」）；云端请求使用 HTTPS Bearer，错误回显会脱敏。环境变量仅作回退，不推荐长期明文写在 shell profile。

## 使用（egui）

1. 评审页 → **场景识别设置** → 粘贴 Key → **写入钥匙串**  
2. 维护场景列表；Base URL / Model 默认已是千问，按需改后保存  
3. 勾选 **导入后场景识别**，或导入后点 **场景识别命名**

## 使用（Flutter）

1. 启动 `ui_flutter/run_macos.sh`  
2. **图片评审** 或 **视频评审** 顶栏 → **场景识别设置** → 粘贴 Key → **写入钥匙串**（不会回显已保存值）  
3. 顶栏 **场景识别命名**，或侧栏勾选 **导入后场景识别**（图片/视频共用）

兼容图片与视频批次：视频会先抽代表帧（约时长 10% 处）再识别，前缀规则相同。

## Host RPC（Flutter）

| 方法 | 说明 |
|------|------|
| `scene.catalog_get` / `scene.catalog_set` | 读写场景列表 |
| `scene.config_get` / `scene.config_set` | 读写识别配置（`has_api_key` 布尔；不含 Key） |
| `scene.api_key_set` / `scene.api_key_clear` | 写入/清除钥匙串（响应不含 Key） |
| `scene.catalog_import_table` | 外部表格导入（path / text + merge） |
| `review.recognize_scenes` | 图片批次 `{ "batch_id": N }` |
| `video.recognize_scenes` | 视频批次 `{ "batch_id": N }`（抽帧后识别） |
| `review.import_folder` / `review.import_paths` | 可选 `auto_recognize: true` |
| `video.import_folder` | 可选 `auto_recognize: true` |

## 重新生成相关代码位置

- [`src/review/scene_recognize/`](../src/review/scene_recognize/)  
- Host：[`src/host/dispatch.rs`](../src/host/dispatch.rs)  
- UI：评审顶栏「场景识别命名 / 设置」
