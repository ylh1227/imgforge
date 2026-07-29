import 'package:flutter/material.dart';

import 'liquid_glass.dart';

/// Shared glass-style menu surface for dropdowns and popups.
abstract final class GlassMenuStyle {
  static Color background(BuildContext context) {
    final isDark = Theme.of(context).brightness == Brightness.dark;
    return isDark ? const Color(0xF02C2C2E) : const Color(0xF5FFFFFF);
  }

  static BorderSide hairline(BuildContext context) {
    final isDark = Theme.of(context).brightness == Brightness.dark;
    return BorderSide(
      color: isDark
          ? Colors.white.withValues(alpha: 0.18)
          : Colors.white.withValues(alpha: 0.78),
      width: 0.7,
    );
  }

  static BorderRadius get radius =>
      BorderRadius.circular(LiquidGlassTokens.controlRadius);

  static RoundedRectangleBorder panelShape(BuildContext context) {
    return RoundedRectangleBorder(
      borderRadius: radius,
      side: hairline(context),
    );
  }

  static MenuStyle menuStyle(BuildContext context) {
    return MenuStyle(
      elevation: const WidgetStatePropertyAll(0),
      backgroundColor: WidgetStatePropertyAll(background(context)),
      surfaceTintColor: const WidgetStatePropertyAll(Colors.transparent),
      shadowColor: const WidgetStatePropertyAll(Colors.transparent),
      shape: WidgetStatePropertyAll(panelShape(context)),
      padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(vertical: 6)),
    );
  }
}

String _labelFromDropdownItem<T>(DropdownMenuItem<T> item) {
  final child = item.child;
  if (child is Text) {
    if (child.data != null && child.data!.isNotEmpty) return child.data!;
    final span = child.textSpan;
    if (span != null) return span.toPlainText();
  }
  return item.value?.toString() ?? '';
}

/// Form-style dropdown with Liquid Glass menu corners (Material 3 [DropdownMenu]).
class GlassDropdownButtonFormField<T> extends StatelessWidget {
  const GlassDropdownButtonFormField({
    super.key,
    required this.value,
    required this.items,
    required this.onChanged,
    this.decoration,
    this.isExpanded = false,
    this.width,
  });

  final T? value;
  final List<DropdownMenuItem<T>>? items;
  final ValueChanged<T?>? onChanged;
  final InputDecoration? decoration;
  final bool isExpanded;
  final double? width;

  @override
  Widget build(BuildContext context) {
    final entries = (items ?? [])
        .where((item) => item.value != null)
        .map(
          (item) => DropdownMenuEntry<T>(
            value: item.value as T,
            label: _labelFromDropdownItem(item),
          ),
        )
        .toList();

    return DropdownMenu<T>(
      width: width ?? (isExpanded ? double.infinity : null),
      initialSelection: value,
      enabled: onChanged != null,
      onSelected: onChanged,
      dropdownMenuEntries: entries,
      label: decoration?.labelText != null ? Text(decoration!.labelText!) : null,
      hintText: decoration?.hintText,
      menuStyle: GlassMenuStyle.menuStyle(context),
      menuHeight: 320,
      enableFilter: false,
      enableSearch: false,
      selectOnly: true,
      requestFocusOnTap: false,
      trailingIcon: const Icon(Icons.expand_more_rounded, size: 22),
    );
  }
}

/// [DropdownMenu] with Liquid Glass menu corners.
class GlassDropdownMenu<T> extends StatelessWidget {
  const GlassDropdownMenu({
    super.key,
    this.width,
    this.initialSelection,
    this.label,
    this.onSelected,
    required this.dropdownMenuEntries,
    this.requestFocusOnTap = false,
  });

  final double? width;
  final T? initialSelection;
  final Widget? label;
  final ValueChanged<T?>? onSelected;
  final List<DropdownMenuEntry<T>> dropdownMenuEntries;
  final bool requestFocusOnTap;

  @override
  Widget build(BuildContext context) {
    return DropdownMenu<T>(
      width: width,
      initialSelection: initialSelection,
      label: label,
      onSelected: onSelected,
      dropdownMenuEntries: dropdownMenuEntries,
      requestFocusOnTap: requestFocusOnTap,
      menuStyle: GlassMenuStyle.menuStyle(context),
      menuHeight: 320,
      enableFilter: false,
      enableSearch: false,
      selectOnly: true,
      trailingIcon: const Icon(Icons.expand_more_rounded, size: 22),
    );
  }
}
