import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:video_player/video_player.dart';

/// One synced video lane.
class SyncedPlayerSlot {
  SyncedPlayerSlot({
    required this.videoId,
    required this.path,
    required this.offsetMs,
    required this.durationMs,
    required this.controller,
  });

  final int videoId;
  final String path;
  final int offsetMs;
  final int durationMs;
  final VideoPlayerController controller;

  bool get ready => controller.value.isInitialized;

  /// Prefer live controller duration when host metadata is missing/wrong.
  int get effectiveDurationMs {
    final live = controller.value.isInitialized
        ? controller.value.duration.inMilliseconds
        : 0;
    if (live > 0) return live;
    return durationMs;
  }

  /// Local media time for a global timeline position.
  int localMsForGlobal(int globalMs) {
    final local = globalMs + offsetMs;
    if (local < 0) return 0;
    final dur = effectiveDurationMs;
    if (dur > 0 && local > dur) return dur;
    return local;
  }

  /// How far global clock can run for this lane.
  int maxGlobalMs() {
    final remain = effectiveDurationMs - offsetMs;
    return remain > 0 ? remain : 0;
  }
}

class SyncedPlaybackController extends ChangeNotifier {
  static const int maxLanes = 6;
  static const int syncToleranceMs = 120;
  static const int syncToleranceManyMs = 220;
  static const int endEpsilonMs = 80;

  final List<SyncedPlayerSlot> slots = [];
  bool playing = false;
  double rate = 1.0;
  int globalPtsMs = 0;
  int maxGlobalMs = 0;

  Timer? _tick;
  bool _seeking = false;
  bool _tickBusy = false;
  bool _disposed = false;
  int _syncCursor = 0;
  int _lastNotifyPts = -1;
  DateTime? _playStartedAt;

  SyncedPlayerSlot? slotFor(int videoId) {
    for (final s in slots) {
      if (s.videoId == videoId) return s;
    }
    return null;
  }

  int get _syncTolerance =>
      slots.length > 3 ? syncToleranceManyMs : syncToleranceMs;

  void _recomputeMaxGlobal() {
    maxGlobalMs = 0;
    final usable = slots
        .map((s) => s.maxGlobalMs())
        .where((ms) => ms >= 200)
        .toList();
    if (usable.isNotEmpty) {
      maxGlobalMs = usable.reduce((a, b) => a < b ? a : b);
    } else if (slots.isNotEmpty) {
      // Fall back to best-effort min even if short/broken metadata.
      maxGlobalMs = slots.map((s) => s.maxGlobalMs()).reduce((a, b) => a < b ? a : b);
    }
  }

  /// Attach players for up to [maxLanes] items.
  /// Each map needs: id, file_path, offset_ms, duration_ms.
  Future<void> attach(List<Map<String, dynamic>> videos) async {
    await disposeAll();
    if (_disposed) return;

    final take = videos.take(maxLanes).toList();
    for (final v in take) {
      final id = (v['id'] as num?)?.toInt();
      final path = v['file_path']?.toString();
      if (id == null || path == null || path.isEmpty) continue;
      final file = File(path);
      if (!file.existsSync()) continue;

      final offset = (v['offset_ms'] as num?)?.toInt() ?? 0;
      final duration = (v['duration_ms'] as num?)?.toInt() ?? 0;
      final controller = VideoPlayerController.file(
        file,
        videoPlayerOptions: VideoPlayerOptions(mixWithOthers: true),
      );
      try {
        await controller.initialize();
        await controller.setLooping(false);
        await controller.setVolume(slots.isEmpty ? 1.0 : 0.0); // only first has audio
        await controller.pause();
        final liveDur = controller.value.duration.inMilliseconds;
        final durMs = liveDur > 0 ? liveDur : duration;
        slots.add(
          SyncedPlayerSlot(
            videoId: id,
            path: path,
            offsetMs: offset,
            durationMs: durMs,
            controller: controller,
          ),
        );
      } catch (e) {
        debugPrint('SyncedPlayback attach failed $path: $e');
        await controller.dispose();
      }
    }

    _recomputeMaxGlobal();
    globalPtsMs = globalPtsMs.clamp(0, maxGlobalMs > 0 ? maxGlobalMs : 0);
    await seekGlobal(globalPtsMs, force: true);
    notifyListeners();
  }

  Future<void> updateOffsets(Map<int, int> offsetsById) async {
    for (var i = 0; i < slots.length; i++) {
      final s = slots[i];
      final next = offsetsById[s.videoId];
      if (next == null || next == s.offsetMs) continue;
      slots[i] = SyncedPlayerSlot(
        videoId: s.videoId,
        path: s.path,
        offsetMs: next,
        durationMs: s.durationMs,
        controller: s.controller,
      );
    }
    _recomputeMaxGlobal();
    await seekGlobal(globalPtsMs, force: true);
    notifyListeners();
  }

  Future<void> play() async {
    if (slots.isEmpty) return;
    _recomputeMaxGlobal();
    // Restart from beginning if already at / past the shared end.
    if (maxGlobalMs > 0 && globalPtsMs >= maxGlobalMs - endEpsilonMs) {
      await seekGlobal(0, force: true);
    }
    playing = true;
    _playStartedAt = DateTime.now();
    for (final s in slots) {
      if (!s.ready) continue;
      try {
        await s.controller.setPlaybackSpeed(rate);
        await s.controller.play();
      } catch (e) {
        debugPrint('SyncedPlayback play failed ${s.path}: $e');
      }
    }
    _startTick();
    notifyListeners();
  }

  Future<void> pause() async {
    playing = false;
    _playStartedAt = null;
    _tick?.cancel();
    _tick = null;
    for (final s in slots) {
      if (s.ready) {
        try {
          await s.controller.pause();
        } catch (_) {}
      }
    }
    notifyListeners();
  }

  Future<void> toggle() async {
    if (playing) {
      await pause();
    } else {
      await play();
    }
  }

  Future<void> setRate(double next) async {
    rate = next.clamp(0.25, 2.0);
    for (final s in slots) {
      if (s.ready) await s.controller.setPlaybackSpeed(rate);
    }
    notifyListeners();
  }

  Future<void> seekGlobal(int ms, {bool force = false}) async {
    if (_seeking && !force) return;
    _seeking = true;
    globalPtsMs = ms.clamp(0, maxGlobalMs > 0 ? maxGlobalMs : 0);
    try {
      for (final s in slots) {
        if (!s.ready) continue;
        final local = s.localMsForGlobal(globalPtsMs);
        try {
          await s.controller.seekTo(Duration(milliseconds: local));
        } catch (e) {
          debugPrint('SyncedPlayback seek failed ${s.path}: $e');
        }
      }
    } finally {
      _seeking = false;
    }
    _lastNotifyPts = globalPtsMs;
    notifyListeners();
  }

  Future<void> stepMs(int delta) async {
    await pause();
    await seekGlobal(globalPtsMs + delta, force: true);
  }

  void _startTick() {
    _tick?.cancel();
    _tick = Timer.periodic(const Duration(milliseconds: 100), (_) {
      unawaited(_onTick());
    });
  }

  Future<void> _onTick() async {
    if (!playing || slots.isEmpty || _seeking || _tickBusy || _disposed) return;
    _tickBusy = true;
    try {
      final lead = slots.first;
      if (!lead.ready) return;

      final leadLocal = lead.controller.value.position.inMilliseconds;
      var derivedGlobal = leadLocal - lead.offsetMs;
      if (derivedGlobal < 0) derivedGlobal = 0;

      final started = _playStartedAt;
      final playAgeMs = started == null
          ? 0
          : DateTime.now().difference(started).inMilliseconds;

      // End-of-timeline: ignore the first ~300ms so a stale position / short
      // maxGlobal from a bad lane cannot pause immediately after play().
      final atEnd = maxGlobalMs > 0 && derivedGlobal >= maxGlobalMs - endEpsilonMs;
      final leadEnded = !lead.controller.value.isPlaying &&
          lead.effectiveDurationMs > 0 &&
          leadLocal >= lead.effectiveDurationMs - endEpsilonMs;
      if (playAgeMs > 300 && atEnd && (leadEnded || derivedGlobal >= maxGlobalMs)) {
        globalPtsMs = maxGlobalMs;
        await pause();
        return;
      }

      globalPtsMs = maxGlobalMs > 0
          ? derivedGlobal.clamp(0, maxGlobalMs)
          : derivedGlobal;

      // Correct at most one lagging follower per tick to avoid seek storms
      // when 4–6 mpv/media_kit instances are open.
      if (slots.length > 1) {
        final tol = _syncTolerance;
        final n = slots.length - 1;
        for (var k = 0; k < n; k++) {
          _syncCursor = (_syncCursor % n) + 1;
          final s = slots[_syncCursor];
          if (!s.ready) continue;
          final want = s.localMsForGlobal(globalPtsMs);
          final have = s.controller.value.position.inMilliseconds;
          if ((have - want).abs() > tol) {
            try {
              await s.controller.seekTo(Duration(milliseconds: want));
              if (playing && !s.controller.value.isPlaying) {
                await s.controller.play();
              }
            } catch (e) {
              debugPrint('SyncedPlayback sync seek failed ${s.path}: $e');
            }
            break; // one correction per tick
          }
        }
      }

      // Keep lead playing if the platform paused it under decoder pressure.
      if (playing && lead.ready && !lead.controller.value.isPlaying && !atEnd) {
        try {
          await lead.controller.play();
        } catch (_) {}
      }

      if ((globalPtsMs - _lastNotifyPts).abs() >= 40) {
        _lastNotifyPts = globalPtsMs;
        notifyListeners();
      }
    } finally {
      _tickBusy = false;
    }
  }

  Future<void> disposeAll() async {
    _tick?.cancel();
    _tick = null;
    playing = false;
    _playStartedAt = null;
    final old = List<SyncedPlayerSlot>.from(slots);
    slots.clear();
    maxGlobalMs = 0;
    for (final s in old) {
      try {
        await s.controller.dispose();
      } catch (_) {}
    }
  }

  @override
  void dispose() {
    _disposed = true;
    unawaited(disposeAll());
    super.dispose();
  }
}
