import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../host/host_controller.dart';
import '../widgets/page_chrome.dart';
import '../widgets/section_card.dart';

class ExtractPage extends StatefulWidget {
  const ExtractPage({super.key});

  @override
  State<ExtractPage> createState() => _ExtractPageState();
}

class _ExtractPageState extends State<ExtractPage> {
  final rootCtrl = TextEditingController();
  List<String> files = [];
  Map<String, dynamic>? batch;
  Map<String, dynamic>? summary;
  String info = '';

  @override
  void dispose() {
    rootCtrl.dispose();
    super.dispose();
  }

  Future<void> _pick() async {
    final path = await FilePicker.platform.getDirectoryPath();
    if (path != null) setState(() => rootCtrl.text = path);
  }

  Future<void> _scan() async {
    final host = context.read<HostController>();
    final res = await host.call('extract.scan', {'root': rootCtrl.text.trim()});
    final list = (res['files'] as List?)?.map((e) => e.toString()).toList() ?? [];
    setState(() {
      files = list;
      info = '找到 ${files.length} 个候选文件';
    });
  }

  Future<void> _extract(String path) async {
    final host = context.read<HostController>();
    batch = await host.call('extract.from_path', {'path': path});
    summary = await host.call('extract.summary', {'path': path});
    setState(() => info = '已解析 $path');
  }

  @override
  Widget build(BuildContext context) {
    final records = (batch?['records'] as List?) ?? [];
    final columns = (summary?['columns'] as List?) ?? [];
    final rows = (summary?['rows'] as List?) ?? [];

    return PageChrome(
      title: '数据提取',
      subtitle: '扫描 Imatest 结果、汇总与阈值视图',
      child: ListView(
        padding: const EdgeInsets.all(20),
        children: [
          SectionCard(
            title: '扫描',
            child: Column(
              children: [
                Row(
                  children: [
                    Expanded(
                      child: TextField(
                        controller: rootCtrl,
                        decoration: const InputDecoration(labelText: '结果目录'),
                      ),
                    ),
                    const SizedBox(width: 8),
                    OutlinedButton(onPressed: _pick, child: const Text('选择')),
                    const SizedBox(width: 8),
                    FilledButton(onPressed: _scan, child: const Text('扫描')),
                  ],
                ),
                const SizedBox(height: 8),
                Text(info),
              ],
            ),
          ),
          SectionCard(
            title: '文件 (${files.length})',
            child: SizedBox(
              height: 220,
              child: ListView.builder(
                itemCount: files.length,
                itemBuilder: (_, i) => ListTile(
                  dense: true,
                  title: Text(files[i], overflow: TextOverflow.ellipsis),
                  onTap: () => _extract(files[i]),
                ),
              ),
            ),
          ),
          SectionCard(
            title: '记录 (${records.length})',
            child: SizedBox(
              height: 180,
              child: ListView.builder(
                itemCount: records.length,
                itemBuilder: (_, i) {
                  final r = (records[i] as Map).cast<String, dynamic>();
                  return ListTile(
                    dense: true,
                    title: Text(r['metric_name']?.toString() ?? r.toString()),
                    subtitle: Text('${r['module'] ?? ''}  ${r['value'] ?? ''}'),
                  );
                },
              ),
            ),
          ),
          SectionCard(
            title: '汇总表 列=${columns.length} 行=${rows.length}',
            child: SizedBox(
              height: 240,
              child: SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: SingleChildScrollView(
                  child: DataTable(
                    columns: [
                      const DataColumn(label: Text('样本')),
                      ...columns.map(
                        (c) => DataColumn(
                          label: Text((c as Map)['label']?.toString() ?? ''),
                        ),
                      ),
                    ],
                    rows: rows.map((row) {
                      final r = (row as Map).cast<String, dynamic>();
                      final values = (r['values'] as Map?) ?? {};
                      return DataRow(
                        cells: [
                          DataCell(Text(r['sample_name']?.toString() ?? '')),
                          ...columns.map((c) {
                            final key = (c as Map)['key']?.toString() ?? '';
                            final cell = values[key];
                            return DataCell(Text(cell?.toString() ?? ''));
                          }),
                        ],
                      );
                    }).toList(),
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
