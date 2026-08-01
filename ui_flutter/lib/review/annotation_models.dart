import 'dart:ui';

enum AnnotationKind { rectangle, arrow, text }

enum CanvasTool { select, rectangle, arrow, text }

class AnnStyle {
  const AnnStyle({
    this.color = const Color(0xFFFF3B30),
    this.lineWidth = 2,
  });

  final Color color;
  final double lineWidth;

  Map<String, dynamic> toJson() => {
        'color': [
          (color.r * 255.0).round().clamp(0, 255),
          (color.g * 255.0).round().clamp(0, 255),
          (color.b * 255.0).round().clamp(0, 255),
          (color.a * 255.0).round().clamp(0, 255),
        ],
        'line_width': lineWidth,
      };

  static AnnStyle fromJson(Map<String, dynamic>? m) {
    if (m == null) return const AnnStyle();
    final c = m['color'];
    Color color = const Color(0xFFFF3B30);
    if (c is List && c.length >= 3) {
      color = Color.fromARGB(
        c.length > 3 ? (c[3] as num).toInt() : 255,
        (c[0] as num).toInt(),
        (c[1] as num).toInt(),
        (c[2] as num).toInt(),
      );
    }
    return AnnStyle(
      color: color,
      lineWidth: (m['line_width'] as num?)?.toDouble() ?? 2,
    );
  }
}

class ReviewAnnotation {
  ReviewAnnotation({
    required this.id,
    required this.imageItemId,
    required this.kind,
    required this.position,
    required this.style,
    required this.content,
    this.locked = false,
    this.zIndex = 0,
  });

  final int id;
  final int imageItemId;
  final AnnotationKind kind;
  /// Normalized geometry keys depend on [kind].
  Map<String, dynamic> position;
  AnnStyle style;
  String content;
  final bool locked;
  final int zIndex;

  static AnnotationKind parseKind(dynamic v) {
    final s = v?.toString() ?? '';
    switch (s) {
      case 'Arrow':
      case 'arrow':
      case '1':
        return AnnotationKind.arrow;
      case 'Text':
      case 'text':
      case '2':
        return AnnotationKind.text;
      default:
        return AnnotationKind.rectangle;
    }
  }

  static String kindWire(AnnotationKind k) {
    switch (k) {
      case AnnotationKind.rectangle:
        return 'Rectangle';
      case AnnotationKind.arrow:
        return 'Arrow';
      case AnnotationKind.text:
        return 'Text';
    }
  }

  static ReviewAnnotation fromJson(Map<String, dynamic> m) {
    final pos = Map<String, dynamic>.from(m['position'] as Map? ?? {});
    return ReviewAnnotation(
      id: (m['id'] as num?)?.toInt() ?? 0,
      imageItemId: (m['image_item_id'] as num?)?.toInt() ?? 0,
      kind: parseKind(m['kind']),
      position: pos,
      style: AnnStyle.fromJson(
        m['style'] is Map ? Map<String, dynamic>.from(m['style'] as Map) : null,
      ),
      content: m['content']?.toString() ?? '',
      locked: m['locked'] == true,
      zIndex: (m['z_index'] as num?)?.toInt() ?? 0,
    );
  }

  Map<String, dynamic> toCreateJson() => {
        'id': 0,
        'image_item_id': imageItemId,
        'kind': kindWire(kind),
        'position': position,
        'style': style.toJson(),
        'content': content,
        'created_at': DateTime.now().toUtc().toIso8601String(),
        'locked': locked,
        'z_index': zIndex,
      };

  Rect? get rectNorm {
    if (kind != AnnotationKind.rectangle) return null;
    final x0 = (position['x0'] as num?)?.toDouble() ?? 0;
    final y0 = (position['y0'] as num?)?.toDouble() ?? 0;
    final x1 = (position['x1'] as num?)?.toDouble() ?? 0;
    final y1 = (position['y1'] as num?)?.toDouble() ?? 0;
    return Rect.fromLTRB(
      x0 < x1 ? x0 : x1,
      y0 < y1 ? y0 : y1,
      x0 < x1 ? x1 : x0,
      y0 < y1 ? y1 : y0,
    );
  }

  Offset? get textNorm {
    if (kind != AnnotationKind.text) return null;
    return Offset(
      (position['x'] as num?)?.toDouble() ?? 0,
      (position['y'] as num?)?.toDouble() ?? 0,
    );
  }

  (Offset, Offset)? get arrowNorm {
    if (kind != AnnotationKind.arrow) return null;
    return (
      Offset(
        (position['x0'] as num?)?.toDouble() ?? 0,
        (position['y0'] as num?)?.toDouble() ?? 0,
      ),
      Offset(
        (position['x1'] as num?)?.toDouble() ?? 0,
        (position['y1'] as num?)?.toDouble() ?? 0,
      ),
    );
  }

  bool hitTest(Offset norm, {double slop = 0.02}) {
    switch (kind) {
      case AnnotationKind.rectangle:
        final r = rectNorm;
        if (r == null) return false;
        return r.inflate(slop).contains(norm);
      case AnnotationKind.arrow:
        final a = arrowNorm;
        if (a == null) return false;
        return _distToSegment(norm, a.$1, a.$2) < slop;
      case AnnotationKind.text:
        final t = textNorm;
        if (t == null) return false;
        return (norm - t).distance < slop * 2;
    }
  }

  static double _distToSegment(Offset p, Offset a, Offset b) {
    final ab = b - a;
    final len2 = ab.dx * ab.dx + ab.dy * ab.dy;
    if (len2 < 1e-9) return (p - a).distance;
    var t = ((p.dx - a.dx) * ab.dx + (p.dy - a.dy) * ab.dy) / len2;
    t = t.clamp(0.0, 1.0);
    final proj = Offset(a.dx + ab.dx * t, a.dy + ab.dy * t);
    return (p - proj).distance;
  }
}
