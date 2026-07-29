import 'package:flutter/material.dart';

import 'widgets/liquid_glass.dart';

/// Liquid Glass–era tokens for ImgForge (Apple HIG Materials–inspired).
///
/// Content layer stays opaque; interactive controls use glass-like capsules /
/// soft fills so the whole product reads as one system.
class ImgForgeTheme {
  static const _seed = Color(0xFF007AFF);

  static const _bgLight = Color(0xFFE8ECF6);
  static const _bgDark = Color(0xFF08080A);
  static const _surfaceLight = Color(0xFFF9FAFD);
  static const _surfaceDark = Color(0xFF1C1C1E);

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
    final isDark = scheme.brightness == Brightness.dark;
    final hairline = isDark
        ? Colors.white.withValues(alpha: 0.18)
        : Colors.white.withValues(alpha: 0.78);
    final controlFill = isDark
        ? Colors.white.withValues(alpha: 0.08)
        : Colors.white.withValues(alpha: 0.55);
    final controlBorder = isDark
        ? Colors.white.withValues(alpha: 0.16)
        : Colors.black.withValues(alpha: 0.06);

    final stadium = ButtonStyle(
      elevation: const WidgetStatePropertyAll(0),
      visualDensity: VisualDensity.standard,
      minimumSize: const WidgetStatePropertyAll(Size(0, 40)),
      padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 16, vertical: 12)),
      shape: const WidgetStatePropertyAll(StadiumBorder()),
      tapTargetSize: MaterialTapTargetSize.shrinkWrap,
    );

    return ThemeData(
      useMaterial3: true,
      colorScheme: scheme,
      scaffoldBackgroundColor: scaffold,
      fontFamily: 'PingFang SC',
      visualDensity: VisualDensity.standard,
      dividerColor: scheme.outlineVariant.withValues(alpha: 0.28),
      textTheme: _textTheme(scheme),
      splashFactory: InkSparkle.splashFactory,

      // —— Inputs (content-layer glass wash) ——
      inputDecorationTheme: InputDecorationTheme(
        isDense: true,
        filled: true,
        fillColor: controlFill,
        floatingLabelBehavior: FloatingLabelBehavior.auto,
        labelStyle: TextStyle(color: scheme.onSurfaceVariant, fontSize: 13),
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(LiquidGlassTokens.controlRadius),
          borderSide: BorderSide(color: controlBorder),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(LiquidGlassTokens.controlRadius),
          borderSide: BorderSide(color: controlBorder),
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(LiquidGlassTokens.controlRadius),
          borderSide: BorderSide(color: scheme.primary.withValues(alpha: 0.75), width: 1.4),
        ),
        contentPadding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      ),

      // —— Buttons ——
      filledButtonTheme: FilledButtonThemeData(
        style: stadium.copyWith(
          backgroundColor: WidgetStateProperty.resolveWith((states) {
            if (states.contains(WidgetState.disabled)) {
              return scheme.primary.withValues(alpha: 0.28);
            }
            return scheme.primary.withValues(alpha: isDark ? 0.88 : 0.94);
          }),
          foregroundColor: const WidgetStatePropertyAll(Colors.white),
          shadowColor: WidgetStatePropertyAll(scheme.primary.withValues(alpha: 0.35)),
          elevation: const WidgetStatePropertyAll(0),
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: stadium.copyWith(
          foregroundColor: WidgetStatePropertyAll(scheme.onSurface),
          backgroundColor: WidgetStatePropertyAll(controlFill),
          side: WidgetStatePropertyAll(BorderSide(color: hairline, width: 0.8)),
        ),
      ),
      textButtonTheme: TextButtonThemeData(
        style: stadium.copyWith(
          foregroundColor: WidgetStatePropertyAll(scheme.primary),
        ),
      ),
      iconButtonTheme: IconButtonThemeData(
        style: IconButton.styleFrom(
          foregroundColor: scheme.onSurfaceVariant,
          backgroundColor: controlFill,
          shape: const StadiumBorder(),
          side: BorderSide(color: hairline, width: 0.7),
        ),
      ),
      floatingActionButtonTheme: FloatingActionButtonThemeData(
        elevation: 0,
        highlightElevation: 0,
        backgroundColor: scheme.primary.withValues(alpha: 0.92),
        foregroundColor: Colors.white,
        shape: const StadiumBorder(),
      ),

      // —— Chips / segmented (glass capsules) ——
      chipTheme: ChipThemeData(
        showCheckmark: false,
        elevation: 0,
        pressElevation: 0,
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 2),
        labelPadding: const EdgeInsets.symmetric(horizontal: 4),
        shape: const StadiumBorder(),
        side: BorderSide(color: controlBorder),
        backgroundColor: controlFill,
        selectedColor: scheme.primary.withValues(alpha: isDark ? 0.28 : 0.16),
        labelStyle: TextStyle(
          fontSize: 12.5,
          fontWeight: FontWeight.w500,
          color: scheme.onSurface,
        ),
        secondaryLabelStyle: TextStyle(
          fontSize: 12.5,
          fontWeight: FontWeight.w600,
          color: scheme.primary,
        ),
      ),
      segmentedButtonTheme: SegmentedButtonThemeData(
        style: ButtonStyle(
          visualDensity: VisualDensity.compact,
          padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 12)),
          side: WidgetStatePropertyAll(BorderSide(color: hairline, width: 0.8)),
          backgroundColor: WidgetStateProperty.resolveWith((states) {
            if (states.contains(WidgetState.selected)) {
              return scheme.primary.withValues(alpha: isDark ? 0.30 : 0.14);
            }
            return controlFill;
          }),
          foregroundColor: WidgetStateProperty.resolveWith((states) {
            if (states.contains(WidgetState.selected)) return scheme.primary;
            return scheme.onSurface;
          }),
          shape: const WidgetStatePropertyAll(StadiumBorder()),
        ),
      ),

      // —— Content cards (opaque grouped, soft rim — not Liquid Glass) ——
      cardTheme: CardThemeData(
        elevation: 0,
        color: isDark ? const Color(0xFF2A2A2C) : Colors.white.withValues(alpha: 0.72),
        margin: EdgeInsets.zero,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(18),
          side: BorderSide(color: hairline.withValues(alpha: isDark ? 0.5 : 0.9), width: 0.7),
        ),
      ),

      // —— Slider (interactive glass thumb) ——
      sliderTheme: SliderThemeData(
        activeTrackColor: scheme.primary,
        inactiveTrackColor: scheme.onSurface.withValues(alpha: 0.10),
        thumbColor: Colors.white,
        overlayColor: scheme.primary.withValues(alpha: 0.12),
        trackHeight: 4.5,
        thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 9, elevation: 2),
        overlayShape: const RoundSliderOverlayShape(overlayRadius: 18),
      ),

      listTileTheme: ListTileThemeData(
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
        selectedTileColor: scheme.primary.withValues(alpha: isDark ? 0.18 : 0.10),
        iconColor: scheme.onSurfaceVariant,
        contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 2),
      ),
      dialogTheme: DialogThemeData(
        backgroundColor: isDark ? const Color(0xEE2C2C2E) : const Color(0xF2FFFFFF),
        elevation: 0,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(22),
          side: BorderSide(color: hairline, width: 0.7),
        ),
      ),
      snackBarTheme: SnackBarThemeData(
        behavior: SnackBarBehavior.floating,
        elevation: 0,
        backgroundColor: isDark ? const Color(0xEE3A3A3C) : const Color(0xF21C1C1E),
        contentTextStyle: const TextStyle(color: Colors.white, fontSize: 13),
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
      ),
      popupMenuTheme: PopupMenuThemeData(
        elevation: 0,
        color: isDark ? const Color(0xF02C2C2E) : const Color(0xF5FFFFFF),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(LiquidGlassTokens.controlRadius),
          side: BorderSide(color: hairline, width: 0.7),
        ),
      ),
      menuTheme: MenuThemeData(
        style: MenuStyle(
          elevation: const WidgetStatePropertyAll(0),
          backgroundColor: WidgetStatePropertyAll(
            isDark ? const Color(0xF02C2C2E) : const Color(0xF5FFFFFF),
          ),
          shape: WidgetStatePropertyAll(
            RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(LiquidGlassTokens.controlRadius),
              side: BorderSide(color: hairline, width: 0.7),
            ),
          ),
          padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(vertical: 6)),
        ),
      ),
      dropdownMenuTheme: DropdownMenuThemeData(
        inputDecorationTheme: InputDecorationTheme(
          isDense: true,
          filled: true,
          fillColor: controlFill,
          contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
          border: OutlineInputBorder(
            borderRadius: BorderRadius.circular(LiquidGlassTokens.controlRadius),
            borderSide: BorderSide(color: controlBorder),
          ),
          enabledBorder: OutlineInputBorder(
            borderRadius: BorderRadius.circular(LiquidGlassTokens.controlRadius),
            borderSide: BorderSide(color: controlBorder),
          ),
        ),
        menuStyle: MenuStyle(
          elevation: const WidgetStatePropertyAll(0),
          backgroundColor: WidgetStatePropertyAll(
            isDark ? const Color(0xF22C2C2E) : const Color(0xF8FFFFFF),
          ),
          shape: WidgetStatePropertyAll(
            RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(LiquidGlassTokens.controlRadius),
              side: BorderSide(color: hairline, width: 0.7),
            ),
          ),
          padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(vertical: 6)),
        ),
      ),
      dataTableTheme: DataTableThemeData(
        headingTextStyle: TextStyle(
          fontSize: 12,
          fontWeight: FontWeight.w600,
          color: scheme.onSurface,
        ),
        dataTextStyle: TextStyle(fontSize: 12, color: scheme.onSurface),
        dividerThickness: 0.5,
        headingRowColor: WidgetStatePropertyAll(
          scheme.primary.withValues(alpha: isDark ? 0.12 : 0.06),
        ),
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(14),
          border: Border.all(color: hairline, width: 0.7),
        ),
      ),
      switchTheme: SwitchThemeData(
        thumbColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) return Colors.white;
          return isDark ? Colors.white70 : Colors.white;
        }),
        trackColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) {
            return scheme.primary.withValues(alpha: 0.85);
          }
          return scheme.onSurface.withValues(alpha: 0.18);
        }),
      ),
      checkboxTheme: CheckboxThemeData(
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(5)),
        side: BorderSide(color: controlBorder, width: 1.2),
        fillColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.selected)) return scheme.primary;
          return Colors.transparent;
        }),
      ),
      progressIndicatorTheme: ProgressIndicatorThemeData(
        color: scheme.primary,
        linearTrackColor: scheme.onSurface.withValues(alpha: 0.08),
      ),
      dividerTheme: DividerThemeData(
        color: scheme.outlineVariant.withValues(alpha: 0.28),
        space: 1,
        thickness: 0.7,
      ),
      navigationRailTheme: NavigationRailThemeData(
        backgroundColor: Colors.transparent,
        elevation: 0,
        indicatorColor: scheme.primary.withValues(alpha: isDark ? 0.28 : 0.16),
        indicatorShape: const StadiumBorder(),
        selectedIconTheme: IconThemeData(color: scheme.primary, size: 22),
        selectedLabelTextStyle: TextStyle(
          color: scheme.primary,
          fontWeight: FontWeight.w600,
          fontSize: 11,
          letterSpacing: -0.1,
        ),
        unselectedIconTheme: IconThemeData(
          color: scheme.onSurfaceVariant.withValues(alpha: 0.85),
          size: 22,
        ),
        unselectedLabelTextStyle: TextStyle(
          color: scheme.onSurfaceVariant,
          fontWeight: FontWeight.w500,
          fontSize: 11,
        ),
      ),
    );
  }

  static TextTheme _textTheme(ColorScheme scheme) {
    final base = scheme.brightness == Brightness.dark
        ? Typography.material2021(platform: TargetPlatform.macOS).white
        : Typography.material2021(platform: TargetPlatform.macOS).black;
    return base
        .apply(
          fontFamily: 'PingFang SC',
          bodyColor: scheme.onSurface,
          displayColor: scheme.onSurface,
        )
        .copyWith(
          headlineSmall: TextStyle(
            fontSize: 26,
            fontWeight: FontWeight.w700,
            letterSpacing: -0.6,
            color: scheme.onSurface,
            height: 1.15,
          ),
          titleMedium: TextStyle(
            fontSize: 15,
            fontWeight: FontWeight.w700,
            letterSpacing: -0.2,
            color: scheme.onSurface,
          ),
          titleSmall: TextStyle(
            fontSize: 13,
            fontWeight: FontWeight.w600,
            letterSpacing: -0.1,
            color: scheme.onSurface,
          ),
          bodyMedium: TextStyle(
            fontSize: 13,
            fontWeight: FontWeight.w400,
            height: 1.35,
            color: scheme.onSurface,
          ),
          labelSmall: TextStyle(
            fontSize: 11,
            fontWeight: FontWeight.w500,
            letterSpacing: 0,
            color: scheme.onSurfaceVariant,
          ),
        );
  }
}
