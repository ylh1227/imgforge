import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import 'host/host_controller.dart';
import 'pages/convert_page.dart';
import 'pages/extract_page.dart';
import 'pages/review_page.dart';
import 'pages/tasks_page.dart';
import 'pages/video_page.dart';
import 'theme.dart';
import 'theme/theme_controller.dart';
import 'widgets/glass_dropdown.dart';
import 'widgets/liquid_glass.dart';

class ImgForgeApp extends StatelessWidget {
  const ImgForgeApp({super.key});

  @override
  Widget build(BuildContext context) {
    final theme = context.watch<ThemeController>();
    return MaterialApp(
      title: 'ImgForge',
      debugShowCheckedModeBanner: false,
      theme: ImgForgeTheme.light(),
      darkTheme: ImgForgeTheme.dark(),
      themeMode: theme.themeMode,
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

  static const _items = [
    (Icons.transform_outlined, Icons.transform, '格式转换'),
    (Icons.photo_library_outlined, Icons.photo_library, '图片评审'),
    (Icons.videocam_outlined, Icons.videocam, '视频评审'),
    (Icons.table_chart_outlined, Icons.table_chart, '数据提取'),
    (Icons.history_outlined, Icons.history, '任务中心'),
  ];

  @override
  Widget build(BuildContext context) {
    final host = context.watch<HostController>();
    final scheme = Theme.of(context).colorScheme;
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final pages = [
      const ConvertPage(),
      const ReviewPage(),
      const VideoPage(),
      const ExtractPage(),
      const TasksPage(),
    ];

    // macOS Tahoe pattern: content extends under floating glass sidebar
    // (background extension) — glass is the functional layer only.
    return Scaffold(
      body: DecoratedBox(
        decoration: BoxDecoration(
          gradient: LinearGradient(
            begin: Alignment.topLeft,
            end: Alignment.bottomRight,
            colors: isDark
                ? const [Color(0xFF1A1B22), Color(0xFF0C0C10), Color(0xFF141821)]
                : const [Color(0xFFF3F5FB), Color(0xFFE6EAF4), Color(0xFFD5DDF0)],
          ),
        ),
        child: Stack(
          children: [
            Positioned(
              right: -120,
              top: -90,
              child: _GlowOrb(
                color: scheme.primary.withValues(alpha: isDark ? 0.28 : 0.20),
                size: 420,
              ),
            ),
            Positioned(
              left: 40,
              bottom: -140,
              child: _GlowOrb(
                color: const Color(0xFF64D2FF).withValues(alpha: isDark ? 0.16 : 0.14),
                size: 360,
              ),
            ),
            Positioned(
              left: MediaQuery.sizeOf(context).width * 0.35,
              top: MediaQuery.sizeOf(context).height * 0.4,
              child: _GlowOrb(
                color: scheme.secondary.withValues(alpha: isDark ? 0.10 : 0.08),
                size: 260,
              ),
            ),

            // Content sheet — full window, peeks behind sidebar.
            Positioned.fill(
              child: Padding(
                padding: const EdgeInsets.all(LiquidGlassTokens.inset),
                child: Material(
                  elevation: 0,
                  color: scheme.surface.withValues(alpha: isDark ? 0.88 : 0.90),
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(LiquidGlassTokens.windowRadius),
                    side: BorderSide(
                      color: Colors.white.withValues(alpha: isDark ? 0.06 : 0.55),
                      width: 0.8,
                    ),
                  ),
                  clipBehavior: Clip.antiAlias,
                  child: Stack(
                    children: [
                      // Soft extension wash under the sidebar lane.
                      Positioned(
                        left: 0,
                        top: 0,
                        bottom: 0,
                        width: LiquidGlassTokens.sidebarWidth + 28,
                        child: DecoratedBox(
                          decoration: BoxDecoration(
                            gradient: LinearGradient(
                              begin: Alignment.centerLeft,
                              end: Alignment.centerRight,
                              colors: [
                                scheme.primary.withValues(alpha: isDark ? 0.07 : 0.05),
                                scheme.primary.withValues(alpha: 0),
                              ],
                            ),
                          ),
                        ),
                      ),
                      // Content inset so it clears the floating glass sidebar.
                      Positioned.fill(
                        child: Padding(
                          padding: const EdgeInsets.only(
                            left: LiquidGlassTokens.sidebarWidth + 20,
                          ),
                          child: pages[index],
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),

            // Floating Liquid Glass sidebar (functional layer).
            Positioned(
              left: LiquidGlassTokens.inset + 8,
              top: LiquidGlassTokens.inset + 10,
              bottom: LiquidGlassTokens.inset + 10,
              width: LiquidGlassTokens.sidebarWidth,
              child: GlassEffectContainer(
                borderRadius: LiquidGlassTokens.sidebarRadius,
                padding: const EdgeInsets.fromLTRB(8, 14, 8, 14),
                child: Column(
                  children: [
                    Text(
                      'ImgForge',
                      textAlign: TextAlign.center,
                      style: Theme.of(context).textTheme.titleMedium?.copyWith(
                            fontSize: 12.5,
                            letterSpacing: -0.35,
                          ),
                    ),
                    const SizedBox(height: 8),
                    LiquidGlass(
                      borderRadius: 999,
                      tint: host.connected
                          ? scheme.primary.withValues(alpha: 0.20)
                          : const Color(0xFFFF9F0A).withValues(alpha: 0.22),
                      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                      child: Text(
                        host.connected ? '已连接' : '连接中',
                        textAlign: TextAlign.center,
                        style: Theme.of(context).textTheme.labelSmall?.copyWith(
                              color: host.connected
                                  ? scheme.primary
                                  : const Color(0xFFFF9F0A),
                              fontWeight: FontWeight.w600,
                              fontSize: 10,
                            ),
                      ),
                    ),
                    const SizedBox(height: 18),
                    Expanded(
                      child: ListView.separated(
                        itemCount: _items.length,
                        separatorBuilder: (_, __) => const SizedBox(height: 6),
                        itemBuilder: (context, i) {
                          final item = _items[i];
                          final selected = index == i;
                          return _SidebarItem(
                            icon: selected ? item.$2 : item.$1,
                            label: item.$3,
                            selected: selected,
                            onTap: () => setState(() => index = i),
                          );
                        },
                      ),
                    ),
                    const SizedBox(height: 8),
                    const _ThemeScheduleControl(),
                  ],
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _SidebarItem extends StatefulWidget {
  const _SidebarItem({
    required this.icon,
    required this.label,
    required this.selected,
    required this.onTap,
  });

  final IconData icon;
  final String label;
  final bool selected;
  final VoidCallback onTap;

  @override
  State<_SidebarItem> createState() => _SidebarItemState();
}

class _SidebarItemState extends State<_SidebarItem> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final color = widget.selected
        ? scheme.primary
        : scheme.onSurfaceVariant.withValues(alpha: _hover ? 1 : 0.85);

    final body = Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(widget.icon, size: 22, color: color),
        const SizedBox(height: 4),
        Text(
          widget.label,
          textAlign: TextAlign.center,
          maxLines: 2,
          style: TextStyle(
            fontSize: 10.5,
            height: 1.15,
            fontWeight: widget.selected ? FontWeight.w600 : FontWeight.w500,
            color: color,
            letterSpacing: -0.1,
          ),
        ),
      ],
    );

    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        onTap: widget.onTap,
        behavior: HitTestBehavior.opaque,
        child: AnimatedScale(
          scale: _hover && !widget.selected ? 1.03 : 1,
          duration: const Duration(milliseconds: 140),
          curve: Curves.easeOutCubic,
          child: GlassNavIndicator(
            selected: widget.selected,
            child: Padding(
              padding: EdgeInsets.symmetric(
                horizontal: widget.selected ? 0 : 10,
                vertical: widget.selected ? 0 : 8,
              ),
              child: body,
            ),
          ),
        ),
      ),
    );
  }
}

class _ThemeScheduleControl extends StatelessWidget {
  const _ThemeScheduleControl();

  @override
  Widget build(BuildContext context) {
    final theme = context.watch<ThemeController>();
    final scheme = Theme.of(context).colorScheme;
    final isDark = Theme.of(context).brightness == Brightness.dark;

    return Tooltip(
      message: theme.statusLine,
      waitDuration: const Duration(milliseconds: 400),
      child: LiquidGlass(
        borderRadius: LiquidGlassTokens.controlRadius,
        interactive: true,
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 6),
        child: PopupMenuButton<ThemeSchedule>(
          tooltip: '主题模式',
          padding: EdgeInsets.zero,
          offset: const Offset(0, -168),
          elevation: 0,
          color: GlassMenuStyle.background(context),
          shape: GlassMenuStyle.panelShape(context),
          onSelected: theme.setSchedule,
          itemBuilder: (context) => ThemeSchedule.values
              .map(
                (mode) => PopupMenuItem(
                  value: mode,
                  child: Row(
                    children: [
                      Icon(
                        mode.icon,
                        size: 18,
                        color: theme.schedule == mode ? scheme.primary : null,
                      ),
                      const SizedBox(width: 10),
                      Text(
                        mode.label,
                        style: TextStyle(
                          fontWeight:
                              theme.schedule == mode ? FontWeight.w600 : FontWeight.w500,
                        ),
                      ),
                    ],
                  ),
                ),
              )
              .toList(),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Icon(
                theme.schedule.icon,
                size: 16,
                color: scheme.primary,
              ),
              const SizedBox(width: 4),
              Flexible(
                child: Text(
                  theme.schedule == ThemeSchedule.sunCycle
                      ? (isDark ? '夜间' : '日间')
                      : theme.schedule.label,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.labelSmall?.copyWith(
                        fontSize: 10,
                        fontWeight: FontWeight.w600,
                        color: scheme.onSurfaceVariant,
                      ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _GlowOrb extends StatelessWidget {
  const _GlowOrb({required this.color, required this.size});

  final Color color;
  final double size;

  @override
  Widget build(BuildContext context) {
    return IgnorePointer(
      child: Container(
        width: size,
        height: size,
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          gradient: RadialGradient(
            colors: [color, color.withValues(alpha: 0)],
          ),
        ),
      ),
    );
  }
}
