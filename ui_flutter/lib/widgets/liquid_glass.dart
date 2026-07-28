import 'dart:math' as math;
import 'dart:ui';

import 'package:flutter/material.dart';

/// Apple Liquid Glass–inspired surfaces (Flutter approximation of HIG Materials).
///
/// Hierarchy: glass only on the functional layer (nav / toolbars / transient controls).
/// Content stays opaque. Prefer [LiquidGlassVariant.regular] for text-heavy chrome;
/// [LiquidGlassVariant.clear] only over rich media. Do not stack glass on glass —
/// group siblings in [GlassEffectContainer].
enum LiquidGlassVariant { regular, clear }

/// Shared radii for concentricity with the window / content sheet.
abstract final class LiquidGlassTokens {
  static const double windowRadius = 28;
  static const double sidebarRadius = 26;
  static const double toolbarRadius = 22;
  static const double controlRadius = 14;
  static const double inset = 12;
  static const double sidebarWidth = 96;
}

class LiquidGlass extends StatelessWidget {
  const LiquidGlass({
    super.key,
    required this.child,
    this.variant = LiquidGlassVariant.regular,
    this.borderRadius = LiquidGlassTokens.toolbarRadius,
    this.padding,
    this.tint,
    this.interactive = false,
    this.clipBehavior = Clip.antiAlias,
  });

  final Widget child;
  final LiquidGlassVariant variant;
  final double borderRadius;
  final EdgeInsetsGeometry? padding;
  final Color? tint;
  /// Light press scale — mirrors SwiftUI `.glassEffect(.regular.interactive())`.
  final bool interactive;
  final Clip clipBehavior;

  @override
  Widget build(BuildContext context) {
    final glass = _GlassPlate(
      variant: variant,
      borderRadius: borderRadius,
      padding: padding,
      tint: tint,
      clipBehavior: clipBehavior,
      child: child,
    );
    if (!interactive) return glass;
    return _GlassPressable(child: glass);
  }
}

class _GlassPressable extends StatefulWidget {
  const _GlassPressable({required this.child});
  final Widget child;

  @override
  State<_GlassPressable> createState() => _GlassPressableState();
}

class _GlassPressableState extends State<_GlassPressable> {
  bool _down = false;

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      behavior: HitTestBehavior.deferToChild,
      onTapDown: (_) => setState(() => _down = true),
      onTapUp: (_) => setState(() => _down = false),
      onTapCancel: () => setState(() => _down = false),
      child: AnimatedScale(
        scale: _down ? 0.97 : 1,
        duration: const Duration(milliseconds: 120),
        curve: Curves.easeOutCubic,
        child: AnimatedOpacity(
          opacity: _down ? 0.92 : 1,
          duration: const Duration(milliseconds: 120),
          child: widget.child,
        ),
      ),
    );
  }
}

class _GlassPlate extends StatelessWidget {
  const _GlassPlate({
    required this.child,
    required this.variant,
    required this.borderRadius,
    required this.clipBehavior,
    this.padding,
    this.tint,
  });

  final Widget child;
  final LiquidGlassVariant variant;
  final double borderRadius;
  final EdgeInsetsGeometry? padding;
  final Color? tint;
  final Clip clipBehavior;

  @override
  Widget build(BuildContext context) {
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final regular = variant == LiquidGlassVariant.regular;

    // Regular: more blur + luminosity lift for legibility (sidebar / toolbars).
    // Clear: thinner fill so media shows through.
    final sigma = regular ? (isDark ? 36.0 : 42.0) : (isDark ? 22.0 : 26.0);
    final baseFill = tint ??
        (regular
            ? (isDark ? const Color(0x5A3A3A3C) : const Color(0x73FFFFFF))
            : (isDark ? const Color(0x332C2C2E) : const Color(0x38FFFFFF)));

    final radius = BorderRadius.circular(borderRadius);

    return CustomPaint(
      painter: _GlassShadowPainter(
        borderRadius: borderRadius,
        isDark: isDark,
      ),
      child: ClipRRect(
        borderRadius: radius,
        clipBehavior: clipBehavior,
        child: BackdropFilter(
          filter: ImageFilter.blur(sigmaX: sigma, sigmaY: sigma),
          child: DecoratedBox(
            decoration: BoxDecoration(
              borderRadius: radius,
              gradient: LinearGradient(
                begin: Alignment.topLeft,
                end: Alignment.bottomRight,
                colors: [
                  Color.alphaBlend(
                    Colors.white.withValues(alpha: isDark ? 0.14 : 0.55),
                    baseFill,
                  ),
                  baseFill,
                  Color.alphaBlend(
                    Colors.black.withValues(alpha: isDark ? 0.28 : 0.04),
                    baseFill,
                  ),
                ],
                stops: const [0.0, 0.42, 1.0],
              ),
              border: Border.all(
                width: 0.7,
                color: isDark
                    ? Colors.white.withValues(alpha: 0.22)
                    : Colors.white.withValues(alpha: 0.85),
              ),
            ),
            child: Stack(
              fit: StackFit.passthrough,
              children: [
                Positioned.fill(
                  child: IgnorePointer(
                    child: CustomPaint(
                      painter: _SpecularRimPainter(
                        borderRadius: borderRadius,
                        isDark: isDark,
                        intense: regular,
                      ),
                    ),
                  ),
                ),
                if (padding != null)
                  Padding(padding: padding!, child: child)
                else
                  child,
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _GlassShadowPainter extends CustomPainter {
  _GlassShadowPainter({required this.borderRadius, required this.isDark});

  final double borderRadius;
  final bool isDark;

  @override
  void paint(Canvas canvas, Size size) {
    final r = RRect.fromRectAndRadius(
      Offset.zero & size,
      Radius.circular(borderRadius),
    );
    final soft = Paint()
      ..color = Colors.black.withValues(alpha: isDark ? 0.45 : 0.10)
      ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 22);
    canvas.drawRRect(r.shift(const Offset(0, 10)), soft);

    final contact = Paint()
      ..color = Colors.black.withValues(alpha: isDark ? 0.28 : 0.06)
      ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 6);
    canvas.drawRRect(r.shift(const Offset(0, 2)), contact);
  }

  @override
  bool shouldRepaint(covariant _GlassShadowPainter old) =>
      old.borderRadius != borderRadius || old.isDark != isDark;
}

/// Arc highlight along the upper rim — approximates specular response.
class _SpecularRimPainter extends CustomPainter {
  _SpecularRimPainter({
    required this.borderRadius,
    required this.isDark,
    required this.intense,
  });

  final double borderRadius;
  final bool isDark;
  final bool intense;

  @override
  void paint(Canvas canvas, Size size) {
    final r = RRect.fromRectAndRadius(
      Rect.fromLTWH(0.6, 0.6, size.width - 1.2, size.height - 1.2),
      Radius.circular(math.max(0, borderRadius - 0.6)),
    );

    final glow = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = intense ? 1.35 : 1.0
      ..shader = LinearGradient(
        begin: Alignment.topLeft,
        end: Alignment.centerRight,
        colors: [
          Colors.white.withValues(alpha: isDark ? 0.55 : 0.95),
          Colors.white.withValues(alpha: isDark ? 0.12 : 0.35),
          Colors.white.withValues(alpha: 0),
        ],
        stops: const [0.0, 0.35, 0.75],
      ).createShader(Offset.zero & size);
    canvas.drawRRect(r, glow);

    // Soft inner sheen band.
    final sheen = Paint()
      ..shader = LinearGradient(
        begin: Alignment.topCenter,
        end: const Alignment(0, -0.2),
        colors: [
          Colors.white.withValues(alpha: isDark ? 0.10 : 0.28),
          Colors.white.withValues(alpha: 0),
        ],
      ).createShader(Rect.fromLTWH(0, 0, size.width, size.height * 0.45));
    canvas.drawRRect(r, sheen);
  }

  @override
  bool shouldRepaint(covariant _SpecularRimPainter old) =>
      old.borderRadius != borderRadius ||
      old.isDark != isDark ||
      old.intense != intense;
}

/// Shared sampling region for adjacent glass controls (GlassEffectContainer).
class GlassEffectContainer extends StatelessWidget {
  const GlassEffectContainer({
    super.key,
    required this.child,
    this.padding = const EdgeInsets.all(10),
    this.borderRadius = LiquidGlassTokens.sidebarRadius,
    this.variant = LiquidGlassVariant.regular,
  });

  final Widget child;
  final EdgeInsetsGeometry padding;
  final double borderRadius;
  final LiquidGlassVariant variant;

  @override
  Widget build(BuildContext context) {
    return LiquidGlass(
      variant: variant,
      borderRadius: borderRadius,
      padding: padding,
      child: child,
    );
  }
}

/// Capsule control — glass look without nested BackdropFilter (safe in content).
/// Primary gets a tinted fill; secondary uses luminous plate + hairline.
class GlassCapsuleButton extends StatelessWidget {
  const GlassCapsuleButton({
    super.key,
    required this.label,
    required this.onPressed,
    this.primary = false,
  });

  final String label;
  final VoidCallback? onPressed;
  final bool primary;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final isDark = Theme.of(context).brightness == Brightness.dark;

    if (primary) {
      return FilledButton(
        onPressed: onPressed,
        child: Text(label),
      );
    }

    final fill = isDark
        ? Colors.white.withValues(alpha: 0.08)
        : Colors.white.withValues(alpha: 0.55);
    final rim = isDark
        ? Colors.white.withValues(alpha: 0.20)
        : Colors.white.withValues(alpha: 0.90);

    return DecoratedBox(
      decoration: ShapeDecoration(
        shape: StadiumBorder(side: BorderSide(color: rim, width: 0.8)),
        gradient: LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [
            Color.alphaBlend(Colors.white.withValues(alpha: isDark ? 0.12 : 0.35), fill),
            fill,
          ],
        ),
        shadows: [
          BoxShadow(
            color: Colors.black.withValues(alpha: isDark ? 0.25 : 0.05),
            blurRadius: 10,
            offset: const Offset(0, 3),
          ),
        ],
      ),
      child: TextButton(
        onPressed: onPressed,
        style: TextButton.styleFrom(
          foregroundColor: scheme.onSurface,
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
          shape: const StadiumBorder(),
          minimumSize: const Size(0, 40),
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        ),
        child: Text(label),
      ),
    );
  }
}


/// Selected nav glyph sits on a miniature glass lens (not a flat tint block).
class GlassNavIndicator extends StatelessWidget {
  const GlassNavIndicator({
    super.key,
    required this.selected,
    required this.child,
  });

  final bool selected;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    if (!selected) return child;
    final scheme = Theme.of(context).colorScheme;
    return LiquidGlass(
      borderRadius: 14,
      tint: scheme.primary.withValues(alpha: 0.22),
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
      child: child,
    );
  }
}
