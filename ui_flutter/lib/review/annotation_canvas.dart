import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'annotation_models.dart';
import 'shared_viewport.dart';

typedef AnnotationCreate = Future<void> Function(ReviewAnnotation draft);
typedef AnnotationUpdate = Future<void> Function(ReviewAnnotation ann);
typedef AnnotationDelete = Future<void> Function(int id);

/// Image + annotation overlay. Zoom/pan via [sharedViewport] (fit-relative).
/// Paints in viewport space so compare panes never cull away when zooming out.
class AnnotationCanvas extends StatefulWidget {
  const AnnotationCanvas({
    super.key,
    required this.imagePath,
    required this.imageId,
    required this.annotations,
    required this.tool,
    required this.style,
    required this.sharedViewport,
    this.selectedId,
    this.onSelect,
    this.onCreate,
    this.onUpdate,
    this.onDelete,
    this.readOnly = false,
    this.syncLeader = false,
  });

  final String imagePath;
  final int imageId;
  final List<ReviewAnnotation> annotations;
  final CanvasTool tool;
  final AnnStyle style;
  final SharedViewportController sharedViewport;
  final int? selectedId;
  final ValueChanged<int?>? onSelect;
  final AnnotationCreate? onCreate;
  final AnnotationUpdate? onUpdate;
  final AnnotationDelete? onDelete;
  final bool readOnly;
  final bool syncLeader;

  @override
  State<AnnotationCanvas> createState() => AnnotationCanvasState();
}

class AnnotationCanvasState extends State<AnnotationCanvas> {
  ui.Image? _decoded;
  Size? _natural;
  Size? _viewport;

  Offset? _dragStartScene;
  Offset? _dragCurrentScene;
  int? _draggingId;
  Offset? _dragOriginNorm;
  Map<String, dynamic>? _dragOriginPos;

  bool _seeded = false;
  bool _viewGesturing = false;
  double _gestureBaseScale = 1;
  Offset _gestureSceneFocal = Offset.zero;

  /// Independent view when sync is off / Alt-held (DICOM "solo" pattern).
  final _localView = SharedViewportController();

  SharedViewportController get _view =>
      widget.sharedViewport.isLinked ? widget.sharedViewport : _localView;

  @override
  void initState() {
    super.initState();
    widget.sharedViewport.addListener(_onShared);
    _localView.addListener(_onShared);
    HardwareKeyboard.instance.addHandler(_onKey);
    _loadImage();
  }

  @override
  void didUpdateWidget(covariant AnnotationCanvas oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.sharedViewport != widget.sharedViewport) {
      oldWidget.sharedViewport.removeListener(_onShared);
      widget.sharedViewport.addListener(_onShared);
    }
    if (oldWidget.imagePath != widget.imagePath) {
      _decoded?.dispose();
      _decoded = null;
      _natural = null;
      _seeded = false;
      _loadImage();
    }
  }

  @override
  void dispose() {
    HardwareKeyboard.instance.removeHandler(_onKey);
    widget.sharedViewport.removeListener(_onShared);
    _localView.removeListener(_onShared);
    _localView.dispose();
    _decoded?.dispose();
    super.dispose();
  }

  bool _onKey(KeyEvent event) {
    final alt = HardwareKeyboard.instance.isAltPressed;
    if (widget.sharedViewport.syncSuspended != alt) {
      widget.sharedViewport.setSyncSuspended(alt);
    }
    return false;
  }

  void _onShared() {
    if (mounted) setState(() {});
  }

  Future<void> _loadImage() async {
    final file = File(widget.imagePath);
    if (!file.existsSync()) {
      setState(() {
        _decoded = null;
        _natural = null;
      });
      return;
    }
    final bytes = await file.readAsBytes();
    final codec = await ui.instantiateImageCodec(bytes);
    final frame = await codec.getNextFrame();
    if (!mounted) {
      frame.image.dispose();
      return;
    }
    _decoded?.dispose();
    setState(() {
      _decoded = frame.image;
      _natural = Size(
        frame.image.width.toDouble(),
        frame.image.height.toDouble(),
      );
    });
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (widget.syncLeader) fitToView();
    });
  }

  void fitToView() {
    final box = context.findRenderObject() as RenderBox?;
    final natural = _natural;
    if (box == null || !box.hasSize || natural == null || natural.isEmpty) {
      return;
    }
    _viewport = box.size;
    if (widget.sharedViewport.isLinked) {
      widget.sharedViewport.fit();
    } else {
      _localView.fit();
    }
    _seeded = true;
  }

  void setZoom100() {
    final view = _viewport;
    final natural = _natural;
    if (view == null || natural == null) return;
    _view.setOneToOne(viewport: view, natural: natural);
  }

  Offset? _viewToScene(Offset viewPt) {
    final view = _viewport;
    final natural = _natural;
    if (view == null || natural == null) return null;
    return _view.viewToScene(viewPt, view, natural);
  }

  Offset? _toNorm(Offset scene) {
    final natural = _natural;
    if (natural == null || natural.isEmpty) return null;
    return Offset(
      (scene.dx / natural.width).clamp(0.0, 1.0),
      (scene.dy / natural.height).clamp(0.0, 1.0),
    );
  }

  bool get _allowViewNav =>
      widget.readOnly ||
      (widget.tool == CanvasTool.select && _draggingId == null);

  Future<void> _finishCreate() async {
    final a = _dragStartScene;
    final b = _dragCurrentScene;
    _dragStartScene = null;
    _dragCurrentScene = null;
    if (a == null || b == null || widget.onCreate == null) {
      setState(() {});
      return;
    }
    final na = _toNorm(a);
    final nb = _toNorm(b);
    if (na == null || nb == null) return;

    ReviewAnnotation? draft;
    switch (widget.tool) {
      case CanvasTool.rectangle:
        if ((na - nb).distance < 0.008) return;
        draft = ReviewAnnotation(
          id: 0,
          imageItemId: widget.imageId,
          kind: AnnotationKind.rectangle,
          position: {
            'kind': 'rectangle',
            'x0': na.dx,
            'y0': na.dy,
            'x1': nb.dx,
            'y1': nb.dy,
          },
          style: widget.style,
          content: '',
        );
      case CanvasTool.arrow:
        if ((na - nb).distance < 0.008) return;
        draft = ReviewAnnotation(
          id: 0,
          imageItemId: widget.imageId,
          kind: AnnotationKind.arrow,
          position: {
            'kind': 'arrow',
            'x0': na.dx,
            'y0': na.dy,
            'x1': nb.dx,
            'y1': nb.dy,
          },
          style: widget.style,
          content: '',
        );
      case CanvasTool.text:
      case CanvasTool.select:
        break;
    }
    if (draft != null) await widget.onCreate!(draft);
    if (mounted) setState(() {});
  }

  Future<void> _placeText(Offset scene) async {
    final n = _toNorm(scene);
    if (n == null || widget.onCreate == null) return;
    final controller = TextEditingController();
    final text = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('文字标注'),
        content: TextField(
          controller: controller,
          autofocus: true,
          decoration: const InputDecoration(hintText: '输入文字'),
          onSubmitted: (v) => Navigator.pop(ctx, v),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('取消')),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, controller.text),
            child: const Text('确定'),
          ),
        ],
      ),
    );
    if (text == null || text.trim().isEmpty) return;
    await widget.onCreate!(
      ReviewAnnotation(
        id: 0,
        imageItemId: widget.imageId,
        kind: AnnotationKind.text,
        position: {'kind': 'text', 'x': n.dx, 'y': n.dy},
        style: widget.style,
        content: text.trim(),
      ),
    );
  }

  void _onPointerDown(Offset viewPt) {
    if (widget.readOnly) return;
    final scene = _viewToScene(viewPt);
    if (scene == null) return;
    final norm = _toNorm(scene);
    if (norm == null) return;

    if (widget.tool == CanvasTool.select) {
      ReviewAnnotation? hit;
      for (final ann in widget.annotations.reversed) {
        if (ann.hitTest(norm)) {
          hit = ann;
          break;
        }
      }
      widget.onSelect?.call(hit?.id);
      if (hit != null && !hit.locked) {
        _draggingId = hit.id;
        _dragOriginNorm = norm;
        _dragOriginPos = Map<String, dynamic>.from(hit.position);
      }
      return;
    }

    if (widget.tool == CanvasTool.text) {
      _placeText(scene);
      return;
    }

    _dragStartScene = scene;
    _dragCurrentScene = scene;
    setState(() {});
  }

  void _onPointerMove(Offset viewPt) {
    if (widget.readOnly) return;
    final scene = _viewToScene(viewPt);
    if (scene == null) return;

    if (_draggingId != null && _dragOriginNorm != null && _dragOriginPos != null) {
      final norm = _toNorm(scene);
      if (norm == null) return;
      final d = norm - _dragOriginNorm!;
      ReviewAnnotation? ann;
      for (final a in widget.annotations) {
        if (a.id == _draggingId) {
          ann = a;
          break;
        }
      }
      if (ann == null) return;
      final origin = _dragOriginPos!;
      switch (ann.kind) {
        case AnnotationKind.rectangle:
        case AnnotationKind.arrow:
          ann.position = {
            ...origin,
            'x0': ((origin['x0'] as num).toDouble() + d.dx).clamp(0.0, 1.0),
            'y0': ((origin['y0'] as num).toDouble() + d.dy).clamp(0.0, 1.0),
            'x1': ((origin['x1'] as num).toDouble() + d.dx).clamp(0.0, 1.0),
            'y1': ((origin['y1'] as num).toDouble() + d.dy).clamp(0.0, 1.0),
          };
        case AnnotationKind.text:
          ann.position = {
            ...origin,
            'x': ((origin['x'] as num).toDouble() + d.dx).clamp(0.0, 1.0),
            'y': ((origin['y'] as num).toDouble() + d.dy).clamp(0.0, 1.0),
          };
      }
      setState(() {});
      return;
    }
    if (_dragStartScene != null) {
      setState(() => _dragCurrentScene = scene);
    }
  }

  Future<void> _onPointerUp() async {
    if (_draggingId != null) {
      final id = _draggingId!;
      _draggingId = null;
      _dragOriginNorm = null;
      _dragOriginPos = null;
      ReviewAnnotation? ann;
      for (final a in widget.annotations) {
        if (a.id == id) {
          ann = a;
          break;
        }
      }
      if (ann != null && widget.onUpdate != null) {
        await widget.onUpdate!(ann);
      }
      return;
    }
    await _finishCreate();
  }

  void _onScaleStart(ScaleStartDetails details) {
    final view = _viewport;
    final natural = _natural;
    if (view == null || natural == null) return;
    if (!_allowViewNav && details.pointerCount < 2) return;
    _viewGesturing = true;
    _gestureBaseScale = _view.scaleFor(view, natural);
    _gestureSceneFocal = _view.viewToScene(details.localFocalPoint, view, natural);
  }

  void _onScaleUpdate(ScaleUpdateDetails details) {
    if (!_viewGesturing) return;
    final view = _viewport;
    final natural = _natural;
    if (view == null || natural == null) return;

    if (details.pointerCount >= 2 || details.scale != 1.0) {
      _view.zoomAbout(
        viewport: view,
        natural: natural,
        newScale: _gestureBaseScale * details.scale,
        scenePoint: _gestureSceneFocal,
        focalInView: details.localFocalPoint,
      );
      return;
    }

    if (_allowViewNav && _draggingId == null) {
      _view.panByViewDelta(details.focalPointDelta, view, natural);
    }
  }

  void _onScaleEnd(ScaleEndDetails details) {
    _viewGesturing = false;
  }

  void _onScrollZoom(PointerScrollEvent e) {
    final view = _viewport;
    final natural = _natural;
    if (view == null || natural == null) return;
    final factor = e.scrollDelta.dy > 0 ? 0.9 : 1.1;
    final sceneFocal = _view.viewToScene(e.localPosition, view, natural);
    final base = _view.scaleFor(view, natural);
    _view.zoomAbout(
      viewport: view,
      natural: natural,
      newScale: base * factor,
      scenePoint: sceneFocal,
      focalInView: e.localPosition,
    );
  }

  @override
  Widget build(BuildContext context) {
    final natural = _natural;

    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.delete): () {
          final id = widget.selectedId;
          if (id != null) widget.onDelete?.call(id);
        },
        const SingleActivator(LogicalKeyboardKey.backspace): () {
          final id = widget.selectedId;
          if (id != null) widget.onDelete?.call(id);
        },
      },
      child: Focus(
        autofocus: widget.syncLeader,
        child: LayoutBuilder(
          builder: (context, constraints) {
            final view = Size(constraints.maxWidth, constraints.maxHeight);
            _viewport = view;
            if (widget.syncLeader &&
                !_seeded &&
                natural != null &&
                !natural.isEmpty &&
                view.width > 0) {
              WidgetsBinding.instance.addPostFrameCallback((_) {
                if (!mounted || _seeded) return;
                fitToView();
              });
            }

            return Listener(
              onPointerSignal: (ev) {
                if (ev is PointerScrollEvent) _onScrollZoom(ev);
              },
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onScaleStart: _onScaleStart,
                onScaleUpdate: _onScaleUpdate,
                onScaleEnd: _onScaleEnd,
                onDoubleTap: fitToView,
                child: Listener(
                  behavior: HitTestBehavior.opaque,
                  onPointerDown: (e) {
                    if (e.kind != PointerDeviceKind.mouse &&
                        e.kind != PointerDeviceKind.touch &&
                        e.kind != PointerDeviceKind.trackpad) {
                      return;
                    }
                    _onPointerDown(e.localPosition);
                  },
                  onPointerMove: (e) {
                    if (_draggingId != null || _dragStartScene != null) {
                      _onPointerMove(e.localPosition);
                    }
                  },
                  onPointerUp: (_) => _onPointerUp(),
                  onPointerCancel: (_) {
                    _draggingId = null;
                    _dragStartScene = null;
                    _dragCurrentScene = null;
                    setState(() {});
                  },
                  // Clip so zoomed image cannot paint into neighboring compare panes.
                  child: ClipRect(
                    child: CustomPaint(
                      size: view,
                      isComplex: true,
                      willChange: true,
                      painter: _ViewportPainter(
                        image: _decoded,
                        natural: natural,
                        imageRect: natural == null
                            ? Rect.zero
                            : _view.imageRect(view, natural),
                        annotations: widget.annotations,
                        selectedId: widget.selectedId,
                        draftStart: _dragStartScene,
                        draftEnd: _dragCurrentScene,
                        draftTool: widget.tool,
                        draftStyle: widget.style,
                      ),
                    ),
                  ),
                ),
              ),
            );
          },
        ),
      ),
    );
  }
}

class _ViewportPainter extends CustomPainter {
  _ViewportPainter({
    required this.image,
    required this.natural,
    required this.imageRect,
    required this.annotations,
    required this.selectedId,
    required this.draftStart,
    required this.draftEnd,
    required this.draftTool,
    required this.draftStyle,
  });

  final ui.Image? image;
  final Size? natural;
  final Rect imageRect;
  final List<ReviewAnnotation> annotations;
  final int? selectedId;
  final Offset? draftStart;
  final Offset? draftEnd;
  final CanvasTool draftTool;
  final AnnStyle draftStyle;

  Offset _sceneToView(Offset scene) {
    final nat = natural;
    if (nat == null || nat.isEmpty || imageRect.width <= 0) return Offset.zero;
    return Offset(
      imageRect.left + scene.dx / nat.width * imageRect.width,
      imageRect.top + scene.dy / nat.height * imageRect.height,
    );
  }

  @override
  void paint(Canvas canvas, Size size) {
    canvas.drawRect(Offset.zero & size, Paint()..color = const Color(0xFF2C2C2E));

    if (image != null && natural != null && imageRect.width > 0) {
      paintImage(
        canvas: canvas,
        rect: imageRect,
        image: image!,
        fit: BoxFit.fill,
        filterQuality: FilterQuality.medium,
      );
    }

    for (final ann in annotations) {
      _paintAnn(canvas, ann, ann.id == selectedId);
    }

    if (draftStart != null && draftEnd != null) {
      final paint = Paint()
        ..color = draftStyle.color
        ..strokeWidth = draftStyle.lineWidth
        ..style = PaintingStyle.stroke;
      final a = _sceneToView(draftStart!);
      final b = _sceneToView(draftEnd!);
      if (draftTool == CanvasTool.rectangle) {
        canvas.drawRect(Rect.fromPoints(a, b), paint);
      } else if (draftTool == CanvasTool.arrow) {
        _drawArrow(canvas, a, b, paint);
      }
    }
  }

  void _paintAnn(Canvas canvas, ReviewAnnotation ann, bool selected) {
    final paint = Paint()
      ..color = ann.style.color
      ..strokeWidth = ann.style.lineWidth + (selected ? 1.5 : 0)
      ..style = PaintingStyle.stroke;
    final fill = Paint()
      ..color = ann.style.color.withValues(alpha: selected ? 0.18 : 0.08)
      ..style = PaintingStyle.fill;

    switch (ann.kind) {
      case AnnotationKind.rectangle:
        final r = ann.rectNorm;
        if (r == null) return;
        final rect = Rect.fromLTRB(
          imageRect.left + r.left * imageRect.width,
          imageRect.top + r.top * imageRect.height,
          imageRect.left + r.right * imageRect.width,
          imageRect.top + r.bottom * imageRect.height,
        );
        canvas.drawRect(rect, fill);
        canvas.drawRect(rect, paint);
      case AnnotationKind.arrow:
        final a = ann.arrowNorm;
        if (a == null) return;
        final p0 = Offset(
          imageRect.left + a.$1.dx * imageRect.width,
          imageRect.top + a.$1.dy * imageRect.height,
        );
        final p1 = Offset(
          imageRect.left + a.$2.dx * imageRect.width,
          imageRect.top + a.$2.dy * imageRect.height,
        );
        _drawArrow(canvas, p0, p1, paint);
      case AnnotationKind.text:
        final t = ann.textNorm;
        if (t == null) return;
        final pos = Offset(
          imageRect.left + t.dx * imageRect.width,
          imageRect.top + t.dy * imageRect.height,
        );
        final tp = TextPainter(
          text: TextSpan(
            text: ann.content.isEmpty ? '文字' : ann.content,
            style: TextStyle(
              color: ann.style.color,
              fontSize: 16 + ann.style.lineWidth,
              fontWeight: FontWeight.w600,
              shadows: const [
                Shadow(color: Color(0x99000000), blurRadius: 3),
              ],
            ),
          ),
          textDirection: TextDirection.ltr,
        )..layout(maxWidth: imageRect.width * 0.5);
        if (selected) {
          canvas.drawRRect(
            RRect.fromRectAndRadius(
              Rect.fromLTWH(pos.dx - 4, pos.dy - 2, tp.width + 8, tp.height + 4),
              const Radius.circular(4),
            ),
            Paint()..color = ann.style.color.withValues(alpha: 0.2),
          );
        }
        tp.paint(canvas, pos);
    }
  }

  void _drawArrow(Canvas canvas, Offset a, Offset b, Paint paint) {
    canvas.drawLine(a, b, paint);
    final ang = math.atan2(b.dy - a.dy, b.dx - a.dx);
    const len = 14.0;
    const wing = 0.45;
    final p1 = Offset(
      b.dx - len * math.cos(ang - wing),
      b.dy - len * math.sin(ang - wing),
    );
    final p2 = Offset(
      b.dx - len * math.cos(ang + wing),
      b.dy - len * math.sin(ang + wing),
    );
    final path = Path()
      ..moveTo(b.dx, b.dy)
      ..lineTo(p1.dx, p1.dy)
      ..lineTo(p2.dx, p2.dy)
      ..close();
    canvas.drawPath(path, Paint()..color = paint.color);
  }

  @override
  bool shouldRepaint(covariant _ViewportPainter old) => true;
}
