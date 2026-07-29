import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../host/host_controller.dart';
import '../widgets/glass_dropdown.dart';
import '../widgets/glass_list_panel.dart';
import '../widgets/liquid_glass.dart';
import '../widgets/page_chrome.dart';
import '../widgets/section_card.dart';

class ReviewPage extends StatefulWidget {
  const ReviewPage({super.key});

  @override
  State<ReviewPage> createState() => _ReviewPageState();
}

class _ReviewPageState extends State<ReviewPage> {
  List<Map<String, dynamic>> batches = [];
  List<Map<String, dynamic>> images = [];
  List<Map<String, dynamic>> annotations = [];
  int? batchId;
  int? imageId;
  final remarkCtrl = TextEditingController();
  String status = 'Pending';
  String info = '';

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _reloadBatches());
  }

  @override
  void dispose() {
    remarkCtrl.dispose();
    super.dispose();
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
    if (path == null) return;
    final host = context.read<HostController>();
    final res = await host.call('review.import_folder', {
      'folder': path,
      'recursive': true,
    });
    batchId = (res['batch_id'] as num?)?.toInt();
    setState(() => info = '已导入批次 $batchId');
    await _reloadBatches();
    if (batchId != null) await _loadImages(batchId!);
  }

  Future<void> _loadImages(int id) async {
    final host = context.read<HostController>();
    images = await host.callList('review.list_images', {'batch_id': id});
    setState(() {
      batchId = id;
      imageId = images.isNotEmpty ? (images.first['id'] as num?)?.toInt() : null;
    });
    if (imageId != null) await _selectImage(imageId!);
  }

  Future<void> _selectImage(int id) async {
    final host = context.read<HostController>();
    final item = images.firstWhere(
      (e) => (e['id'] as num?)?.toInt() == id,
      orElse: () => <String, dynamic>{},
    );
    annotations = await host.callList('review.load_annotations', {'image_id': id});
    setState(() {
      imageId = id;
      remarkCtrl.text = item['remark']?.toString() ?? '';
      status = item['status']?.toString() ?? 'Pending';
    });
    if (batchId != null) {
      await host.call('review.session_save', {
        'batch_id': batchId,
        'image_id': id,
      });
    }
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
    if (batchId != null) await _loadImages(batchId!);
  }

  Future<void> _exportCsv() async {
    if (batchId == null) return;
    final path = await FilePicker.platform.saveFile(
      dialogTitle: '导出 CSV',
      fileName: 'review_$batchId.csv',
    );
    if (path == null) return;
    await context.read<HostController>().call('review.export_csv', {
      'batch_id': batchId,
      'path': path,
    });
    setState(() => info = '已导出 $path');
  }

  Future<void> _addRectAnnotation() async {
    if (imageId == null) return;
    await context.read<HostController>().call('review.add_annotation', {
      'id': 0,
      'image_item_id': imageId,
      'kind': 'Rectangle',
      'position': {'kind': 'rectangle', 'x0': 0.2, 'y0': 0.2, 'x1': 0.5, 'y1': 0.5},
      'style': {
        'color': [255, 59, 48, 255],
        'line_width': 2.0,
      },
      'content': '',
      'created_at': DateTime.now().toUtc().toIso8601String(),
    });
    await _selectImage(imageId!);
  }

  @override
  Widget build(BuildContext context) {
    Map<String, dynamic>? selected;
    for (final img in images) {
      if ((img['id'] as num?)?.toInt() == imageId) {
        selected = img;
        break;
      }
    }
    final path = selected?['file_path']?.toString();

    return PageChrome(
      title: '图片评审',
      subtitle: '批次、状态、备注、标注与导出',
      actions: [
        GlassCapsuleButton(
          label: '导入文件夹',
          primary: true,
          onPressed: _importFolder,
        ),
        const SizedBox(width: 8),
        GlassCapsuleButton(label: '导出 CSV', onPressed: _exportCsv),
        const SizedBox(width: 12),
      ],
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GlassListPanel(
            width: 280,
            child: ListView(
              padding: EdgeInsets.zero,
              children: [
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
                const GlassSectionLabel('图片'),
                ...images.map(
                  (img) => GlassListTile(
                    selected: imageId == (img['id'] as num?)?.toInt(),
                    title: img['file_path']
                            ?.toString()
                            .split(Platform.pathSeparator)
                            .last ??
                        '',
                    subtitle: img['status']?.toString() ?? '',
                    onTap: () => _selectImage((img['id'] as num).toInt()),
                  ),
                ),
                if (info.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.fromLTRB(10, 8, 10, 4),
                    child: Text(info, style: Theme.of(context).textTheme.labelSmall),
                  ),
              ],
            ),
          ),
          Expanded(
            child: ListView(
              padding: const EdgeInsets.fromLTRB(8, 0, 16, 16),
              children: [
                SectionCard(
                  title: '预览',
                  child: SizedBox(
                    height: 340,
                    child: path != null && File(path).existsSync()
                        ? InteractiveViewer(
                            child: Image.file(File(path), fit: BoxFit.contain),
                          )
                        : const Center(child: Text('选择图片')),
                  ),
                ),
                SectionCard(
                  title: '属性与标注',
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
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
                            label: '添加矩形标注',
                            onPressed: _addRectAnnotation,
                          ),
                        ],
                      ),
                      const SizedBox(height: 8),
                      Text('标注 ${annotations.length} 条'),
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
}
