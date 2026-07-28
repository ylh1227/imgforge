import 'package:flutter/material.dart';

class ImgForgeTheme {
  static const _seed = Color(0xFF1C4D3A);
  static const _bgLight = Color(0xFFF3F1EC);
  static const _bgDark = Color(0xFF121411);
  static const _surfaceLight = Color(0xFFFBF9F5);
  static const _surfaceDark = Color(0xFF1B1E1A);

  static ThemeData light() {
    final scheme = ColorScheme.fromSeed(
      seedColor: _seed,
      brightness: Brightness.light,
      surface: _surfaceLight,
    );
    return _base(scheme, _bgLight);
  }

  static ThemeData dark() {
    final scheme = ColorScheme.fromSeed(
      seedColor: _seed,
      brightness: Brightness.dark,
      surface: _surfaceDark,
    );
    return _base(scheme, _bgDark);
  }

  static ThemeData _base(ColorScheme scheme, Color scaffold) {
    return ThemeData(
      useMaterial3: true,
      colorScheme: scheme,
      scaffoldBackgroundColor: scaffold,
      fontFamily: 'PingFang SC',
      visualDensity: VisualDensity.compact,
      inputDecorationTheme: const InputDecorationTheme(
        isDense: true,
        border: OutlineInputBorder(),
        contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 10),
      ),
      cardTheme: CardThemeData(
        elevation: 0,
        color: scheme.surface,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(10),
          side: BorderSide(color: scheme.outlineVariant.withValues(alpha: 0.5)),
        ),
      ),
      navigationRailTheme: NavigationRailThemeData(
        backgroundColor: scheme.surface,
        indicatorColor: scheme.primaryContainer,
        selectedIconTheme: IconThemeData(color: scheme.onPrimaryContainer),
        unselectedIconTheme: IconThemeData(color: scheme.onSurfaceVariant),
      ),
    );
  }
}
