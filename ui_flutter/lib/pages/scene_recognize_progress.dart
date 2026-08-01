import 'package:flutter/material.dart';

import '../widgets/liquid_glass.dart';

/// Live progress for scene recognize + match jobs.
class SceneRecognizeProgressController extends ChangeNotifier {
  int current = 0;
  int total = 0;
  String message = '准备中…';
  bool finished = false;
  bool cancelling = false;
  String? resultSummary;

  double? get fraction {
    if (total <= 0) return null;
    return (current / total).clamp(0.0, 1.0);
  }

  void update({
    required int current,
    required int total,
    required String message,
  }) {
    this.current = current;
    this.total = total;
    this.message = message;
    notifyListeners();
  }

  void markCancelling() {
    cancelling = true;
    message = '正在取消…（当前项完成后停止）';
    notifyListeners();
  }

  void complete(String summary) {
    finished = true;
    cancelling = false;
    resultSummary = summary;
    if (total > 0) current = total;
    message = summary;
    notifyListeners();
  }
}

/// Modal progress dialog. Caller owns [controller] lifetime until dialog closes.
Future<void> showSceneRecognizeProgressDialog({
  required BuildContext context,
  required SceneRecognizeProgressController controller,
  required Future<void> Function() onCancel,
  String title = '场景识别与匹配',
}) {
  return showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (ctx) => _SceneRecognizeProgressDialog(
      controller: controller,
      onCancel: onCancel,
      title: title,
    ),
  );
}

class _SceneRecognizeProgressDialog extends StatelessWidget {
  const _SceneRecognizeProgressDialog({
    required this.controller,
    required this.onCancel,
    required this.title,
  });

  final SceneRecognizeProgressController controller;
  final Future<void> Function() onCancel;
  final String title;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Dialog(
      insetPadding: const EdgeInsets.symmetric(horizontal: 36, vertical: 24),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 480, minWidth: 360),
        child: LiquidGlass(
          borderRadius: LiquidGlassTokens.toolbarRadius,
          padding: const EdgeInsets.fromLTRB(22, 20, 22, 18),
          child: ListenableBuilder(
            listenable: controller,
            builder: (context, _) {
              final done = controller.finished;
              final pct = controller.fraction;
              return Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Row(
                    children: [
                      Icon(
                        done ? Icons.check_circle_outline : Icons.auto_awesome,
                        color: done ? scheme.primary : scheme.onSurface,
                      ),
                      const SizedBox(width: 10),
                      Expanded(
                        child: Text(
                          title,
                          style: Theme.of(context).textTheme.titleMedium,
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 18),
                  ClipRRect(
                    borderRadius: BorderRadius.circular(6),
                    child: LinearProgressIndicator(
                      minHeight: 10,
                      value: done ? 1 : pct,
                    ),
                  ),
                  const SizedBox(height: 12),
                  Text(
                    controller.total > 0
                        ? '${controller.current} / ${controller.total}'
                        : '处理中…',
                    style: Theme.of(context).textTheme.headlineSmall?.copyWith(
                          fontSize: 22,
                          fontWeight: FontWeight.w700,
                          letterSpacing: -0.4,
                        ),
                  ),
                  const SizedBox(height: 8),
                  Text(
                    controller.message,
                    style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                          color: scheme.onSurfaceVariant,
                          height: 1.35,
                        ),
                  ),
                  const SizedBox(height: 20),
                  Row(
                    children: [
                      if (!done)
                        Text(
                          controller.cancelling ? '取消中…' : '识别与匹配进行中',
                          style: Theme.of(context).textTheme.labelSmall,
                        ),
                      const Spacer(),
                      if (!done)
                        OutlinedButton(
                          onPressed: controller.cancelling
                              ? null
                              : () async {
                                  controller.markCancelling();
                                  await onCancel();
                                },
                          child: const Text('取消'),
                        )
                      else
                        FilledButton(
                          onPressed: () => Navigator.of(context).pop(),
                          child: const Text('完成'),
                        ),
                    ],
                  ),
                ],
              );
            },
          ),
        ),
      ),
    );
  }
}
