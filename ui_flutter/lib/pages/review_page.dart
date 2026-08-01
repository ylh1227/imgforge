import 'dart:async';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';

import '../host/host_controller.dart';
import '../review/annotation_canvas.dart';
import '../review/annotation_models.dart';
import '../review/image_compare.dart';
import '../review/shared_viewport.dart';
import '../widgets/glass_dropdown.dart';
import '../widgets/glass_list_panel.dart';
import '../widgets/liquid_glass.dart';
import '../widgets/page_chrome.dart';
import 'scene_recognize_progress.dart';
import 'scene_recognize_settings.dart';

class ReviewPage extends StatefulWidget {
  const ReviewPage({super.key});

  @override
  State<ReviewPage> createState() => _ReviewPageState();
}

class _ReviewPageState extends State<ReviewPage> {
  List<Map<String, dynamic>> batches = [];
  List<Map<String, dynamic>> images = [];
  List<Map<String, dynamic>> tags = [];
  List<ReviewAnnotation> annotations = [];
  Set<int> imageTagIds = {};
  final selectedCompareIds = <int>{};

  int? batchId;
  int? imageId;
  int? selectedAnnId;

  final remarkCtrl = TextEditingController();
  final searchCtrl = TextEditingController();
  final canvasKey = GlobalKey<AnnotationCanvasState>();
  final sharedViewport = SharedViewportController();

  String status = 'Pending';
  String statusFilter = 'All';
  String annotationFilter = 'Any';
  String info = '';

  CanvasTool tool = CanvasTool.select;
  AnnStyle annStyle = const AnnStyle();
  ImageCompareMode compareMode = ImageCompareMode.single;

  bool autoRecognizeOnImport = false;
  bool recognizing = false;
  String? recognizeJobId;
  SceneRecognizeProgressController? _recognizeProgress;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      await _boot();
    });
  }

  @override
  void dispose() {
    remarkCtrl.dispose();
    searchCtrl.dispose();
    sharedViewport.dispose();
    super.dispose();
  }

  Future<void> _boot() async {
    await _reloadBatches();
    await _loadScenePrefs();
    await _loadTags();
    try {
      final sess = await context.read<HostController>().call('review.session_restore');
      final bid = (sess['batch_id'] as num?)?.toInt();
      final iid = (sess['image_id'] as num?)?.toInt();
      if (bid != null) {
        await _loadImages(bid, selectId: iid);
      }
    } catch (_) {}
  }

  Future<void> _loadScenePrefs() async {
    try {
      final cfg = await context.read<HostController>().call('scene.config_get');
      if (!mounted) return;
      setState(() => autoRecognizeOnImport = cfg['auto_on_import'] == true);
    } catch (_) {}
  }

  Future<void> _loadTags() async {
    try {
      final list = await context.read<HostController>().callList('review.list_tags');
      if (!mounted) return;
      setState(() => tags = list);
    } catch (_) {}
  }

  Future<void> _reloadBatches() async {
    final host = context.read<HostController>();
    try {
      batches = await host.callList('review.list_batches');
      setState(() => info = '批次 ${batches.length}');
    } catch (e) {
      setState(() => info = '加载失败：$e');
    }
  }

  Future<void> _importFolder() async {
    final path = await FilePicker.platform.getDirectoryPath();
    if (path == null || !mounted) return;
    final host = context.read<HostController>();
    final res = await host.call('review.import_folder', {
      'folder': path,
      'recursive': true,
      'auto_recognize': autoRecognizeOnImport,
    });
    if (!mounted) return;
    batchId = (res['batch_id'] as num?)?.toInt();
    final recognize = res['recognize'];
    final recognizeError = res['recognize_error']?.toString();
    var msg = '已导入批次 $batchId';
    if (recognize is Map) {
      msg +=
          ' · 识别 ${recognize['matched']}/${recognize['total']}，重命名 ${recognize['renamed']}';
    } else if (recognizeError != null && recognizeError.isNotEmpty) {
      msg += ' · 场景识别跳过：$recognizeError';
    }
    setState(() => info = msg);
    await _reloadBatches();
    if (batchId != null) await _loadImages(batchId!);
  }

  Future<void> _recognizeScenes() async {
    if (batchId == null) {
      setState(() => info = '请先选择评审批次');
      return;
    }
    if (recognizing) return;
    final host = context.read<HostController>();
    final progress = SceneRecognizeProgressController()
      ..update(current: 0, total: 0, message: '准备识别与匹配…');
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

    final jobId = 'scene-recognize-$batchId';
    setState(() {
      recognizing = true;
      recognizeJobId = jobId;
      info = '场景识别中…';
    });
    host.jobs.remove(jobId);
    host.addListener(onHost);

    // Show modal first so progress is visible as soon as the job starts.
    final dialogFuture = showSceneRecognizeProgressDialog(
      context: context,
      controller: progress,
      onCancel: _cancelRecognize,
      title: '图片场景识别与匹配',
    );

    try {
      final start = await host.call('review.recognize_scenes', {'batch_id': batchId});
      final startedJob = start['job_id']?.toString();
      if (startedJob != null) {
        setState(() => recognizeJobId = startedJob);
      }
      final finished = await done.future.timeout(
        const Duration(hours: 2),
        onTimeout: () => <String, dynamic>{'ok': false, 'message': '超时'},
      );
      if (!mounted) return;
      if (finished['ok'] == false) {
        final msg = '场景识别失败：${finished['message'] ?? ''}';
        progress.complete(msg);
        setState(() => info = msg);
      } else {
        final result = finished['result'];
        final cancelled = result is Map && result['cancelled'] == true;
        final summary = result is Map
            ? (cancelled
                ? '已取消：匹配 ${result['matched']}/${result['total']}，重命名 ${result['renamed']}'
                : '识别完成：匹配 ${result['matched']}/${result['total']}，重命名 ${result['renamed']}，失败 ${result['failed']}')
            : '场景识别完成';
        progress.complete(summary);
        setState(() => info = summary);
        if (batchId != null) await _loadImages(batchId!);
      }
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
    try {
      await context.read<HostController>().call('app.cancel_job', {'job_id': jobId});
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
    await _loadScenePrefs();
  }

  Future<void> _loadImages(int id, {int? selectId}) async {
    final host = context.read<HostController>();
    final params = <String, dynamic>{
      'batch_id': id,
      'search': searchCtrl.text.trim(),
      'annotation_filter': annotationFilter,
    };
    if (statusFilter != 'All') params['status'] = statusFilter;
    images = await host.callList('review.list_images', params);
    setState(() {
      batchId = id;
      selectedCompareIds.removeWhere((x) => !images.any((e) => (e['id'] as num?)?.toInt() == x));
    });
    final prefer = selectId ?? imageId;
    Map<String, dynamic>? next;
    for (final e in images) {
      if ((e['id'] as num?)?.toInt() == prefer) {
        next = e;
        break;
      }
    }
    next ??= images.isNotEmpty ? images.first : null;
    final nextId = (next?['id'] as num?)?.toInt();
    if (nextId != null) {
      await _selectImage(nextId);
    } else {
      setState(() {
        imageId = null;
        annotations = [];
      });
    }
  }

  Future<void> _selectImage(int id) async {
    final host = context.read<HostController>();
    final item = images.firstWhere(
      (e) => (e['id'] as num?)?.toInt() == id,
      orElse: () => <String, dynamic>{},
    );
    final raw = await host.callList('review.load_annotations', {'image_id': id});
    Set<int> tagIds = {};
    try {
      final t = await host.call('review.tags_for_image', {'image_id': id});
      final list = t['tag_ids'];
      if (list is List) {
        tagIds = list.map((e) => (e as num).toInt()).toSet();
      }
    } catch (_) {}
    setState(() {
      imageId = id;
      remarkCtrl.text = item['remark']?.toString() ?? '';
      status = item['status']?.toString() ?? 'Pending';
      annotations = raw.map(ReviewAnnotation.fromJson).toList();
      selectedAnnId = null;
      imageTagIds = tagIds;
    });
    sharedViewport.reset();
    if (batchId != null) {
      await host.call('review.session_save', {
        'batch_id': batchId,
        'image_id': id,
      });
    }
  }

  Future<void> _reloadAnnotations() async {
    if (imageId == null) return;
    final raw = await context
        .read<HostController>()
        .callList('review.load_annotations', {'image_id': imageId});
    setState(() {
      annotations = raw.map(ReviewAnnotation.fromJson).toList();
    });
  }

  Future<void> _saveMeta() async {
    if (imageId == null) return;
    final host = context.read<HostController>();
    await host.call('review.set_status', {'image_id': imageId, 'status': status});
    await host.call('review.set_remark', {
      'image_id': imageId,
      'remark': remarkCtrl.text,
    });
    setState(() => info = '已保存');
    if (batchId != null) await _loadImages(batchId!, selectId: imageId);
  }

  Future<void> _setStatusQuick(String s) async {
    if (imageId == null) return;
    setState(() => status = s);
    await context.read<HostController>().call('review.set_status', {
      'image_id': imageId,
      'status': s,
    });
    setState(() => info = '状态 → ${_statusLabel(s)}');
    if (batchId != null) await _loadImages(batchId!, selectId: imageId);
  }

  Future<void> _batchSetStatus(String s) async {
    final ids = selectedCompareIds.isNotEmpty
        ? selectedCompareIds.toList()
        : (imageId != null ? [imageId!] : <int>[]);
    if (ids.isEmpty) return;
    await context.read<HostController>().call('review.batch_set_status', {
      'image_ids': ids,
      'status': s,
    });
    setState(() => info = '批量状态 ${ids.length} 张 → ${_statusLabel(s)}');
    if (batchId != null) await _loadImages(batchId!, selectId: imageId);
  }

  Future<void> _exportCsv() async {
    if (batchId == null) return;
    final path = await FilePicker.platform.saveFile(
      dialogTitle: '导出 CSV',
      fileName: 'review_$batchId.csv',
    );
    if (path == null || !mounted) return;
    await context.read<HostController>().call('review.export_csv', {
      'batch_id': batchId,
      'path': path,
    });
    if (!mounted) return;
    setState(() => info = '已导出 $path');
  }

  Future<void> _exportAnnotationsJson() async {
    if (batchId == null) return;
    final dir = await FilePicker.platform.getDirectoryPath(
      dialogTitle: '选择标注 JSON 导出目录',
    );
    if (dir == null || !mounted) return;
    final res = await context.read<HostController>().call(
      'review.export_annotations_json',
      {'batch_id': batchId, 'output_dir': dir},
    );
    if (!mounted) return;
    setState(() => info = '已导出标注 JSON ${res['count']} 个 → $dir');
  }

  Future<void> _createAnnotation(ReviewAnnotation draft) async {
    final res = await context.read<HostController>().call(
          'review.add_annotation',
          draft.toCreateJson(),
        );
    await _reloadAnnotations();
    final id = (res['id'] as num?)?.toInt();
    setState(() {
      selectedAnnId = id;
      info = '已添加标注';
    });
    if (batchId != null) await _loadImages(batchId!, selectId: imageId);
  }

  Future<void> _updateAnnotation(ReviewAnnotation ann) async {
    await context.read<HostController>().call('review.update_annotation', {
      'id': ann.id,
      'position': ann.position,
      'content': ann.content,
    });
  }

  Future<void> _deleteAnnotation(int id) async {
    await context.read<HostController>().call('review.delete_annotation', {
      'id': id,
    });
    setState(() {
      selectedAnnId = null;
      info = '已删除标注';
    });
    await _reloadAnnotations();
    if (batchId != null) await _loadImages(batchId!, selectId: imageId);
  }

  Future<void> _undoLastAnnotation() async {
    if (imageId == null) return;
    try {
      await context.read<HostController>().call('review.undo_last_annotation', {
        'image_id': imageId,
      });
      await _reloadAnnotations();
      setState(() => info = '已撤销上一条标注');
      if (batchId != null) await _loadImages(batchId!, selectId: imageId);
    } catch (e) {
      setState(() => info = '撤销失败：$e');
    }
  }

  Future<void> _toggleTag(int tagId, bool on) async {
    if (imageId == null) return;
    await context.read<HostController>().call('review.set_image_tag', {
      'image_id': imageId,
      'tag_id': tagId,
      'on': on,
    });
    setState(() {
      if (on) {
        imageTagIds.add(tagId);
      } else {
        imageTagIds.remove(tagId);
      }
    });
  }

  Future<void> _createTag() async {
    final ctrl = TextEditingController();
    final name = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('新建标签'),
        content: TextField(
          controller: ctrl,
          autofocus: true,
          decoration: const InputDecoration(hintText: '标签名'),
          onSubmitted: (v) => Navigator.pop(ctx, v),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('取消')),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, ctrl.text),
            child: const Text('创建'),
          ),
        ],
      ),
    );
    if (name == null || name.trim().isEmpty) return;
    await context.read<HostController>().call('review.create_tag', {
      'name': name.trim(),
    });
    await _loadTags();
  }

  void _navImage(int delta) {
    if (images.isEmpty || imageId == null) return;
    final idx = images.indexWhere((e) => (e['id'] as num?)?.toInt() == imageId);
    if (idx < 0) return;
    final next = (idx + delta).clamp(0, images.length - 1);
    final id = (images[next]['id'] as num).toInt();
    _selectImage(id);
  }

  Map<String, dynamic>? get _selected {
    for (final img in images) {
      if ((img['id'] as num?)?.toInt() == imageId) return img;
    }
    return null;
  }

  ComparePane? get _primaryPane {
    final s = _selected;
    if (s == null) return null;
    final path = s['file_path']?.toString() ?? '';
    return (id: imageId!, path: path, label: fileLabel(path));
  }

  List<ComparePane> get _otherPanes {
    final out = <ComparePane>[];
    for (final id in selectedCompareIds) {
      if (id == imageId) continue;
      Map<String, dynamic>? img;
      for (final e in images) {
        if ((e['id'] as num?)?.toInt() == id) {
          img = e;
          break;
        }
      }
      if (img == null) continue;
      final path = img['file_path']?.toString() ?? '';
      out.add((id: id, path: path, label: fileLabel(path)));
    }
    return out;
  }

  void _ensureImageCompareSelection({int minCount = 2, int maxCount = 4}) {
    if (imageId != null) selectedCompareIds.add(imageId!);
    if (selectedCompareIds.length >= minCount) {
      while (selectedCompareIds.length > maxCount) {
        final drop = selectedCompareIds.firstWhere(
          (id) => id != imageId,
          orElse: () => selectedCompareIds.first,
        );
        selectedCompareIds.remove(drop);
      }
      return;
    }
    final idx = images.indexWhere((e) => (e['id'] as num?)?.toInt() == imageId);
    final start = idx < 0 ? 0 : idx;
    for (var i = 0; i < images.length && selectedCompareIds.length < minCount; i++) {
      final id = (images[(start + i) % images.length]['id'] as num).toInt();
      selectedCompareIds.add(id);
    }
  }

  void _setCompareMode(ImageCompareMode mode) {
    setState(() {
      if (mode != ImageCompareMode.single) {
        _ensureImageCompareSelection(
          minCount: 2,
          maxCount: mode == ImageCompareMode.grid ? 6 : 4,
        );
      }
      compareMode = mode;
      sharedViewport.reset();
    });
    WidgetsBinding.instance.addPostFrameCallback((_) {
      canvasKey.currentState?.fitToView();
    });
  }

  void _cycleCompareMode() {
    final next = switch (compareMode) {
      ImageCompareMode.single => ImageCompareMode.sideBySide,
      ImageCompareMode.sideBySide => ImageCompareMode.grid,
      ImageCompareMode.grid => ImageCompareMode.single,
    };
    _setCompareMode(next);
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    final key = event.logicalKey;
    final meta = HardwareKeyboard.instance.isMetaPressed ||
        HardwareKeyboard.instance.isControlPressed;

    if (meta && key == LogicalKeyboardKey.keyZ) {
      _undoLastAnnotation();
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.escape) {
      if (compareMode != ImageCompareMode.single) {
        _setCompareMode(ImageCompareMode.single);
        return KeyEventResult.handled;
      }
    }
    if (key == LogicalKeyboardKey.keyC) {
      _cycleCompareMode();
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.keyG) {
      _setCompareMode(ImageCompareMode.grid);
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.keyB) {
      _setCompareMode(ImageCompareMode.sideBySide);
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.arrowDown || key == LogicalKeyboardKey.keyJ) {
      _navImage(1);
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.arrowUp || key == LogicalKeyboardKey.keyK) {
      _navImage(-1);
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.digit0 || key == LogicalKeyboardKey.numpad0) {
      _setStatusQuick('Pending');
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.digit1 || key == LogicalKeyboardKey.numpad1) {
      _setStatusQuick('Approved');
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.digit2 || key == LogicalKeyboardKey.numpad2) {
      _setStatusQuick('NeedsFix');
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.digit3 || key == LogicalKeyboardKey.numpad3) {
      _setStatusQuick('Rejected');
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.keyV) {
      setState(() => tool = CanvasTool.select);
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.keyR) {
      setState(() => tool = CanvasTool.rectangle);
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.keyA) {
      setState(() => tool = CanvasTool.arrow);
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.keyT) {
      setState(() => tool = CanvasTool.text);
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.keyF) {
      canvasKey.currentState?.fitToView();
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.delete || key == LogicalKeyboardKey.backspace) {
      if (selectedAnnId != null) {
        _deleteAnnotation(selectedAnnId!);
        return KeyEventResult.handled;
      }
    }
    return KeyEventResult.ignored;
  }

  static String _statusLabel(String s) {
    switch (s) {
      case 'Approved':
        return '通过';
      case 'NeedsFix':
        return '需修';
      case 'Rejected':
        return '拒绝';
      default:
        return '待审';
    }
  }

  @override
  Widget build(BuildContext context) {
    return Focus(
      autofocus: true,
      onKeyEvent: _onKey,
      child: PageChrome(
        title: '图片评审',
        subtitle: '标注 · 对比 · 状态 · 场景识别',
        actions: [
          GlassCapsuleButton(
            label: '导入文件夹',
            primary: true,
            onPressed: _importFolder,
          ),
          const SizedBox(width: 8),
          GlassCapsuleButton(
            label: recognizing ? '识别中…' : '场景识别',
            onPressed: batchId == null || recognizing ? null : _recognizeScenes,
          ),
          const SizedBox(width: 8),
          GlassCapsuleButton(label: '识别设置', onPressed: _openSceneSettings),
          const SizedBox(width: 8),
          GlassCapsuleButton(label: '导出 CSV', onPressed: _exportCsv),
          const SizedBox(width: 8),
          GlassCapsuleButton(label: '导出 JSON', onPressed: _exportAnnotationsJson),
          const SizedBox(width: 12),
        ],
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            GlassListPanel(
              width: 300,
              child: ListView(
                padding: EdgeInsets.zero,
                children: [
                  Padding(
                    padding: const EdgeInsets.fromLTRB(10, 8, 10, 4),
                    child: SwitchListTile(
                      contentPadding: EdgeInsets.zero,
                      dense: true,
                      title: const Text('导入后场景识别', style: TextStyle(fontSize: 13)),
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
                      onTap: () => _loadImages((b['id'] as num).toInt()),
                    ),
                  ),
                  const GlassPanelDivider(),
                  Padding(
                    padding: const EdgeInsets.fromLTRB(12, 12, 12, 12),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        TextField(
                          controller: searchCtrl,
                          decoration: const InputDecoration(
                            hintText: '搜索文件名',
                            prefixIcon: Icon(Icons.search, size: 18),
                          ),
                          onSubmitted: (_) {
                            if (batchId != null) _loadImages(batchId!);
                          },
                        ),
                        const SizedBox(height: 14),
                        Text(
                          '状态筛选',
                          style: Theme.of(context).textTheme.labelSmall,
                        ),
                        const SizedBox(height: 6),
                        GlassDropdownButtonFormField<String>(
                          value: statusFilter,
                          isExpanded: true,
                          items: const [
                            DropdownMenuItem(value: 'All', child: Text('全部状态')),
                            DropdownMenuItem(value: 'Pending', child: Text('待审')),
                            DropdownMenuItem(value: 'Approved', child: Text('通过')),
                            DropdownMenuItem(value: 'NeedsFix', child: Text('需修')),
                            DropdownMenuItem(value: 'Rejected', child: Text('拒绝')),
                          ],
                          onChanged: (v) {
                            setState(() => statusFilter = v ?? 'All');
                            if (batchId != null) _loadImages(batchId!);
                          },
                        ),
                        const SizedBox(height: 14),
                        Text(
                          '标注筛选',
                          style: Theme.of(context).textTheme.labelSmall,
                        ),
                        const SizedBox(height: 6),
                        GlassDropdownButtonFormField<String>(
                          value: annotationFilter,
                          isExpanded: true,
                          items: const [
                            DropdownMenuItem(value: 'Any', child: Text('标注不限')),
                            DropdownMenuItem(value: 'None', child: Text('无标注')),
                            DropdownMenuItem(value: 'Has', child: Text('有标注')),
                          ],
                          onChanged: (v) {
                            setState(() => annotationFilter = v ?? 'Any');
                            if (batchId != null) _loadImages(batchId!);
                          },
                        ),
                        if (selectedCompareIds.isNotEmpty) ...[
                          const SizedBox(height: 12),
                          Wrap(
                            spacing: 6,
                            runSpacing: 6,
                            children: [
                              Text(
                                '已选 ${selectedCompareIds.length}',
                                style: Theme.of(context).textTheme.labelSmall,
                              ),
                              TextButton(
                                onPressed: () => _batchSetStatus('Approved'),
                                child: const Text('批量通过'),
                              ),
                              TextButton(
                                onPressed: () =>
                                    setState(selectedCompareIds.clear),
                                child: const Text('清除选择'),
                              ),
                            ],
                          ),
                        ],
                      ],
                    ),
                  ),
                  const GlassSectionLabel('图片'),
                  ...images.map((img) {
                    final id = (img['id'] as num?)?.toInt() ?? 0;
                    final path = img['file_path']?.toString() ?? '';
                    final checked = selectedCompareIds.contains(id);
                    return GlassListTile(
                      selected: imageId == id,
                      title: fileLabel(path),
                      subtitle:
                          '${_statusLabel(img['status']?.toString() ?? '')} · 标注 ${img['annotation_count'] ?? 0}',
                      leading: Checkbox(
                        value: checked,
                        onChanged: (v) {
                          setState(() {
                            if (v == true) {
                              selectedCompareIds.add(id);
                            } else {
                              selectedCompareIds.remove(id);
                            }
                          });
                        },
                      ),
                      onTap: () => _selectImage(id),
                    );
                  }),
                  if (info.isNotEmpty)
                    Padding(
                      padding: const EdgeInsets.fromLTRB(10, 8, 10, 12),
                      child: Text(info, style: Theme.of(context).textTheme.labelSmall),
                    ),
                ],
              ),
            ),
            Expanded(
              child: Padding(
                padding: const EdgeInsets.fromLTRB(8, 0, 8, 12),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    _toolbar(context),
                    const SizedBox(height: 8),
                    Expanded(
                      child: DecoratedBox(
                        decoration: BoxDecoration(
                          color: Theme.of(context).colorScheme.surfaceContainerHighest
                              .withValues(alpha: 0.35),
                          borderRadius: BorderRadius.circular(16),
                          border: Border.all(
                            color: Theme.of(context)
                                .colorScheme
                                .outlineVariant
                                .withValues(alpha: 0.5),
                          ),
                        ),
                        child: ClipRRect(
                          borderRadius: BorderRadius.circular(16),
                          child: ImageCompareStage(
                            mode: compareMode,
                            primary: _primaryPane,
                            others: _otherPanes,
                            annotations: annotations,
                            tool: tool,
                            style: annStyle,
                            selectedAnnId: selectedAnnId,
                            canvasKey: canvasKey,
                            sharedViewport: sharedViewport,
                            onSelectAnn: (id) => setState(() => selectedAnnId = id),
                            onCreate: _createAnnotation,
                            onUpdate: _updateAnnotation,
                            onDelete: _deleteAnnotation,
                          ),
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
            GlassListPanel(
              width: 280,
              child: ListView(
                padding: const EdgeInsets.fromLTRB(12, 10, 12, 16),
                children: [
                  Text('属性', style: Theme.of(context).textTheme.titleSmall),
                  const SizedBox(height: 8),
                  GlassDropdownButtonFormField<String>(
                    value: ['Pending', 'Approved', 'NeedsFix', 'Rejected'].contains(status)
                        ? status
                        : 'Pending',
                    items: const [
                      DropdownMenuItem(value: 'Pending', child: Text('待审')),
                      DropdownMenuItem(value: 'Approved', child: Text('通过')),
                      DropdownMenuItem(value: 'NeedsFix', child: Text('需修')),
                      DropdownMenuItem(value: 'Rejected', child: Text('拒绝')),
                    ],
                    onChanged: (v) => setState(() => status = v ?? status),
                    decoration: const InputDecoration(labelText: '状态'),
                  ),
                  const SizedBox(height: 8),
                  TextField(
                    controller: remarkCtrl,
                    minLines: 2,
                    maxLines: 4,
                    decoration: const InputDecoration(labelText: '备注'),
                  ),
                  const SizedBox(height: 8),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      GlassCapsuleButton(
                        label: '保存',
                        primary: true,
                        onPressed: _saveMeta,
                      ),
                      GlassCapsuleButton(
                        label: '撤销标注',
                        onPressed: _undoLastAnnotation,
                      ),
                    ],
                  ),
                  const SizedBox(height: 16),
                  Row(
                    children: [
                      Text('标签', style: Theme.of(context).textTheme.titleSmall),
                      const Spacer(),
                      TextButton(onPressed: _createTag, child: const Text('新建')),
                    ],
                  ),
                  const SizedBox(height: 6),
                  Wrap(
                    spacing: 6,
                    runSpacing: 6,
                    children: [
                      for (final t in tags)
                        FilterChip(
                          label: Text(t['name']?.toString() ?? ''),
                          selected: imageTagIds.contains((t['id'] as num?)?.toInt()),
                          onSelected: imageId == null
                              ? null
                              : (on) => _toggleTag((t['id'] as num).toInt(), on),
                        ),
                      if (tags.isEmpty)
                        Text('暂无标签', style: Theme.of(context).textTheme.labelSmall),
                    ],
                  ),
                  const SizedBox(height: 16),
                  Text(
                    '标注 ${annotations.length}',
                    style: Theme.of(context).textTheme.titleSmall,
                  ),
                  const SizedBox(height: 6),
                  ...annotations.map((a) {
                    final kind = switch (a.kind) {
                      AnnotationKind.rectangle => '矩形',
                      AnnotationKind.arrow => '箭头',
                      AnnotationKind.text => '文字',
                    };
                    final title = a.content.isNotEmpty ? '$kind · ${a.content}' : kind;
                    return ListTile(
                      dense: true,
                      contentPadding: EdgeInsets.zero,
                      selected: selectedAnnId == a.id,
                      title: Text(title, maxLines: 1, overflow: TextOverflow.ellipsis),
                      trailing: IconButton(
                        icon: const Icon(Icons.delete_outline, size: 18),
                        onPressed: () => _deleteAnnotation(a.id),
                      ),
                      onTap: () => setState(() => selectedAnnId = a.id),
                    );
                  }),
                  const SizedBox(height: 12),
                  Text(
                    '快捷键：C 循环对比 · B 并排 · G 宫格 · Esc 单图 · ↑↓/JK 切图 · 0–3 状态 · V/R/A/T 工具 · F 适应 · ⌘Z 撤销 · Alt 临时解锁',
                    style: Theme.of(context).textTheme.labelSmall,
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _toolbar(BuildContext context) {
    Widget toolBtn(CanvasTool t, String label, IconData icon) {
      final on = tool == t;
      return Padding(
        padding: const EdgeInsets.only(right: 6),
        child: FilterChip(
          selected: on,
          avatar: Icon(icon, size: 16),
          label: Text(label),
          onSelected: (_) => setState(() => tool = t),
        ),
      );
    }

    Widget modeBtn(ImageCompareMode m, String label) {
      final on = compareMode == m;
      return Padding(
        padding: const EdgeInsets.only(right: 6),
        child: FilterChip(
          selected: on,
          label: Text(label),
          onSelected: (_) {
            _setCompareMode(m);
          },
        ),
      );
    }

    return LiquidGlass(
      borderRadius: LiquidGlassTokens.controlRadius,
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          children: [
            toolBtn(CanvasTool.select, '选择', Icons.near_me_outlined),
            toolBtn(CanvasTool.rectangle, '矩形', Icons.crop_square),
            toolBtn(CanvasTool.arrow, '箭头', Icons.north_east),
            toolBtn(CanvasTool.text, '文字', Icons.text_fields),
            const SizedBox(width: 8),
            IconButton(
              tooltip: '适应窗口（双击画布）',
              onPressed: () => canvasKey.currentState?.fitToView(),
              icon: const Icon(Icons.fit_screen, size: 20),
            ),
            IconButton(
              tooltip: '100%',
              onPressed: () => canvasKey.currentState?.setZoom100(),
              icon: const Icon(Icons.fullscreen, size: 20),
            ),
            ListenableBuilder(
              listenable: sharedViewport,
              builder: (context, _) => Padding(
                padding: const EdgeInsets.symmetric(horizontal: 6),
                child: Text(
                  '${sharedViewport.zoomPercent}%',
                  style: Theme.of(context).textTheme.labelMedium?.copyWith(
                        fontFeatures: const [FontFeature.tabularFigures()],
                      ),
                ),
              ),
            ),
            const SizedBox(width: 4),
            modeBtn(ImageCompareMode.single, '单图'),
            modeBtn(ImageCompareMode.sideBySide, '并排'),
            modeBtn(ImageCompareMode.grid, '宫格'),
            if (compareMode != ImageCompareMode.single) ...[
              const SizedBox(width: 6),
              ListenableBuilder(
                listenable: sharedViewport,
                builder: (context, _) => FilterChip(
                  selected: sharedViewport.syncEnabled,
                  avatar: Icon(
                    sharedViewport.syncSuspended
                        ? Icons.link_off
                        : Icons.link,
                    size: 16,
                  ),
                  label: Text(
                    sharedViewport.syncSuspended
                        ? '临时解锁'
                        : (sharedViewport.syncEnabled ? '联动' : '独立'),
                  ),
                  onSelected: (on) {
                    sharedViewport.setSyncEnabled(on);
                    if (on) {
                      sharedViewport.reset();
                      canvasKey.currentState?.fitToView();
                    }
                  },
                ),
              ),
            ],
            const SizedBox(width: 8),
            Text('线宽', style: Theme.of(context).textTheme.labelSmall),
            SizedBox(
              width: 100,
              child: Slider(
                value: annStyle.lineWidth.clamp(1, 8),
                min: 1,
                max: 8,
                divisions: 7,
                onChanged: (v) => setState(() {
                  annStyle = AnnStyle(color: annStyle.color, lineWidth: v);
                }),
              ),
            ),
            ...const [
              Color(0xFFFF3B30),
              Color(0xFFFF9500),
              Color(0xFFFFCC00),
              Color(0xFF34C759),
              Color(0xFF007AFF),
              Color(0xFFAF52DE),
            ].map(
              (c) => Padding(
                padding: const EdgeInsets.only(left: 4),
                child: InkWell(
                  onTap: () => setState(() {
                    annStyle = AnnStyle(color: c, lineWidth: annStyle.lineWidth);
                  }),
                  child: Container(
                    width: 18,
                    height: 18,
                    decoration: BoxDecoration(
                      color: c,
                      shape: BoxShape.circle,
                      border: Border.all(
                        color: annStyle.color == c ? Colors.white : Colors.transparent,
                        width: 2,
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
