import 'dart:math' as math;

import 'package:flutter/material.dart';

/// Linked pan/zoom for multi-image compare.
///
/// Model follows OpenSeadragon / MIRL Collate / Potato annotator:
/// - [zoom] is relative to each pane's fit-scale (1 = contain)
/// - [focusNorm] is the image-normalized point (0–1) kept at the viewport center
///
/// Syncing image-space focus (not viewport-fraction center) keeps the same
/// region aligned across panes when zooming.
class SharedViewportController extends ChangeNotifier {
  double zoom = 1.0;
  Offset focusNorm = const Offset(0.5, 0.5);

  /// When false, panes navigate independently (solo mode).
  bool syncEnabled = true;

  /// Temporary unlock while Alt is held (MicroDicom pattern).
  bool syncSuspended = false;

  bool get isLinked => syncEnabled && !syncSuspended;

  void reset() {
    zoom = 1.0;
    focusNorm = const Offset(0.5, 0.5);
    notifyListeners();
  }

  void setSyncEnabled(bool value) {
    if (syncEnabled == value) return;
    syncEnabled = value;
    notifyListeners();
  }

  void setSyncSuspended(bool value) {
    if (syncSuspended == value) return;
    syncSuspended = value;
    notifyListeners();
  }

  void setView({required double zoom, required Offset focusNorm}) {
    final z = zoom.clamp(0.05, 64.0);
    final f = Offset(focusNorm.dx.clamp(0.0, 1.0), focusNorm.dy.clamp(0.0, 1.0));
    if ((this.zoom - z).abs() < 1e-6 && (this.focusNorm - f).distance < 1e-6) {
      return;
    }
    this.zoom = z;
    this.focusNorm = f;
    notifyListeners();
  }

  static double fitScaleOf(Size viewport, Size natural) {
    if (viewport.isEmpty || natural.isEmpty) return 1;
    return math.min(
      viewport.width / natural.width,
      viewport.height / natural.height,
    );
  }

  double scaleFor(Size viewport, Size natural) {
    return fitScaleOf(viewport, natural) * zoom;
  }

  /// Image draw rect in viewport coordinates.
  Rect imageRect(Size viewport, Size natural) {
    final s = scaleFor(viewport, natural);
    final w = natural.width * s;
    final h = natural.height * s;
    final left = viewport.width * 0.5 - focusNorm.dx * w;
    final top = viewport.height * 0.5 - focusNorm.dy * h;
    return Rect.fromLTWH(left, top, w, h);
  }

  Offset viewToScene(Offset viewPt, Size viewport, Size natural) {
    final r = imageRect(viewport, natural);
    if (r.width <= 0 || r.height <= 0 || natural.isEmpty) return Offset.zero;
    return Offset(
      (viewPt.dx - r.left) / r.width * natural.width,
      (viewPt.dy - r.top) / r.height * natural.height,
    );
  }

  Offset sceneToNorm(Offset scene, Size natural) {
    if (natural.isEmpty) return const Offset(0.5, 0.5);
    return Offset(
      (scene.dx / natural.width).clamp(0.0, 1.0),
      (scene.dy / natural.height).clamp(0.0, 1.0),
    );
  }

  void fit() {
    zoom = 1.0;
    focusNorm = const Offset(0.5, 0.5);
    notifyListeners();
  }

  void setOneToOne({required Size viewport, required Size natural}) {
    final fit = fitScaleOf(viewport, natural);
    if (fit <= 0) return;
    setView(zoom: 1 / fit, focusNorm: const Offset(0.5, 0.5));
  }

  /// Zoom about a viewport focal point; [scenePoint] in image pixels.
  void zoomAbout({
    required Size viewport,
    required Size natural,
    required double newScale,
    required Offset scenePoint,
    required Offset focalInView,
  }) {
    final fit = fitScaleOf(viewport, natural);
    if (fit <= 0 || natural.isEmpty) return;
    final s = newScale.clamp(0.02, 64.0);
    final newZoom = (s / fit).clamp(0.05, 64.0);
    final w = natural.width * s;
    final h = natural.height * s;
    // After zoom, scenePoint stays under focalInView → derive new focusNorm
    // such that imageRect places that scene point at focal.
    // left + (scene.x/nat.w)*w = focal.x
    // center = viewport/2 - focus*w  => focus = (viewport/2 - left) / w
    final left = focalInView.dx - (scenePoint.dx / natural.width) * w;
    final top = focalInView.dy - (scenePoint.dy / natural.height) * h;
    final focus = Offset(
      w <= 0 ? 0.5 : ((viewport.width * 0.5 - left) / w).clamp(0.0, 1.0),
      h <= 0 ? 0.5 : ((viewport.height * 0.5 - top) / h).clamp(0.0, 1.0),
    );
    setView(zoom: newZoom, focusNorm: focus);
  }

  /// Pan by viewport delta (pixels).
  void panByViewDelta(Offset delta, Size viewport, Size natural) {
    final r = imageRect(viewport, natural);
    if (r.width <= 0 || r.height <= 0) return;
    setView(
      zoom: zoom,
      focusNorm: Offset(
        (focusNorm.dx - delta.dx / r.width).clamp(0.0, 1.0),
        (focusNorm.dy - delta.dy / r.height).clamp(0.0, 1.0),
      ),
    );
  }

  int get zoomPercent => (zoom * 100).round();
}
