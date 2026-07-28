import 'package:flutter/material.dart';

/// Align with egui macOS shell: cool gray surfaces + system blue accent.
class ImgForgeTheme {
  /// macOS system blue (light).
  static const _seed = Color(0xFF007AFF);
  static const _bgLight = Color(0xFFF2F2F7);
  static const _bgDark = Color(0xFF1C1C1E);
  static const _surfaceLight = Color(0xFFFAFAFC);
  static const _surfaceDark = Color(0xFF2C2C2E);

  static ThemeData light() {
    final scheme = ColorScheme.fromSeed(
      seedColor: _seed,
      brightness: Brightness.light,
      surface: _surfaceLight,
      primary: const Color(0xFF007AFF),
      onPrimary: Colors.white,
      secondary: const Color(0xFF5856D6),
    );
    return _base(scheme, _bgLight);
  }

  static ThemeData dark() {
    final scheme = ColorScheme.fromSeed(
      seedColor: const Color(0xFF0A84FF),
      brightness: Brightness.dark,
      surface: _surfaceDark,
      primary: const Color(0xFF0A84FF),
      onPrimary: Colors.white,
      secondary: const Color(0xFF5E5CE6),
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
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          backgroundColor: scheme.primary,
          foregroundColor: scheme.onPrimary,
        ),
      ),
      cardTheme: CardThemeData(
        elevation: 0,
        color: scheme.surface,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(10),
          side: BorderSide(color: scheme.outlineVariant.withValues(alpha: 0.45)),
        ),
      ),
      navigationRailTheme: NavigationRailThemeData(
        backgroundColor: scheme.surface,
        indicatorColor: scheme.primaryContainer,
        selectedIconTheme: IconThemeData(color: scheme.primary),
        selectedLabelTextStyle: TextStyle(
          color: scheme.primary,
          fontWeight: FontWeight.w600,
          fontSize: 12,
        ),
        unselectedIconTheme: IconThemeData(color: scheme.onSurfaceVariant),
      ),
    );
  }
}
