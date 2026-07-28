import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../host/host_controller.dart';
import '../widgets/glass_list_panel.dart';
import '../widgets/liquid_glass.dart';
import '../widgets/page_chrome.dart';
import '../widgets/section_card.dart';

class TasksPage extends StatefulWidget {
  const TasksPage({super.key});

  @override
  State<TasksPage> createState() => _TasksPageState();
}

class _TasksPageState extends State<TasksPage> {
  List<Map<String, dynamic>> convertHistory = [];
  List<Map<String, dynamic>> actionHistory = [];
  Map<String, dynamic>? remote;
  Map<String, dynamic>? jira;
  Map<String, dynamic>? doctor;
  String info = '';

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _reload());
  }

  Future<void> _reload() async {
    final host = context.read<HostController>();
    try {
      final hist = await host.call('tasks.history');
      convertHistory = ((hist['convert'] as List?) ?? [])
          .map((e) => (e as Map).cast<String, dynamic>())
          .toList();
      actionHistory = ((hist['actions'] as List?) ?? [])
          .map((e) => (e as Map).cast<String, dynamic>())
          .toList();
      remote = await host.call('remote.status');
      jira = await host.call('jira.status');
      doctor = await host.call('app.doctor');
      setState(() => info = '已刷新');
    } catch (e) {
      setState(() => info = '$e');
    }
  }

  Future<void> _probeJira() async {
    final res = await context.read<HostController>().call('jira.probe');
    setState(() => info = res['ok'] == true
        ? 'JIRA OK: ${res['display_name']}'
        : 'JIRA 失败: ${res['message']}');
  }

  Future<void> _clearHistory() async {
    await context.read<HostController>().call('tasks.clear_convert_history');
    await _reload();
  }

  @override
  Widget build(BuildContext context) {
    final tools = ((doctor?['tools'] as List?) ?? [])
        .map((e) => (e as Map).cast<String, dynamic>())
        .toList();

    return PageChrome(
      title: '任务中心',
      subtitle: '转换历史、操作日志、远端 / JIRA / Doctor',
      actions: [
        GlassCapsuleButton(label: '刷新', onPressed: _reload),
        const SizedBox(width: 8),
        GlassCapsuleButton(label: '清空转换历史', onPressed: _clearHistory),
        const SizedBox(width: 12),
      ],
      child: ListView(
        padding: const EdgeInsets.all(20),
        children: [
          Text(info, style: Theme.of(context).textTheme.labelLarge),
          const SizedBox(height: 8),
          SectionCard(
            title: '环境 Doctor',
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'v${doctor?['version'] ?? '-'}  ${doctor?['platform'] ?? ''}  cores=${doctor?['cpu_cores'] ?? '-'}',
                ),
                const SizedBox(height: 8),
                ...tools.map(
                  (t) => Text(
                    '${t['name']}: ${t['available'] == true ? 'OK' : '缺'}  ${t['detail']}',
                  ),
                ),
              ],
            ),
          ),
          SectionCard(
            title: '远端 / JIRA',
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  '远端：${remote?['status']}  configured=${remote?['configured']}  prefer=${remote?['prefer_remote']}',
                ),
                Text(
                  'JIRA：${jira?['status']}  project=${jira?['project_key'] ?? '-'}  creds=${jira?['has_credentials']}',
                ),
                const SizedBox(height: 8),
                GlassCapsuleButton(label: '探测 JIRA', onPressed: _probeJira),
              ],
            ),
          ),
          SectionCard(
            title: '转换历史 (${convertHistory.length})',
            child: SizedBox(
              height: 220,
              child: ListView.builder(
                itemCount: convertHistory.length,
                itemBuilder: (_, i) {
                  final h = convertHistory[i];
                  return GlassListTile(
                    title: '${h['input_dir']} → ${h['output_dir']}',
                    subtitle:
                        '成功 ${h['successes']}/${h['total']}  失败 ${h['failures']}  ${h['elapsed_ms']}ms',
                  );
                },
              ),
            ),
          ),
          SectionCard(
            title: '操作历史 (${actionHistory.length})',
            child: SizedBox(
              height: 180,
              child: ListView.builder(
                itemCount: actionHistory.length,
                itemBuilder: (_, i) {
                  final h = actionHistory[i];
                  return GlassListTile(
                    title: '${h['module']} / ${h['operation']}',
                    subtitle: '${h['target']}  ${h['status']}',
                  );
                },
              ),
            ),
          ),
        ],
      ),
    );
  }
}
