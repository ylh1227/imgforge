import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import 'host/host_controller.dart';
import 'pages/convert_page.dart';
import 'pages/extract_page.dart';
import 'pages/review_page.dart';
import 'pages/tasks_page.dart';
import 'pages/video_page.dart';
import 'theme.dart';

class ImgForgeApp extends StatelessWidget {
  const ImgForgeApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'ImgForge',
      debugShowCheckedModeBanner: false,
      theme: ImgForgeTheme.light(),
      darkTheme: ImgForgeTheme.dark(),
      themeMode: ThemeMode.system,
      home: const ShellScaffold(),
    );
  }
}

class ShellScaffold extends StatefulWidget {
  const ShellScaffold({super.key});

  @override
  State<ShellScaffold> createState() => _ShellScaffoldState();
}

class _ShellScaffoldState extends State<ShellScaffold> {
  int index = 0;

  static const _destinations = [
    NavigationRailDestination(
      icon: Icon(Icons.transform_outlined),
      selectedIcon: Icon(Icons.transform),
      label: Text('格式转换'),
    ),
    NavigationRailDestination(
      icon: Icon(Icons.photo_library_outlined),
      selectedIcon: Icon(Icons.photo_library),
      label: Text('图片评审'),
    ),
    NavigationRailDestination(
      icon: Icon(Icons.videocam_outlined),
      selectedIcon: Icon(Icons.videocam),
      label: Text('视频评审'),
    ),
    NavigationRailDestination(
      icon: Icon(Icons.table_chart_outlined),
      selectedIcon: Icon(Icons.table_chart),
      label: Text('数据提取'),
    ),
    NavigationRailDestination(
      icon: Icon(Icons.history_outlined),
      selectedIcon: Icon(Icons.history),
      label: Text('任务中心'),
    ),
  ];

  @override
  Widget build(BuildContext context) {
    final host = context.watch<HostController>();
    final pages = [
      const ConvertPage(),
      const ReviewPage(),
      const VideoPage(),
      const ExtractPage(),
      const TasksPage(),
    ];

    return Scaffold(
      body: Row(
        children: [
          NavigationRail(
            selectedIndex: index,
            onDestinationSelected: (i) => setState(() => index = i),
            labelType: NavigationRailLabelType.all,
            leading: Padding(
              padding: const EdgeInsets.symmetric(vertical: 16),
              child: Column(
                children: [
                  Text(
                    'ImgForge',
                    style: Theme.of(context).textTheme.titleMedium?.copyWith(
                          fontWeight: FontWeight.w700,
                          letterSpacing: -0.3,
                        ),
                  ),
                  const SizedBox(height: 4),
                  Text(
                    host.connected ? 'Host 已连接' : '连接中…',
                    style: Theme.of(context).textTheme.labelSmall?.copyWith(
                          color: host.connected
                              ? Theme.of(context).colorScheme.primary
                              : const Color(0xFFFF9F0A),
                        ),
                  ),
                ],
              ),
            ),
            destinations: _destinations,
          ),
          const VerticalDivider(width: 1),
          Expanded(child: pages[index]),
        ],
      ),
    );
  }
}
