import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../host/host_controller.dart';
import '../widgets/glass_dropdown.dart';
import '../widgets/glass_list_panel.dart';
import '../widgets/liquid_glass.dart';
import '../widgets/page_chrome.dart';
import '../widgets/section_card.dart';

class ConvertPage extends StatefulWidget {
  const ConvertPage({super.key});

  @override
  State<ConvertPage> createState() => _ConvertPageState();
}

class _ConvertPageState extends State<ConvertPage> {
  final inputCtrl = TextEditingController();
  final outputCtrl = TextEditingController(text: './output');
  final renameCtrl = TextEditingController();
  final refPathCtrl = TextEditingController();

  String format = 'webp';
  double quality = 85;
  bool recursive = true;
  bool preserveStructure = true;
  bool overwrite = false;
  bool stripMetadata = false;
  bool bayerOnly = false;
  bool brightnessMatch = false;
  String brightnessMode = 'paired';
  bool bmPercentile = true;
  double bmPercentileValue = 98;
  bool bmRegional = false;
  bool useTargetMax = false;
  double targetMaxKb = 500;
  bool preferRemote = false;

  List<Map<String, dynamic>> formats = [];
  Map<String, dynamic>? preview;
  String? jobId;
  String status = '就绪';
  int advancedTab = 0;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _loadFormats());
  }

  @override
  void dispose() {
    inputCtrl.dispose();
    outputCtrl.dispose();
    renameCtrl.dispose();
    refPathCtrl.dispose();
    super.dispose();
  }

  Future<void> _loadFormats() async {
    final host = context.read<HostController>();
    if (!host.connected) {
      await host.bootstrap();
    }
    if (!host.connected) {
      setState(() => status = host.lastError ?? 'Host 未连接');
      return;
    }
    try {
      final res = await host.call('app.formats');
      final list = (res['formats'] as List?)?.cast<Map>() ?? [];
      setState(() {
        formats = list.map((e) => e.cast<String, dynamic>()).toList();
        if (formats.isNotEmpty) {
          format = formats.first['extension'] as String? ?? 'webp';
        }
      });
      final remote = await host.call('remote.status');
      setState(() => preferRemote = remote['prefer_remote'] == true);
    } catch (e) {
      setState(() => status = '加载失败：$e');
    }
  }

  Map<String, dynamic> _params({bool async = true}) => {
        'input_dir': inputCtrl.text.trim(),
        'output_dir': outputCtrl.text.trim(),
        'format': format,
        'quality': quality.round(),
        'recursive': recursive,
        'preserve_structure': preserveStructure,
        'overwrite': overwrite,
        'strip_metadata': stripMetadata,
        'bayer_only': bayerOnly,
        'rename_template': renameCtrl.text.trim(),
        if (useTargetMax) 'target_max_kb': targetMaxKb.round(),
        'brightness_match_enabled': brightnessMatch,
        'brightness_match_mode': brightnessMode,
        'brightness_match_path': refPathCtrl.text.trim(),
        'brightness_match_metric_percentile': bmPercentile,
        'brightness_match_percentile': bmPercentileValue,
        'brightness_match_regional': bmRegional,
        'async': async,
      };

  Future<void> _pickDir(TextEditingController ctrl) async {
    final path = await FilePicker.platform.getDirectoryPath();
    if (path != null) setState(() => ctrl.text = path);
  }

  Future<void> _preview() async {
    final host = context.read<HostController>();
    try {
      final res = await host.call('convert.preview', _params(async: false));
      setState(() {
        preview = res;
        status =
            '预览：将转换 ${res['to_convert']} / 匹配 ${res['matched']}，冲突 ${res['output_conflicts']}';
      });
    } catch (e) {
      setState(() => status = '预览失败：$e');
    }
  }

  Future<void> _run() async {
    final host = context.read<HostController>();
    try {
      final res = await host.call('convert.run', _params());
      setState(() {
        jobId = res['job_id']?.toString();
        status = jobId == null ? '已完成' : '任务已启动 $jobId';
      });
      await host.reloadPrefs();
    } catch (e) {
      setState(() => status = '转换失败：$e');
    }
  }

  Future<void> _cancel() async {
    if (jobId == null) return;
    final host = context.read<HostController>();
    await host.call('app.cancel_job', {'job_id': jobId});
    setState(() => status = '已请求取消');
  }

  Future<void> _openOutput() async {
    final host = context.read<HostController>();
    await host.call('app.open_path', {'path': outputCtrl.text.trim()});
  }

  Future<void> _savePreset() async {
    final nameCtrl = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('保存预设'),
        content: TextField(
          controller: nameCtrl,
          decoration: const InputDecoration(labelText: '预设名称'),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('取消')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('保存')),
        ],
      ),
    );
    if (ok != true || nameCtrl.text.trim().isEmpty) return;
    final host = context.read<HostController>();
    await host.call('prefs.upsert_preset', {
      'name': nameCtrl.text.trim(),
      'snapshot': {
        'format': format,
        'quality': quality.round(),
        'resize': {'width': null, 'height': null, 'mode': 'fit'},
        'recursive': recursive,
        'preserve_structure': preserveStructure,
        'overwrite': overwrite,
        'strip_metadata': stripMetadata,
        'bayer_only': bayerOnly,
        'rename_template': renameCtrl.text.trim(),
        'target_max_bytes': useTargetMax ? (targetMaxKb.round() * 1024) : null,
        'use_target_max_bytes': useTargetMax,
        'brightness_match_enabled': brightnessMatch,
        'brightness_match_mode': brightnessMode,
        'brightness_match_path': refPathCtrl.text.trim(),
        'brightness_match_metric_percentile': bmPercentile,
        'brightness_match_percentile': bmPercentileValue,
        'brightness_match_regional': bmRegional,
      },
    });
    await host.reloadPrefs();
    setState(() => status = '预设已保存');
  }

  @override
  Widget build(BuildContext context) {
    final host = context.watch<HostController>();
    final presets = ((host.prefs?['presets'] as List?) ?? [])
        .cast<Map>()
        .map((e) => e.cast<String, dynamic>())
        .toList();

    return PageChrome(
      title: '格式转换',
      subtitle: '批量转换、预设、亮度匹配、远端与设备导入入口',
      child: ListView(
        padding: const EdgeInsets.all(20),
        children: [
          SectionCard(
            title: '路径',
            child: Column(
              children: [
                _pathRow('输入目录', inputCtrl, () => _pickDir(inputCtrl)),
                const SizedBox(height: 8),
                _pathRow('输出目录', outputCtrl, () => _pickDir(outputCtrl)),
              ],
            ),
          ),
          SectionCard(
            title: '输出',
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    SizedBox(
                      width: 180,
                      child: GlassDropdownButtonFormField<String>(
                        width: 180,
                        value: format,
                        items: formats
                            .map((f) => DropdownMenuItem(
                                  value: f['extension'] as String,
                                  child: Text((f['extension'] as String).toUpperCase()),
                                ))
                            .toList(),
                        onChanged: (v) => setState(() => format = v ?? format),
                        decoration: const InputDecoration(labelText: '格式'),
                      ),
                    ),
                    const SizedBox(width: 16),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text('质量 ${quality.round()}'),
                          Slider(
                            value: quality,
                            min: 1,
                            max: 100,
                            divisions: 99,
                            onChanged: (v) => setState(() => quality = v),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
                Wrap(
                  spacing: 12,
                  runSpacing: 4,
                  children: [
                    FilterChip(
                      label: const Text('递归'),
                      selected: recursive,
                      onSelected: (v) => setState(() => recursive = v),
                    ),
                    FilterChip(
                      label: const Text('保留目录结构'),
                      selected: preserveStructure,
                      onSelected: (v) => setState(() => preserveStructure = v),
                    ),
                    FilterChip(
                      label: const Text('覆盖'),
                      selected: overwrite,
                      onSelected: (v) => setState(() => overwrite = v),
                    ),
                    FilterChip(
                      label: const Text('去除元数据'),
                      selected: stripMetadata,
                      onSelected: (v) => setState(() => stripMetadata = v),
                    ),
                    FilterChip(
                      label: const Text('仅 Bayer'),
                      selected: bayerOnly,
                      onSelected: (v) => setState(() => bayerOnly = v),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: renameCtrl,
                  decoration: const InputDecoration(
                    labelText: '重命名模板',
                    hintText: '{stem}_{width}x{height}',
                  ),
                ),
                GlassSwitchTile(
                  title: '限制目标体积',
                  value: useTargetMax,
                  onChanged: (v) => setState(() => useTargetMax = v),
                ),
                if (useTargetMax)
                  Row(
                    children: [
                      const Text('上限 KB'),
                      Expanded(
                        child: Slider(
                          value: targetMaxKb,
                          min: 50,
                          max: 5000,
                          onChanged: (v) => setState(() => targetMaxKb = v),
                        ),
                      ),
                      Text('${targetMaxKb.round()}'),
                    ],
                  ),
              ],
            ),
          ),
          SectionCard(
            title: '高级',
            child: Column(
              children: [
                SegmentedButton<int>(
                  segments: const [
                    ButtonSegment(value: 0, label: Text('亮度')),
                    ButtonSegment(value: 1, label: Text('远端')),
                    ButtonSegment(value: 2, label: Text('预设')),
                  ],
                  selected: {advancedTab},
                  onSelectionChanged: (s) => setState(() => advancedTab = s.first),
                ),
                const SizedBox(height: 12),
                if (advancedTab == 0) ...[
                  GlassSwitchTile(
                    title: '亮度匹配',
                    value: brightnessMatch,
                    onChanged: (v) => setState(() => brightnessMatch = v),
                  ),
                  GlassDropdownButtonFormField<String>(
                    value: brightnessMode,
                    items: const [
                      DropdownMenuItem(value: 'paired', child: Text('配对同名图')),
                      DropdownMenuItem(value: 'global', child: Text('全局参考图')),
                    ],
                    onChanged: (v) => setState(() => brightnessMode = v ?? 'paired'),
                    decoration: const InputDecoration(labelText: '模式'),
                  ),
                  const SizedBox(height: 8),
                  _pathRow('参考图路径', refPathCtrl, () async {
                    final r = await FilePicker.platform.pickFiles(type: FileType.image);
                    if (r?.files.single.path != null) {
                      setState(() => refPathCtrl.text = r!.files.single.path!);
                    }
                  }),
                ],
                if (advancedTab == 1)
                  GlassSwitchTile(
                    title: '优先远端执行',
                    subtitle: '需配置远端服务；本地仍可强制执行',
                    value: preferRemote,
                    onChanged: (v) async {
                      setState(() => preferRemote = v);
                      await host.call('remote.set_prefer', {'prefer': v});
                    },
                  ),
                if (advancedTab == 2)
                  Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Wrap(
                        spacing: 8,
                        children: [
                          GlassCapsuleButton(
                            label: '保存当前为预设',
                            onPressed: _savePreset,
                          ),
                        ],
                      ),
                      const SizedBox(height: 8),
                      ...presets.map(
                        (p) => GlassListTile(
                          title: p['name']?.toString() ?? '',
                          trailing: IconButton(
                            icon: const Icon(Icons.delete_outline, size: 18),
                            onPressed: () async {
                              await host.call('prefs.delete_preset', {
                                'name': p['name'],
                              });
                              await host.reloadPrefs();
                            },
                          ),
                          onTap: () {
                            final snap =
                                (p['snapshot'] as Map?)?.cast<String, dynamic>() ?? {};
                            setState(() {
                              format = snap['format']?.toString() ?? format;
                              quality = (snap['quality'] as num?)?.toDouble() ?? quality;
                              recursive = snap['recursive'] == true;
                              preserveStructure = snap['preserve_structure'] == true;
                              overwrite = snap['overwrite'] == true;
                              stripMetadata = snap['strip_metadata'] == true;
                              bayerOnly = snap['bayer_only'] == true;
                              renameCtrl.text = snap['rename_template']?.toString() ?? '';
                            });
                          },
                        ),
                      ),
                    ],
                  ),
              ],
            ),
          ),
          SectionCard(
            title: '操作',
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  children: [
                    GlassCapsuleButton(
                      label: '开始转换',
                      primary: true,
                      onPressed: _run,
                    ),
                    GlassCapsuleButton(label: '预览', onPressed: _preview),
                    GlassCapsuleButton(label: '取消', onPressed: _cancel),
                    GlassCapsuleButton(label: '打开输出', onPressed: _openOutput),
                  ],
                ),
                const SizedBox(height: 8),
                Text(status, style: Theme.of(context).textTheme.bodyMedium),
                if (preview != null) ...[
                  const SizedBox(height: 8),
                  Text(
                    '样例 ${(preview!['samples'] as List?)?.length ?? 0} 条',
                    style: Theme.of(context).textTheme.labelMedium,
                  ),
                ],
                const SizedBox(height: 8),
                SizedBox(
                  height: 140,
                  child: ListView.builder(
                    itemCount: host.logs.length.clamp(0, 40),
                    itemBuilder: (_, i) => Text(
                      host.logs[i],
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ),
                ),
              ],
            ),
          ),
          SectionCard(
            title: '设备导入 (ADB)',
            child: _DeviceImportPanel(onStatus: (s) => setState(() => status = s)),
          ),
        ],
      ),
    );
  }

  Widget _pathRow(String label, TextEditingController ctrl, VoidCallback onPick) {
    return Row(
      children: [
        Expanded(
          child: TextField(
            controller: ctrl,
            decoration: InputDecoration(labelText: label),
          ),
        ),
        const SizedBox(width: 8),
        GlassCapsuleButton(label: '选择', onPressed: onPick),
      ],
    );
  }
}

class _DeviceImportPanel extends StatefulWidget {
  const _DeviceImportPanel({required this.onStatus});
  final ValueChanged<String> onStatus;

  @override
  State<_DeviceImportPanel> createState() => _DeviceImportPanelState();
}

class _DeviceImportPanelState extends State<_DeviceImportPanel> {
  List<Map<String, dynamic>> devices = [];
  final selected = <String>{};
  final sourceCtrl = TextEditingController(text: '/sdcard/DCIM');
  final stagingCtrl = TextEditingController();

  @override
  void dispose() {
    sourceCtrl.dispose();
    stagingCtrl.dispose();
    super.dispose();
  }

  Future<void> _refresh() async {
    final host = context.read<HostController>();
    try {
      final res = await host.call('mobile.list_devices');
      final list = res['value'] is List
          ? (res['value'] as List)
          : (res is Map && res.values.isNotEmpty && res.values.first is List
              ? res.values.first as List
              : <dynamic>[]);
      // Prefer callList-style: host returns array → wrapped as value
      List<Map<String, dynamic>> parsed = [];
      if (res['value'] is List) {
        parsed = (res['value'] as List).map((e) => (e as Map).cast<String, dynamic>()).toList();
      } else {
        parsed = await host.callList('mobile.list_devices');
      }
      setState(() => devices = parsed);
      widget.onStatus('ADB 设备 ${devices.length}');
    } catch (e) {
      widget.onStatus('ADB 刷新失败：$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Wrap(
          spacing: 8,
          children: [
            GlassCapsuleButton(label: '刷新设备', onPressed: _refresh),
          ],
        ),
        const SizedBox(height: 8),
        TextField(
          controller: sourceCtrl,
          decoration: const InputDecoration(labelText: '设备来源路径'),
        ),
        const SizedBox(height: 8),
        TextField(
          controller: stagingCtrl,
          decoration: const InputDecoration(labelText: '本地暂存目录（可空）'),
        ),
        ...devices.map((d) {
          final serial = d['serial']?.toString() ?? '';
          return GlassCheckTile(
            value: selected.contains(serial),
            onChanged: (on) {
              setState(() {
                if (on == true) {
                  selected.add(serial);
                } else {
                  selected.remove(serial);
                }
              });
            },
            title: d['model']?.toString().isNotEmpty == true
                ? '${d['model']} ($serial)'
                : serial,
            subtitle: d['state']?.toString() ?? '',
          );
        }),
        if (devices.isEmpty)
          const Text('点击「刷新设备」列举 ADB；拉取仍建议通过转换流水线 mobile_pull 配置完成完整导入。'),
      ],
    );
  }
}
