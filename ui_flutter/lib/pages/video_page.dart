import 'dart:async';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';

import '../host/host_controller.dart';
import '../video/compare_stage.dart';
import '../video/synced_players.dart';
import '../widgets/glass_dropdown.dart';
import '../widgets/glass_list_panel.dart';
import '../widgets/liquid_glass.dart';
import '../widgets/page_chrome.dart';
import '../widgets/section_card.dart';
import 'scene_recognize_progress.dart';
import 'scene_recognize_settings.dart';

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
  String info = '';
  String alignQuality = 'fast';
  bool cardView = false;
  bool autoRecognizeOnImport = false;
  bool recognizing = false;
  String? recognizeJobId;
  SceneRecognizeProgressController? _recognizeProgress;

  final playback = SyncedPlaybackController();
  CompareViewMode compareMode = CompareViewMode.grid;
  int? soloVideoId;
  double wipeSplit = 0.5;
  bool _attaching = false;

  @override
  void initState() {
    super.initState();
    playback.addListener(_onPlayback);
    WidgetsBinding.instance.addPostFrameCallback((_) => _boot());
  }

  @override
  void dispose() {
    playback.removeListener(_onPlayback);
    playback.dispose();
    super.dispose();
  }

  void _onPlayback() {
    if (mounted) setState(() {});
  }

  Future<void> _boot() async {
    final host = context.read<HostController>();
    try {
      final avail = await host.call('video.availability');
      info =
          'ffmpeg=${avail['ffmpeg_ok']} ffprobe=${avail['ffprobe_ok']} ${avail['ffmpeg_version'] ?? ''}';
      batches = await host.callList('video.list_batches');
      try {
        final cfg = await host.call('scene.config_get');
        autoRecognizeOnImport = cfg['auto_on_import'] == true;
      } catch (_) {}
      setState(() {});
    } catch (e) {
      setState(() => info = '$e');
    }
  }

  List<Map<String, dynamic>> _videosForPlayback() {
    final ids = selectedIds.isNotEmpty
        ? selectedIds.toList()
        : (activeId != null ? [activeId!] : <int>[]);
    final out = <Map<String, dynamic>>[];
    for (final id in ids) {
      for (final v in videos) {
        if ((v['id'] as num?)?.toInt() == id) {
          out.add(v);
          break;
        }
      }
    }
    if (out.length > SyncedPlaybackController.maxLanes) {
      info = '最多同时播放 ${SyncedPlaybackController.maxLanes} 路，已截取前 ${SyncedPlaybackController.maxLanes} 路';
      return out.take(SyncedPlaybackController.maxLanes).toList();
    }
    return out;
  }

  Future<void> _refreshPlayback() async {
    if (_attaching) return;
    _attaching = true;
    try {
      final list = _videosForPlayback();
      await playback.attach(list);
      if (list.length < 2 && compareMode == CompareViewMode.wipe) {
        compareMode = CompareViewMode.grid;
      }
      if (compareMode == CompareViewMode.solo) {
        soloVideoId ??= list.isNotEmpty ? (list.first['id'] as num).toInt() : null;
      }
    } finally {
      _attaching = false;
      if (mounted) setState(() {});
    }
  }

  Future<void> _import() async {
    final path = await FilePicker.platform.getDirectoryPath();
    if (path == null || !mounted) return;
    final host = context.read<HostController>();
    final res = await host.call('video.import_folder', {
      'folder': path,
      'generate_thumbnails': false,
      'auto_recognize': autoRecognizeOnImport,
    });
    if (!mounted) return;
    batchId = (res['batch_id'] as num?)?.toInt();
    var msg =
        '导入 ${res['imported']}，跳过 ${(res['skipped'] as List?)?.length ?? 0}';
    final recognize = res['recognize'];
    final recognizeError = res['recognize_error']?.toString();
    if (recognize is Map) {
      msg +=
          ' · 识别 ${recognize['matched']}/${recognize['total']}，重命名 ${recognize['renamed']}';
    } else if (recognizeError != null && recognizeError.isNotEmpty) {
      msg += ' · 场景识别跳过：$recognizeError';
    }
    info = msg;
    batches = await host.callList('video.list_batches');
    if (batchId != null) await _loadVideos(batchId!);
    setState(() {});
  }

  Future<void> _recognizeScenes() async {
    if (batchId == null) {
      setState(() => info = '请先选择视频批次');
      return;
    }
    if (recognizing) return;
    final host = context.read<HostController>();
    final progress = SceneRecognizeProgressController()
      ..update(current: 0, total: 0, message: '准备抽帧、识别与匹配…');
    _recognizeProgress = progress;
    final done = Completer<Map<String, dynamic>>();
    void onHost() {
      if (!mounted || !recognizing) return;
      final jobId = recognizeJobId;
      if (jobId == null) return;
      final ev = host.jobs[jobId];
      if (ev == null) return;
      final kind = ev['event']?.toString() ?? '';
      if (kind == 'job_progress' || kind == 'JobProgress') {
        final cur = (ev['current'] as num?)?.toInt() ?? 0;
        final tot = (ev['total'] as num?)?.toInt() ?? 0;
        final msg = ev['message']?.toString() ?? '';
        progress.update(
          current: cur,
          total: tot,
          message: msg.isEmpty ? '处理中…' : msg,
        );
      } else if (kind == 'job_finished' || kind == 'JobFinished') {
        if (!done.isCompleted) done.complete(ev);
      }
    }

    final jobId = 'video-scene-recognize-$batchId';
    setState(() {
      recognizing = true;
      recognizeJobId = jobId;
      info = '视频场景识别中…';
    });
    host.jobs.remove(jobId);
    host.addListener(onHost);

    final dialogFuture = showSceneRecognizeProgressDialog(
      context: context,
      controller: progress,
      onCancel: _cancelRecognize,
      title: '视频场景识别与匹配',
    );

    try {
      final start = await host.call('video.recognize_scenes', {'batch_id': batchId});
      final returnedId = start['job_id']?.toString();
      if (returnedId != null && returnedId != jobId) {
        setState(() => recognizeJobId = returnedId);
      }
      final finished = await done.future.timeout(const Duration(minutes: 60));
      if (!mounted) return;
      final ok = finished['ok'] != false;
      final result = finished['result'];
      final report = result is Map ? result.cast<String, dynamic>() : null;
      final cancelled = report?['cancelled'] == true;
      late final String summary;
      if (!ok) {
        summary = '场景识别失败：${finished['message'] ?? ''}';
      } else if (cancelled) {
        summary =
            '已取消：完成 ${report?['items'] is List ? (report!['items'] as List).length : 0}/${report?['total']}，匹配 ${report?['matched']}，重命名 ${report?['renamed']}';
      } else if (report != null) {
        summary =
            '识别完成：匹配 ${report['matched']}/${report['total']}，重命名 ${report['renamed']}，失败 ${report['failed']}';
      } else {
        summary = finished['message']?.toString() ?? '识别完成';
      }
      progress.complete(summary);
      setState(() => info = summary);
      await _loadVideos(batchId!);
      await dialogFuture;
    } catch (e) {
      final msg = '场景识别失败：$e';
      progress.complete(msg);
      if (mounted) setState(() => info = msg);
      await dialogFuture;
    } finally {
      host.removeListener(onHost);
      progress.dispose();
      _recognizeProgress = null;
      if (mounted) {
        setState(() {
          recognizing = false;
          recognizeJobId = null;
        });
      }
    }
  }

  Future<void> _cancelRecognize() async {
    final jobId = recognizeJobId;
    if (jobId == null) return;
    final host = context.read<HostController>();
    try {
      await host.call('app.cancel_job', {'job_id': jobId});
      _recognizeProgress?.markCancelling();
    } catch (e) {
      _recognizeProgress?.update(
        current: _recognizeProgress!.current,
        total: _recognizeProgress!.total,
        message: '取消失败：$e',
      );
    }
  }

  Future<void> _toggleAutoRecognize(bool value) async {
    setState(() => autoRecognizeOnImport = value);
    try {
      await context.read<HostController>().call('scene.config_set', {
        'auto_on_import': value,
      });
    } catch (e) {
      setState(() => info = '保存自动识别偏好失败：$e');
    }
  }

  Future<void> _openSceneSettings() async {
    await showSceneRecognizeSettings(context);
    if (!mounted) return;
    try {
      final cfg = await context.read<HostController>().call('scene.config_get');
      setState(() => autoRecognizeOnImport = cfg['auto_on_import'] == true);
    } catch (_) {}
  }

  Future<void> _loadVideos(int id) async {
    final host = context.read<HostController>();
    videos = await host.callList('video.list_videos', {'batch_id': id});
    setState(() {
      batchId = id;
      selectedIds.clear();
      activeId = videos.isNotEmpty ? (videos.first['id'] as num?)?.toInt() : null;
      if (activeId != null) selectedIds.add(activeId!);
    });
    await _refreshPlayback();
  }

  Future<void> _selectVideo(int id, {bool toggleSelect = false}) async {
    setState(() {
      activeId = id;
      if (toggleSelect) {
        if (selectedIds.contains(id)) {
          selectedIds.remove(id);
        } else {
          selectedIds.add(id);
        }
      } else {
        selectedIds.add(id);
      }
    });
    await _refreshPlayback();
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
    if (batchId != null) {
      await _loadVideos(batchId!);
    }
  }

  Future<void> _exportSheet() async {
    final ids = selectedIds.toList();
    if (ids.isEmpty && activeId != null) ids.add(activeId!);
    if (ids.isEmpty) return;
    final path = await FilePicker.platform.saveFile(
      dialogTitle: '导出宫格',
      fileName: 'contact_sheet.png',
    );
    if (path == null || !mounted) return;
    final host = context.read<HostController>();
    final res = await host.call('video.export_contact_sheet', {
      'ids': ids,
      'pts_ms': playback.globalPtsMs,
      'output': path,
    });
    if (!mounted) return;
    setState(() => info = '已导出 ${res['path']} (${res['cols']}x${res['rows']})');
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
    if (path == null || !mounted) return;
    final host = context.read<HostController>();
    final res = await host.call('video.export_grid_video', {
      'ids': ids,
      'start_ms': playback.globalPtsMs,
      'duration_ms': 5000,
      'output': path,
    });
    if (!mounted) return;
    setState(() => info = '已导出对比视频 ${res['path']}');
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

  Future<void> _setVideoCompareMode(CompareViewMode mode) async {
    if (mode == CompareViewMode.wipe) {
      await _ensureVideoCompareSelection(minCount: 2);
      if (selectedIds.length < 2 && playback.slots.length < 2) {
        setState(() => info = 'Wipe 对比需要至少 2 路视频（勾选后重试）');
        return;
      }
    } else if (mode != CompareViewMode.solo) {
      await _ensureVideoCompareSelection(minCount: 1);
    }
    setState(() {
      compareMode = mode;
      if (mode == CompareViewMode.solo) {
        soloVideoId = activeId ?? soloVideoId;
      }
      info = switch (mode) {
        CompareViewMode.grid => '对比：宫格',
        CompareViewMode.solo => '对比：Solo',
        CompareViewMode.wipe => '对比：Wipe',
      };
    });
  }

  Future<void> _ensureVideoCompareSelection({int minCount = 2}) async {
    if (activeId != null) selectedIds.add(activeId!);
    if (selectedIds.length >= minCount) {
      await _refreshPlayback();
      return;
    }
    final idx = videos.indexWhere((e) => (e['id'] as num?)?.toInt() == activeId);
    final start = idx < 0 ? 0 : idx;
    for (var i = 0; i < videos.length && selectedIds.length < minCount; i++) {
      final id = (videos[(start + i) % videos.length]['id'] as num).toInt();
      selectedIds.add(id);
    }
    setState(() {});
    await _refreshPlayback();
  }

  void _cycleVideoCompareMode() {
    final canWipe = playback.slots.length >= 2 || selectedIds.length >= 2;
    final next = switch (compareMode) {
      CompareViewMode.grid => CompareViewMode.solo,
      CompareViewMode.solo =>
        canWipe ? CompareViewMode.wipe : CompareViewMode.grid,
      CompareViewMode.wipe => CompareViewMode.grid,
    };
    _setVideoCompareMode(next);
  }

  @override
  Widget build(BuildContext context) {
    final canWipe = playback.slots.length >= 2;

    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.escape): () {
          if (compareMode != CompareViewMode.grid) {
            _setVideoCompareMode(CompareViewMode.grid);
          }
        },
        const SingleActivator(LogicalKeyboardKey.space): () {
          playback.toggle();
        },
        const SingleActivator(LogicalKeyboardKey.keyC): _cycleVideoCompareMode,
        const SingleActivator(LogicalKeyboardKey.keyG): () {
          _setVideoCompareMode(CompareViewMode.grid);
        },
        const SingleActivator(LogicalKeyboardKey.keyS): () {
          _setVideoCompareMode(CompareViewMode.solo);
        },
        const SingleActivator(LogicalKeyboardKey.keyW): () {
          _setVideoCompareMode(CompareViewMode.wipe);
        },
      },
      child: Focus(
        autofocus: true,
        child: PageChrome(
          title: '视频评审',
          subtitle: '播放、宫格/Solo/Wipe 对比、对齐、场景识别与导出',
          actions: [
            GlassCapsuleButton(
              label: '导入',
              primary: true,
              onPressed: _import,
            ),
            const SizedBox(width: 8),
            GlassCapsuleButton(
              label: recognizing ? '识别中…' : '场景识别命名',
              onPressed: batchId == null || recognizing ? null : _recognizeScenes,
            ),
            const SizedBox(width: 8),
            GlassCapsuleButton(
              label: '场景识别设置',
              onPressed: _openSceneSettings,
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
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              GlassListPanel(
                width: 300,
                child: Column(
                  children: [
                    Expanded(
                      child: ListView(
                        padding: EdgeInsets.zero,
                        children: [
                          Padding(
                            padding: const EdgeInsets.fromLTRB(10, 8, 10, 4),
                            child: SwitchListTile(
                              contentPadding: EdgeInsets.zero,
                              dense: true,
                              title: const Text(
                                '导入后场景识别',
                                style: TextStyle(fontSize: 13),
                              ),
                              value: autoRecognizeOnImport,
                              onChanged: _toggleAutoRecognize,
                            ),
                          ),
                          const GlassPanelDivider(),
                          const GlassSectionLabel('批次'),
                          ...batches.map(
                            (b) => GlassListTile(
                              selected: batchId == (b['id'] as num?)?.toInt(),
                              title: b['name']?.toString() ?? '',
                              subtitle: '共 ${b['total_count']}',
                              onTap: () => _loadVideos((b['id'] as num).toInt()),
                            ),
                          ),
                          const GlassPanelDivider(),
                          const GlassSectionLabel('视频（勾选加入对比）'),
                          if (!cardView)
                            ...videos.map((v) {
                              final id = (v['id'] as num).toInt();
                              final selected = selectedIds.contains(id);
                              return GlassListTile(
                                selected: activeId == id,
                                title: v['file_path']
                                        ?.toString()
                                        .split(Platform.pathSeparator)
                                        .last ??
                                    '',
                                subtitle:
                                    '${v['width']}x${v['height']}  offset=${v['offset_ms']}ms',
                                leading: SizedBox(
                                  width: 22,
                                  height: 22,
                                  child: Checkbox(
                                    value: selected,
                                    onChanged: (on) async {
                                      setState(() {
                                        if (on == true) {
                                          selectedIds.add(id);
                                        } else {
                                          selectedIds.remove(id);
                                        }
                                        activeId = id;
                                      });
                                      await _refreshPlayback();
                                    },
                                  ),
                                ),
                                onTap: () => _selectVideo(id),
                              );
                            })
                          else
                            Padding(
                              padding: const EdgeInsets.all(6),
                              child: Wrap(
                                spacing: 8,
                                runSpacing: 8,
                                children: videos.map((v) {
                                  final id = (v['id'] as num).toInt();
                                  return SizedBox(
                                    width: 120,
                                    child: GlassListTile(
                                      selected: activeId == id,
                                      title: v['file_path']
                                              ?.toString()
                                              .split(Platform.pathSeparator)
                                              .last ??
                                          '',
                                      onTap: () => _selectVideo(id, toggleSelect: true),
                                    ),
                                  );
                                }).toList(),
                              ),
                            ),
                        ],
                      ),
                    ),
                    if (info.isNotEmpty)
                      Padding(
                        padding: const EdgeInsets.fromLTRB(10, 4, 10, 6),
                        child: Text(
                          info,
                          style: Theme.of(context).textTheme.labelSmall,
                        ),
                      ),
                  ],
                ),
              ),
              Expanded(
                child: ListView(
                  padding: const EdgeInsets.fromLTRB(8, 0, 16, 16),
                  children: [
                    SectionCard(
                      title: '播放 / 对比舞台',
                      child: Column(
                        children: [
                          SizedBox(
                            height: 420,
                            width: double.infinity,
                            child: CompareStage(
                              playback: playback,
                              mode: compareMode,
                              soloVideoId: soloVideoId ?? activeId,
                              wipeSplit: wipeSplit,
                              onSolo: (id) {
                                setState(() {
                                  soloVideoId = id;
                                  compareMode = CompareViewMode.solo;
                                });
                              },
                              onWipeSplit: (v) => setState(() => wipeSplit = v),
                              onExitSolo: () =>
                                  setState(() => compareMode = CompareViewMode.grid),
                            ),
                          ),
                          const SizedBox(height: 8),
                          TransportBar(
                            playback: playback,
                            mode: compareMode,
                            canWipe: canWipe,
                            onMode: (m) => _setVideoCompareMode(m),
                            onPlayPause: () => playback.toggle(),
                            onSeek: (v) => playback.seekGlobal(v.round()),
                            onStep: (d) => playback.stepMs(d),
                            onRate: (r) => playback.setRate(r),
                          ),
                          Align(
                            alignment: Alignment.centerLeft,
                            child: Text(
                              '快捷键：C 循环对比 · G 宫格 · S Solo · W Wipe · Esc 回宫格 · 空格播放/暂停',
                              style: Theme.of(context).textTheme.labelSmall,
                            ),
                          ),
                        ],
                      ),
                    ),
                    SectionCard(
                      title: '对齐与导出',
                      child: GlassActionBar(
                        children: [
                          SizedBox(
                            width: 140,
                            height: 40,
                            child: GlassDropdownMenu<String>(
                              width: 140,
                              initialSelection: alignQuality,
                              requestFocusOnTap: false,
                              onSelected: (v) {
                                if (v != null) setState(() => alignQuality = v);
                              },
                              dropdownMenuEntries: const [
                                DropdownMenuEntry(value: 'fast', label: '快速对齐'),
                                DropdownMenuEntry(value: 'standard', label: '标准对齐'),
                                DropdownMenuEntry(value: 'fine', label: '精细对齐'),
                              ],
                            ),
                          ),
                          GlassCapsuleButton(
                            label: '偏移校准',
                            primary: true,
                            onPressed: _align,
                          ),
                          GlassCapsuleButton(
                            label: '导出宫格 PNG',
                            onPressed: _exportSheet,
                          ),
                          GlassCapsuleButton(
                            label: '导出对比视频',
                            onPressed: _exportGridVideo,
                          ),
                          GlassCapsuleButton(
                            label: '批量通过',
                            onPressed: () => _setStatus('Approved'),
                          ),
                          GlassCapsuleButton(
                            label: '批量需修',
                            onPressed: () => _setStatus('NeedsFix'),
                          ),
                          GlassCapsuleButton(
                            label: '清理抽帧缓存',
                            onPressed: _clearCache,
                          ),
                        ],
                      ),
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
