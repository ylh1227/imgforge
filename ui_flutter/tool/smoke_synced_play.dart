/// macOS smoke: attach 2 local files, seek with offsets, play briefly, exit.
///
///   flutter run -d macos -t tool/smoke_synced_play.dart
import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:imgforge_ui/video/synced_players.dart';
import 'package:video_player_media_kit/video_player_media_kit.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  VideoPlayerMediaKit.ensureInitialized(macOS: true, windows: true);

  final root = Directory.current.path.endsWith('ui_flutter')
      ? Directory.current.path
      : '${Directory.current.path}/ui_flutter';
  final a = File('$root/tool/smoke_videos/a.mp4');
  final b = File('$root/tool/smoke_videos/b.mp4');
  if (!a.existsSync() || !b.existsSync()) {
    stderr.writeln('Missing smoke videos under tool/smoke_videos/');
    exit(2);
  }

  final playback = SyncedPlaybackController();
  try {
    await playback.attach([
      {'id': 1, 'file_path': a.path, 'offset_ms': 0, 'duration_ms': 0},
      {'id': 2, 'file_path': b.path, 'offset_ms': 200, 'duration_ms': 0},
    ]);
    if (playback.slots.length != 2) {
      throw StateError('expected 2 slots, got ${playback.slots.length}');
    }
    await playback.seekGlobal(400, force: true);
    final p0 = playback.slots[0].controller.value.position.inMilliseconds;
    final p1 = playback.slots[1].controller.value.position.inMilliseconds;
    if ((p0 - 400).abs() > 200) {
      throw StateError('lane0 seek expected ~400ms, got $p0');
    }
    if ((p1 - 600).abs() > 200) {
      throw StateError('lane1 seek expected ~600ms (offset 200), got $p1');
    }
    await playback.play();
    await Future<void>.delayed(const Duration(milliseconds: 500));
    if (!playback.playing) throw StateError('expected playing');
    await playback.pause();
    print('SMOKE_OK play+2up sync slots=${playback.slots.length} '
        'p0=$p0 p1=$p1 maxGlobal=${playback.maxGlobalMs}');
  } catch (e, st) {
    print('SMOKE_FAIL $e\n$st');
    await playback.disposeAll();
    exit(1);
  }
  await playback.disposeAll();
  await Future<void>.delayed(const Duration(milliseconds: 200));
  exit(0);
}
