import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../host/host_controller.dart';
import '../widgets/page_chrome.dart';
import '../widgets/section_card.dart';

class VideoPage extends StatefulWidget {
  const VideoPage({super.key});

  @override
  State<VideoPage> createState() => _VideoPageState();
}

class _VideoPageState extends State<VideoPage> {
  List<Map<String, dynamic>> batches = [];
  List<Map<String, dynamic>> videos = [];
  final selectedIds = <int>{};
  int? batchId;
  int? activeId;
  double ptsMs = 0;
  String? framePath;
  String info = '';
  String alignQuality = 'fast';
  bool cardView = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _boot());
  }

  Future<void> _boot() async {
    final host = context.read<HostController>();
    try {
      final avail = await host.call('video.availability');
      info =
          'ffmpeg=${avail['ffmpeg_ok']} ffprobe=${avail['ffprobe_ok']} ${avail['ffmpeg_version'] ?? ''}';
      batches = await host.callList('video.list_batches');
      setState(() {});
    } catch (e) {
      setState(() => info = '$e');
    }
  }

  Future<void> _import() async {
    final path = await FilePicker.platform.getDirectoryPath();
    if (path == null) return;
    final host = context.read<HostController>();
    final res = await host.call('video.import_folder', {
      'folder': path,
      'generate_thumbnails': false,
    });
    batchId = (res['batch_id'] as num?)?.toInt();
    info = '导入 ${res['imported']}，跳过 ${(res['skipped'] as List?)?.length ?? 0}';
    batches = await host.callList('video.list_batches');
    if (batchId != null) await _loadVideos(batchId!);
    setState(() {});
  }

  Future<void> _loadVideos(int id) async {
    final host = context.read<HostController>();
    videos = await host.callList('video.list_videos', {'batch_id': id});
    setState(() {
      batchId = id;
      selectedIds.clear();
      activeId = videos.isNotEmpty ? (videos.first['id'] as num?)?.toInt() : null;
      ptsMs = 0;
    });
    if (activeId != null) await _seek();
  }

  Future<void> _seek() async {
    if (activeId == null) return;
    final host = context.read<HostController>();
    final res = await host.call('video.frame_at', {
      'id': activeId,
      'pts_ms': ptsMs.round(),
      'width': 960,
    });
    setState(() => framePath = res['path']?.toString());
  }

  Future<void> _ensureCover(int id) async {
    final host = context.read<HostController>();
    final res = await host.call('video.ensure_cover', {'id': id});
    setState(() => framePath = res['path']?.toString());
  }

  Future<void> _align() async {
    final ids = selectedIds.isEmpty && activeId != null
        ? [activeId!]
        : selectedIds.toList();
    if (ids.length < 2) {
      setState(() => info = '对齐需要勾选至少 2 路视频');
      return;
    }
    final host = context.read<HostController>();
    final res = await host.call('video.align', {
      'ids': ids,
      'quality': alignQuality,
    });
    setState(() => info = '对齐完成 ${res['elapsed_ms']}ms，pairs=${(res['pairs'] as List?)?.length}');
    if (batchId != null) await _loadVideos(batchId!);
  }

  Future<void> _exportSheet() async {
    final ids = selectedIds.toList();
    if (ids.isEmpty && activeId != null) ids.add(activeId!);
    if (ids.isEmpty) return;
    final path = await FilePicker.platform.saveFile(
      dialogTitle: '导出宫格',
      fileName: 'contact_sheet.png',
    );
    if (path == null) return;
    final host = context.read<HostController>();
    final res = await host.call('video.export_contact_sheet', {
      'ids': ids,
      'pts_ms': ptsMs.round(),
      'output': path,
    });
    setState(() => info = '已导出 ${res['path']} (${res['cols']}x${res['rows']})');
  }

  Future<void> _setStatus(String status) async {
    final ids = selectedIds.isEmpty && activeId != null
        ? [activeId!]
        : selectedIds.toList();
    if (ids.isEmpty) return;
    await context.read<HostController>().call('video.batch_update_status', {
      'ids': ids,
      'status': status,
    });
    if (batchId != null) await _loadVideos(batchId!);
  }

  Future<void> _clearCache() async {
    final res = await context.read<HostController>().call('video.clear_frame_cache');
    setState(() => info = '清理缓存 ${res['removed']}');
  }

  @override
  Widget build(BuildContext context) {
    return PageChrome(
      title: '视频评审',
      subtitle: '导入、预览帧、多路勾选、对齐、宫格导出与缓存',
      actions: [
        OutlinedButton.icon(
          onPressed: _import,
          icon: const Icon(Icons.folder_open),
          label: const Text('导入'),
        ),
        const SizedBox(width: 8),
        IconButton(
          tooltip: cardView ? '列表' : '卡片',
          onPressed: () => setState(() => cardView = !cardView),
          icon: Icon(cardView ? Icons.view_list : Icons.grid_view),
        ),
        const SizedBox(width: 8),
      ],
      child: Row(
        children: [
          SizedBox(
            width: 300,
            child: Column(
              children: [
                Expanded(
                  child: ListView(
                    children: [
                      const ListTile(dense: true, title: Text('批次')),
                      ...batches.map(
                        (b) => ListTile(
                          dense: true,
                          selected: batchId == (b['id'] as num?)?.toInt(),
                          title: Text(b['name']?.toString() ?? ''),
                          subtitle: Text('共 ${b['total_count']}'),
                          onTap: () => _loadVideos((b['id'] as num).toInt()),
                        ),
                      ),
                      const Divider(),
                      if (!cardView)
                        ...videos.map((v) {
                          final id = (v['id'] as num).toInt();
                          return CheckboxListTile(
                            dense: true,
                            value: selectedIds.contains(id),
                            onChanged: (on) {
                              setState(() {
                                if (on == true) {
                                  selectedIds.add(id);
                                } else {
                                  selectedIds.remove(id);
                                }
                                activeId = id;
                              });
                              _seek();
                            },
                            title: Text(
                              v['file_path']?.toString().split(Platform.pathSeparator).last ?? '',
                              overflow: TextOverflow.ellipsis,
                            ),
                            subtitle: Text(
                              '${v['width']}x${v['height']}  offset=${v['offset_ms']}ms',
                            ),
                          );
                        })
                      else
                        Padding(
                          padding: const EdgeInsets.all(8),
                          child: Wrap(
                            spacing: 8,
                            runSpacing: 8,
                            children: videos.map((v) {
                              final id = (v['id'] as num).toInt();
                              return SizedBox(
                                width: 120,
                                child: InkWell(
                                  onTap: () {
                                    setState(() {
                                      activeId = id;
                                      selectedIds.add(id);
                                    });
                                    _ensureCover(id);
                                  },
                                  child: Card(
                                    child: Padding(
                                      padding: const EdgeInsets.all(8),
                                      child: Text(
                                        v['file_path']
                                                ?.toString()
                                                .split(Platform.pathSeparator)
                                                .last ??
                                            '',
                                        maxLines: 3,
                                        overflow: TextOverflow.ellipsis,
                                      ),
                                    ),
                                  ),
                                ),
                              );
                            }).toList(),
                          ),
                        ),
                    ],
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.all(8),
                  child: Text(info, style: Theme.of(context).textTheme.labelSmall),
                ),
              ],
            ),
          ),
          const VerticalDivider(width: 1),
          Expanded(
            child: ListView(
              padding: const EdgeInsets.all(16),
              children: [
                SectionCard(
                  title: '预览帧',
                  child: Column(
                    children: [
                      SizedBox(
                        height: 360,
                        width: double.infinity,
                        child: framePath != null && File(framePath!).existsSync()
                            ? Image.file(File(framePath!), fit: BoxFit.contain)
                            : const Center(child: Text('选择视频并拖动时间轴')),
                      ),
                      Slider(
                        value: ptsMs,
                        min: 0,
                        max: _activeDurationMs().toDouble().clamp(1, double.infinity),
                        label: '${ptsMs.round()} ms',
                        onChanged: (v) => setState(() => ptsMs = v),
                        onChangeEnd: (_) => _seek(),
                      ),
                    ],
                  ),
                ),
                SectionCard(
                  title: '对比与导出',
                  child: Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      DropdownButton<String>(
                        value: alignQuality,
                        items: const [
                          DropdownMenuItem(value: 'fast', child: Text('快速对齐')),
                          DropdownMenuItem(value: 'standard', child: Text('标准对齐')),
                          DropdownMenuItem(value: 'fine', child: Text('精细对齐')),
                        ],
                        onChanged: (v) => setState(() => alignQuality = v ?? 'fast'),
                      ),
                      FilledButton(onPressed: _align, child: const Text('偏移校准')),
                      OutlinedButton(onPressed: _exportSheet, child: const Text('导出宫格 PNG')),
                      OutlinedButton(onPressed: _exportGridVideo, child: const Text('导出对比视频')),
                      OutlinedButton(
                        onPressed: () => _setStatus('Approved'),
                        child: const Text('批量通过'),
                      ),
                      OutlinedButton(
                        onPressed: () => _setStatus('NeedsFix'),
                        child: const Text('批量需修'),
                      ),
                      OutlinedButton(onPressed: _clearCache, child: const Text('清理抽帧缓存')),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Future<void> _exportGridVideo() async {
    final ids = selectedIds.toList();
    if (ids.isEmpty && activeId != null) ids.add(activeId!);
    if (ids.length < 2) {
      setState(() => info = '对比视频需要至少 2 路');
      return;
    }
    final path = await FilePicker.platform.saveFile(
      dialogTitle: '导出对比视频',
      fileName: 'compare_grid.mp4',
    );
    if (path == null) return;
    final host = context.read<HostController>();
    final res = await host.call('video.export_grid_video', {
      'ids': ids,
      'start_ms': ptsMs.round(),
      'duration_ms': 5000,
      'output': path,
      'quality': 'high',
    });
    setState(() => info = '已导出对比视频 ${res['path']}');
  }

  num _activeDurationMs() {
    for (final v in videos) {
      if ((v['id'] as num?)?.toInt() == activeId) {
        return (v['duration_ms'] as num?) ?? 1;
      }
    }
    return 1;
  }
}
