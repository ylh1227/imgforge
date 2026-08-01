import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../host/host_controller.dart';
import '../widgets/glass_dropdown.dart';
import '../widgets/liquid_glass.dart';
import '../widgets/section_card.dart';

/// Opens the scene-recognize settings dialog (Host RPC).
Future<void> showSceneRecognizeSettings(BuildContext context) {
  return showDialog<void>(
    context: context,
    barrierDismissible: true,
    builder: (_) => const SceneRecognizeSettingsDialog(),
  );
}

class SceneRecognizeSettingsDialog extends StatefulWidget {
  const SceneRecognizeSettingsDialog({super.key});

  @override
  State<SceneRecognizeSettingsDialog> createState() =>
      _SceneRecognizeSettingsDialogState();
}

class _SceneRow {
  _SceneRow({String id = '', String name = '', String description = ''})
      : id = TextEditingController(text: id),
        name = TextEditingController(text: name),
        description = TextEditingController(text: description);

  final TextEditingController id;
  final TextEditingController name;
  final TextEditingController description;

  void dispose() {
    id.dispose();
    name.dispose();
    description.dispose();
  }

  Map<String, dynamic> toJson() => {
        'id': id.text.trim(),
        'name': name.text.trim(),
        if (description.text.trim().isNotEmpty)
          'description': description.text.trim(),
      };
}

class _SceneRecognizeSettingsDialogState
    extends State<SceneRecognizeSettingsDialog> {
  /// 预设 label = 百炼控制台「模型 Code」/ Model ID（与官网一致）。
  /// https://help.aliyun.com/zh/model-studio/vision-model
  static const _dashScope =
      'https://dashscope.aliyuncs.com/compatible-mode/v1';
  static const _presets = <({String label, String baseUrl, String model})>[
    // Qwen3.7 / 3.6（原生多模态）
    (label: 'qwen3.7-flash', baseUrl: _dashScope, model: 'qwen3.7-flash'),
    (label: 'qwen3.7-plus', baseUrl: _dashScope, model: 'qwen3.7-plus'),
    (label: 'qwen3.6-flash', baseUrl: _dashScope, model: 'qwen3.6-flash'),
    (label: 'qwen3.6-plus', baseUrl: _dashScope, model: 'qwen3.6-plus'),
    // Qwen3-VL
    (label: 'qwen3-vl-flash', baseUrl: _dashScope, model: 'qwen3-vl-flash'),
    (label: 'qwen3-vl-plus', baseUrl: _dashScope, model: 'qwen3-vl-plus'),
  ];

  bool loading = true;
  bool saving = false;
  String? status;
  String? error;

  bool enabled = false;
  bool autoOnImport = false;
  bool prefixUnknown = false;
  bool enableThinking = false;
  bool hasApiKey = false;
  bool apiKeyInKeychain = false;
  bool importMerge = false;
  int concurrency = 4;
  int maxEdge = 640;

  final baseUrlCtrl = TextEditingController();
  final modelCtrl = TextEditingController();
  final pasteCtrl = TextEditingController();
  final apiKeyCtrl = TextEditingController();
  bool obscureApiKey = true;

  String? presetLabel;
  final rows = <_SceneRow>[];

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _load());
  }

  @override
  void dispose() {
    baseUrlCtrl.dispose();
    modelCtrl.dispose();
    pasteCtrl.dispose();
    apiKeyCtrl.dispose();
    for (final r in rows) {
      r.dispose();
    }
    super.dispose();
  }

  void _syncPresetLabel() {
    final base = baseUrlCtrl.text.trim();
    final model = modelCtrl.text.trim();
    for (final p in _presets) {
      if (p.baseUrl == base && p.model == model) {
        presetLabel = p.label;
        return;
      }
    }
    presetLabel = null;
  }

  Future<void> _load() async {
    final host = context.read<HostController>();
    setState(() {
      loading = true;
      error = null;
    });
    try {
      final cfg = await host.call('scene.config_get');
      final cat = await host.call('scene.catalog_get');
      for (final r in rows) {
        r.dispose();
      }
      rows.clear();
      final scenes = (cat['scenes'] as List?) ?? const [];
      for (final raw in scenes) {
        final s = (raw as Map).cast<String, dynamic>();
        rows.add(
          _SceneRow(
            id: s['id']?.toString() ?? '',
            name: s['name']?.toString() ?? '',
            description: s['description']?.toString() ?? '',
          ),
        );
      }
      enabled = cfg['enabled'] == true;
      autoOnImport = cfg['auto_on_import'] == true;
      prefixUnknown = cfg['prefix_unknown'] == true;
      enableThinking = cfg['enable_thinking'] == true;
      concurrency = (cfg['concurrency'] as num?)?.toInt().clamp(1, 16) ?? 4;
      maxEdge = (cfg['max_edge'] as num?)?.toInt().clamp(256, 1280) ?? 640;
      hasApiKey = cfg['has_api_key'] == true;
      apiKeyInKeychain = cfg['api_key_in_keychain'] == true;
      baseUrlCtrl.text = cfg['base_url']?.toString() ??
          'https://dashscope.aliyuncs.com/compatible-mode/v1';
      modelCtrl.text = cfg['model']?.toString() ?? 'qwen3.7-flash';
      apiKeyCtrl.clear();
      _syncPresetLabel();
      setState(() {
        loading = false;
        status = '已加载';
      });
    } catch (e) {
      setState(() {
        loading = false;
        error = e.toString();
      });
    }
  }

  Future<void> _save() async {
    final host = context.read<HostController>();
    setState(() {
      saving = true;
      error = null;
    });
    try {
      final scenes = rows
          .map((r) => r.toJson())
          .where(
            (s) =>
                (s['id'] as String).isNotEmpty ||
                (s['name'] as String).isNotEmpty,
          )
          .toList();
      await host.call('scene.catalog_set', {
        'catalog': {'scenes': scenes},
      });
      await host.call('scene.config_set', {
        'enabled': enabled,
        'base_url': baseUrlCtrl.text.trim(),
        'model': modelCtrl.text.trim(),
        'auto_on_import': autoOnImport,
        'prefix_unknown': prefixUnknown,
        'enable_thinking': enableThinking,
        'concurrency': concurrency,
        'max_edge': maxEdge,
        'thinking_budget': 32,
      });
      // Reload so empty/invalid rows are dropped and has_api_key stays fresh.
      await _load();
      if (!mounted) return;
      setState(() {
        saving = false;
        status = '设置已保存';
      });
    } catch (e) {
      setState(() {
        saving = false;
        error = e.toString();
      });
    }
  }

  Future<void> _importTableFile() async {
    final picked = await FilePicker.platform.pickFiles(
      dialogTitle: '导入场景表格',
      type: FileType.custom,
      allowedExtensions: const ['xlsx', 'xls', 'xlsm', 'csv', 'tsv', 'txt'],
    );
    final path = picked?.files.single.path;
    if (path == null) return;
    await _importViaHost(path: path);
  }

  Future<void> _importPaste() async {
    final text = pasteCtrl.text.trim();
    if (text.isEmpty) return;
    await _importViaHost(text: text);
    pasteCtrl.clear();
  }

  Future<void> _importViaHost({String? path, String? text}) async {
    final host = context.read<HostController>();
    setState(() {
      error = null;
      status = '导入中…';
    });
    try {
      final params = <String, dynamic>{'merge': importMerge};
      if (path != null) params['path'] = path;
      if (text != null) params['text'] = text;
      final res = await host.call('scene.catalog_import_table', params);
      await _load();
      if (!mounted) return;
      setState(() {
        status =
            '已导入 ${res['imported']} 条，当前共 ${res['count']} 条（已写入磁盘）';
      });
    } catch (e) {
      setState(() => error = e.toString());
    }
  }

  Future<void> _exportCsv() async {
    final buf = StringBuffer('id,name,description\n');
    for (final r in rows) {
      final id = r.id.text.trim();
      final name = r.name.text.trim();
      final desc = r.description.text.trim();
      if (id.isEmpty && name.isEmpty) continue;
      buf.writeln('${_csv(id)},${_csv(name)},${_csv(desc)}');
    }
    final path = await FilePicker.platform.saveFile(
      dialogTitle: '导出场景 CSV',
      fileName: 'scene_catalog.csv',
      type: FileType.custom,
      allowedExtensions: const ['csv'],
    );
    if (path == null) return;
    try {
      final out = path.endsWith('.csv') ? path : '$path.csv';
      await File(out).writeAsString(buf.toString());
      setState(() => status = '已导出 $out');
    } catch (e) {
      setState(() => error = e.toString());
    }
  }

  String _csv(String s) {
    if (s.contains(',') || s.contains('"') || s.contains('\n')) {
      return '"${s.replaceAll('"', '""')}"';
    }
    return s;
  }

  Future<void> _storeApiKey() async {
    final key = apiKeyCtrl.text.trim();
    if (key.isEmpty) {
      setState(() => error = '请先粘贴 API Key，再写入钥匙串');
      return;
    }
    final host = context.read<HostController>();
    setState(() {
      error = null;
      status = '正在写入钥匙串…';
    });
    try {
      await host.call('scene.api_key_set', {'api_key': key});
      apiKeyCtrl.clear();
      await _load();
      if (!mounted) return;
      setState(() => status = 'API Key 已写入系统钥匙串（不会回显明文）');
    } catch (e) {
      setState(() => error = e.toString());
    }
  }

  Future<void> _clearApiKey() async {
    final host = context.read<HostController>();
    try {
      await host.call('scene.api_key_clear');
      apiKeyCtrl.clear();
      await _load();
      if (!mounted) return;
      setState(() => status = '已清除钥匙串中的 API Key');
    } catch (e) {
      setState(() => error = e.toString());
    }
  }

  void _applyPreset(String? label) {
    if (label == null) return;
    final p = _presets.firstWhere((e) => e.label == label);
    setState(() {
      presetLabel = label;
      baseUrlCtrl.text = p.baseUrl;
      modelCtrl.text = p.model;
    });
  }

  void _addRow() {
    setState(() => rows.add(_SceneRow()));
  }

  void _removeRow(int i) {
    setState(() {
      rows.removeAt(i).dispose();
    });
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final maxH = MediaQuery.sizeOf(context).height * 0.88;

    return Dialog(
      insetPadding: const EdgeInsets.symmetric(horizontal: 28, vertical: 24),
      backgroundColor: Colors.transparent,
      child: ConstrainedBox(
        constraints: BoxConstraints(maxWidth: 760, maxHeight: maxH),
        child: Material(
          color: scheme.surface.withValues(alpha: 0.96),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(22),
            side: BorderSide(color: scheme.outlineVariant.withValues(alpha: 0.35)),
          ),
          clipBehavior: Clip.antiAlias,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(20, 16, 12, 8),
                child: Row(
                  children: [
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            '场景识别设置',
                            style: Theme.of(context).textTheme.titleLarge,
                          ),
                          const SizedBox(height: 4),
                          Text(
                            '识别结果前缀写入文件名：场景名_原文件名。API Key 写入系统钥匙串，不落盘、不回显。',
                            style: Theme.of(context).textTheme.bodySmall?.copyWith(
                                  color: scheme.onSurfaceVariant,
                                ),
                          ),
                        ],
                      ),
                    ),
                    IconButton(
                      tooltip: '关闭',
                      onPressed: () => Navigator.of(context).pop(),
                      icon: const Icon(Icons.close),
                    ),
                  ],
                ),
              ),
              if (loading)
                const Expanded(child: Center(child: CircularProgressIndicator()))
              else
                Expanded(
                  child: ListView(
                    padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
                    children: [
                      SectionCard(
                        title: '服务',
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            SwitchListTile(
                              contentPadding: EdgeInsets.zero,
                              title: const Text('启用场景识别'),
                              value: enabled,
                              onChanged: (v) => setState(() => enabled = v),
                            ),
                            SwitchListTile(
                              contentPadding: EdgeInsets.zero,
                              title: const Text('导入后自动识别命名'),
                              value: autoOnImport,
                              onChanged: (v) => setState(() => autoOnImport = v),
                            ),
                            SwitchListTile(
                              contentPadding: EdgeInsets.zero,
                              title: const Text('未匹配时加「未识别_」前缀'),
                              value: prefixUnknown,
                              onChanged: (v) =>
                                  setState(() => prefixUnknown = v),
                            ),
                            SwitchListTile(
                              contentPadding: EdgeInsets.zero,
                              title: const Text('启用深度思考（更慢）'),
                              subtitle: const Text(
                                '场景分类建议关闭；Thinking 模型关闭后可快数倍',
                                style: TextStyle(fontSize: 12),
                              ),
                              value: enableThinking,
                              onChanged: (v) =>
                                  setState(() => enableThinking = v),
                            ),
                            ListTile(
                              contentPadding: EdgeInsets.zero,
                              title: Text('并发请求：$concurrency'),
                              subtitle: Slider(
                                value: concurrency.toDouble(),
                                min: 1,
                                max: 8,
                                divisions: 7,
                                label: '$concurrency',
                                onChanged: (v) =>
                                    setState(() => concurrency = v.round()),
                              ),
                            ),
                            ListTile(
                              contentPadding: EdgeInsets.zero,
                              title: Text('缩略图边长：$maxEdge'),
                              subtitle: Slider(
                                value: maxEdge.toDouble(),
                                min: 256,
                                max: 1024,
                                divisions: 6,
                                label: '$maxEdge',
                                onChanged: (v) =>
                                    setState(() => maxEdge = v.round()),
                              ),
                            ),
                            const SizedBox(height: 4),
                            Builder(
                              builder: (context) {
                                final selected = presetLabel != null &&
                                        _presets.any((p) => p.label == presetLabel)
                                    ? presetLabel!
                                    : '自定义';
                                return GlassDropdownButtonFormField<String>(
                                  key: ValueKey('preset-$selected'),
                                  value: selected,
                                  items: [
                                    for (final p in _presets)
                                      DropdownMenuItem(
                                        value: p.label,
                                        child: Text(p.label),
                                      ),
                                    const DropdownMenuItem(
                                      value: '自定义',
                                      child: Text('自定义'),
                                    ),
                                  ],
                                  onChanged: (label) {
                                    if (label == null || label == '自定义') {
                                      setState(() => presetLabel = null);
                                      return;
                                    }
                                    _applyPreset(label);
                                  },
                                  decoration: const InputDecoration(
                                    labelText: '服务商预设',
                                    hintText: '自定义时可直接改下方字段',
                                  ),
                                );
                              },
                            ),
                            const SizedBox(height: 8),
                            TextField(
                              controller: baseUrlCtrl,
                              onChanged: (_) => setState(_syncPresetLabel),
                              decoration: const InputDecoration(
                                labelText: 'API Base URL',
                              ),
                            ),
                            const SizedBox(height: 8),
                            TextField(
                              controller: modelCtrl,
                              onChanged: (_) => setState(_syncPresetLabel),
                              decoration: const InputDecoration(
                                labelText: 'Model',
                              ),
                            ),
                            const SizedBox(height: 12),
                            Text('API Key', style: Theme.of(context).textTheme.labelLarge),
                            const SizedBox(height: 4),
                            Text(
                              '仅用于一次性写入钥匙串；保存设置不会提交 Key。云端调用走 HTTPS Bearer。',
                              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                                    color: scheme.onSurfaceVariant,
                                  ),
                            ),
                            const SizedBox(height: 8),
                            TextField(
                              controller: apiKeyCtrl,
                              obscureText: obscureApiKey,
                              enableSuggestions: false,
                              autocorrect: false,
                              decoration: InputDecoration(
                                labelText: '粘贴新 Key（不会回显已保存值）',
                                suffixIcon: IconButton(
                                  tooltip: obscureApiKey ? '显示' : '隐藏',
                                  onPressed: () =>
                                      setState(() => obscureApiKey = !obscureApiKey),
                                  icon: Icon(
                                    obscureApiKey
                                        ? Icons.visibility_outlined
                                        : Icons.visibility_off_outlined,
                                  ),
                                ),
                              ),
                            ),
                            const SizedBox(height: 8),
                            Wrap(
                              spacing: 8,
                              runSpacing: 8,
                              children: [
                                GlassCapsuleButton(
                                  label: '写入钥匙串',
                                  primary: true,
                                  onPressed: _storeApiKey,
                                ),
                                GlassCapsuleButton(
                                  label: '清除钥匙串 Key',
                                  onPressed: apiKeyInKeychain ? _clearApiKey : null,
                                ),
                              ],
                            ),
                            const SizedBox(height: 10),
                            Text(
                              hasApiKey
                                  ? (apiKeyInKeychain
                                      ? '已配置 Key（系统钥匙串）'
                                      : '已配置 Key（环境变量回退）')
                                  : '未配置 API Key',
                              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                                    color: hasApiKey
                                        ? const Color(0xFF248A3D)
                                        : scheme.error,
                                  ),
                            ),
                          ],
                        ),
                      ),
                      SectionCard(
                        title: '场景列表',
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            Text(
                              '列：id（英文标识）· name（前缀用中文名）· description（可选）',
                              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                                    color: scheme.onSurfaceVariant,
                                  ),
                            ),
                            const SizedBox(height: 10),
                            if (rows.isEmpty)
                              Padding(
                                padding: const EdgeInsets.symmetric(vertical: 12),
                                child: Text(
                                  '暂无场景，请添加一行或导入表格',
                                  style: Theme.of(context)
                                      .textTheme
                                      .bodyMedium
                                      ?.copyWith(color: scheme.onSurfaceVariant),
                                ),
                              ),
                            for (var i = 0; i < rows.length; i++) ...[
                              if (i > 0) const SizedBox(height: 8),
                              Row(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Expanded(
                                    flex: 2,
                                    child: TextField(
                                      controller: rows[i].id,
                                      decoration: const InputDecoration(
                                        labelText: 'id',
                                        hintText: 'night',
                                      ),
                                    ),
                                  ),
                                  const SizedBox(width: 8),
                                  Expanded(
                                    flex: 2,
                                    child: TextField(
                                      controller: rows[i].name,
                                      decoration: const InputDecoration(
                                        labelText: 'name',
                                        hintText: '夜景',
                                      ),
                                    ),
                                  ),
                                  const SizedBox(width: 8),
                                  Expanded(
                                    flex: 3,
                                    child: TextField(
                                      controller: rows[i].description,
                                      decoration: const InputDecoration(
                                        labelText: 'description',
                                      ),
                                    ),
                                  ),
                                  IconButton(
                                    tooltip: '删除',
                                    onPressed: () => _removeRow(i),
                                    icon: const Icon(Icons.delete_outline),
                                  ),
                                ],
                              ),
                            ],
                            const SizedBox(height: 10),
                            Wrap(
                              spacing: 8,
                              runSpacing: 8,
                              children: [
                                GlassCapsuleButton(
                                  label: '添加一行',
                                  onPressed: _addRow,
                                ),
                                GlassCapsuleButton(
                                  label: '外部导入表格',
                                  onPressed: _importTableFile,
                                ),
                                GlassCapsuleButton(
                                  label: '导出 CSV',
                                  onPressed: rows.isEmpty ? null : _exportCsv,
                                ),
                              ],
                            ),
                            const SizedBox(height: 12),
                            Text('导入模式', style: Theme.of(context).textTheme.labelLarge),
                            const SizedBox(height: 4),
                            SegmentedButton<bool>(
                              segments: const [
                                ButtonSegment(
                                  value: false,
                                  label: Text('替换'),
                                ),
                                ButtonSegment(
                                  value: true,
                                  label: Text('合并'),
                                ),
                              ],
                              selected: {importMerge},
                              onSelectionChanged: (s) =>
                                  setState(() => importMerge = s.first),
                            ),
                            const SizedBox(height: 8),
                            Text(
                              '支持 Excel / CSV / TSV / TXT（含 UTF-8、UTF-16 BOM）',
                              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                                    color: scheme.onSurfaceVariant,
                                  ),
                            ),
                            const SizedBox(height: 12),
                            TextField(
                              controller: pasteCtrl,
                              minLines: 3,
                              maxLines: 5,
                              decoration: const InputDecoration(
                                labelText: '粘贴表格文本',
                                hintText: 'id,name,description',
                                alignLabelWithHint: true,
                              ),
                            ),
                            const SizedBox(height: 8),
                            Align(
                              alignment: Alignment.centerLeft,
                              child: GlassCapsuleButton(
                                label: '解析到列表并写入',
                                onPressed: _importPaste,
                              ),
                            ),
                          ],
                        ),
                      ),
                      if (status != null)
                        Padding(
                          padding: const EdgeInsets.only(bottom: 6),
                          child: Text(status!, style: Theme.of(context).textTheme.bodySmall),
                        ),
                      if (error != null)
                        Padding(
                          padding: const EdgeInsets.only(bottom: 6),
                          child: SelectableText(
                            error!,
                            style: Theme.of(context)
                                .textTheme
                                .bodySmall
                                ?.copyWith(color: scheme.error),
                          ),
                        ),
                    ],
                  ),
                ),
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
                child: Row(
                  children: [
                    GlassCapsuleButton(
                      label: '重新加载',
                      onPressed: loading || saving ? null : _load,
                    ),
                    const Spacer(),
                    GlassCapsuleButton(
                      label: '取消',
                      onPressed: () => Navigator.of(context).pop(),
                    ),
                    const SizedBox(width: 8),
                    GlassCapsuleButton(
                      label: saving ? '保存中…' : '保存设置',
                      primary: true,
                      onPressed: loading || saving ? null : _save,
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
