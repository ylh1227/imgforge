import 'package:flutter/material.dart';

import 'liquid_glass.dart';

class PageChrome extends StatelessWidget {
  const PageChrome({
    super.key,
    required this.title,
    required this.subtitle,
    required this.child,
    this.actions,
  });

  final String title;
  final String subtitle;
  final Widget child;
  final List<Widget>? actions;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // Floating toolbar — regular glass over content (functional layer).
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 14, 16, 0),
          child: LiquidGlass(
            borderRadius: LiquidGlassTokens.toolbarRadius,
            interactive: false,
            padding: const EdgeInsets.fromLTRB(18, 12, 12, 12),
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        title,
                        style: Theme.of(context).textTheme.headlineSmall?.copyWith(
                              fontSize: 20,
                              letterSpacing: -0.5,
                            ),
                      ),
                      const SizedBox(height: 2),
                      Text(
                        subtitle,
                        style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                              color: scheme.onSurfaceVariant.withValues(alpha: 0.9),
                              fontSize: 12,
                            ),
                      ),
                    ],
                  ),
                ),
                if (actions != null) ...[
                  const SizedBox(width: 8),
                  // Keep actions in the same glass sampling region (no nested glass).
                  ...actions!,
                ],
              ],
            ),
          ),
        ),
        Expanded(child: child),
      ],
    );
  }
}
