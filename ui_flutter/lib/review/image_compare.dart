import 'dart:io';

import 'package:flutter/material.dart';

import 'annotation_canvas.dart';
import 'annotation_models.dart';
import 'shared_viewport.dart';

enum ImageCompareMode { single, sideBySide, grid }

typedef ComparePane = ({int id, String path, String label});

/// Multi-image compare stage (annotate on primary; linked zoom/pan by default).
class ImageCompareStage extends StatefulWidget {
  const ImageCompareStage({
    super.key,
    required this.mode,
    required this.primary,
    required this.others,
    required this.annotations,
    required this.tool,
    required this.style,
    required this.selectedAnnId,
    required this.canvasKey,
    required this.sharedViewport,
    this.onSelectAnn,
    this.onCreate,
    this.onUpdate,
    this.onDelete,
  });

  final ImageCompareMode mode;
  final ComparePane? primary;
  final List<ComparePane> others;
  final List<ReviewAnnotation> annotations;
  final CanvasTool tool;
  final AnnStyle style;
  final int? selectedAnnId;
  final GlobalKey<AnnotationCanvasState> canvasKey;
  final SharedViewportController sharedViewport;
  final ValueChanged<int?>? onSelectAnn;
  final AnnotationCreate? onCreate;
  final AnnotationUpdate? onUpdate;
  final AnnotationDelete? onDelete;

  @override
  State<ImageCompareStage> createState() => _ImageCompareStageState();
}

class _ImageCompareStageState extends State<ImageCompareStage> {
  int? _activePaneId;

  List<ComparePane> get _panes {
    if (widget.primary == null) return const [];
    final limit = widget.mode == ImageCompareMode.grid ? 6 : 4;
    return [widget.primary!, ...widget.others].take(limit).toList();
  }

  @override
  void didUpdateWidget(covariant ImageCompareStage oldWidget) {
    super.didUpdateWidget(oldWidget);
    final primaryId = widget.primary?.id;
    if (primaryId != null &&
        (_activePaneId == null ||
            !_panes.any((p) => p.id == _activePaneId))) {
      _activePaneId = primaryId;
    }
  }

  Widget _pane(ComparePane p, {required bool editable}) {
    if (!File(p.path).existsSync()) {
      return Center(
        child: Text('缺失\n${p.label}', textAlign: TextAlign.center),
      );
    }
    final active = (_activePaneId ?? widget.primary?.id) == p.id;
    final scheme = Theme.of(context).colorScheme;

    return GestureDetector(
      onTap: () => setState(() => _activePaneId = p.id),
      child: DecoratedBox(
        decoration: BoxDecoration(
          border: Border.all(
            color: active
                ? scheme.primary.withValues(alpha: 0.85)
                : scheme.outlineVariant.withValues(alpha: 0.35),
            width: active ? 2 : 1,
          ),
        ),
        child: ClipRect(
          child: AnnotationCanvas(
            key: editable ? widget.canvasKey : ValueKey('pane-${p.id}'),
            imagePath: p.path,
            imageId: p.id,
            annotations: editable ? widget.annotations : const [],
            tool: editable ? widget.tool : CanvasTool.select,
            style: widget.style,
            sharedViewport: widget.sharedViewport,
            selectedId: editable ? widget.selectedAnnId : null,
            onSelect: editable ? widget.onSelectAnn : null,
            onCreate: editable ? widget.onCreate : null,
            onUpdate: editable ? widget.onUpdate : null,
            onDelete: editable ? widget.onDelete : null,
            readOnly: !editable,
            syncLeader: editable,
          ),
        ),
      ),
    );
  }

  Widget _labeled(ComparePane p, {required bool editable}) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(8, 6, 8, 4),
          child: Text(
            p.label,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: Theme.of(context).textTheme.labelSmall?.copyWith(
                  fontWeight: (_activePaneId ?? widget.primary?.id) == p.id
                      ? FontWeight.w700
                      : FontWeight.w500,
                ),
          ),
        ),
        Expanded(child: _pane(p, editable: editable)),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final panes = _panes;
    if (panes.isEmpty) {
      return const Center(child: Text('选择图片'));
    }

    if (widget.mode == ImageCompareMode.single || panes.length == 1) {
      return _pane(panes.first, editable: true);
    }

    if (widget.mode == ImageCompareMode.sideBySide) {
      return Row(
        children: [
          for (var i = 0; i < panes.length; i++) ...[
            if (i > 0) const VerticalDivider(width: 1),
            Expanded(child: _labeled(panes[i], editable: i == 0)),
          ],
        ],
      );
    }

    final cols = panes.length <= 2 ? panes.length : (panes.length <= 4 ? 2 : 3);
    final rows = (panes.length + cols - 1) ~/ cols;
    return Column(
      children: [
        for (var r = 0; r < rows; r++) ...[
          if (r > 0) const Divider(height: 1),
          Expanded(
            child: Row(
              children: [
                for (var c = 0; c < cols; c++) ...[
                  if (c > 0) const VerticalDivider(width: 1),
                  Expanded(
                    child: Builder(
                      builder: (context) {
                        final idx = r * cols + c;
                        if (idx >= panes.length) return const SizedBox.shrink();
                        return _labeled(panes[idx], editable: idx == 0);
                      },
                    ),
                  ),
                ],
              ],
            ),
          ),
        ],
      ],
    );
  }
}

String fileLabel(String path) {
  final parts = path.split(Platform.pathSeparator);
  return parts.isEmpty ? path : parts.last;
}
