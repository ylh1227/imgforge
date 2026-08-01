import 'package:flutter_test/flutter_test.dart';
import 'package:imgforge_ui/video/synced_players.dart';

void main() {
  test('SyncedPlayerSlot local/global offset math', () {
    // Mirror SyncedPlayerSlot helpers without a real VideoPlayerController.
    int localMsForGlobal(int globalMs, int offsetMs, int durationMs) {
      final local = globalMs + offsetMs;
      if (local < 0) return 0;
      if (durationMs > 0 && local > durationMs) return durationMs;
      return local;
    }

    int maxGlobalMs(int durationMs, int offsetMs) {
      final remain = durationMs - offsetMs;
      return remain > 0 ? remain : 0;
    }

    expect(localMsForGlobal(0, 500, 5000), 500);
    expect(localMsForGlobal(1000, 500, 5000), 1500);
    expect(localMsForGlobal(0, -100, 5000), 0);
    expect(localMsForGlobal(4800, 500, 5000), 5000);
    expect(maxGlobalMs(5000, 500), 4500);
    expect(SyncedPlaybackController.maxLanes, 6);
    expect(SyncedPlaybackController.syncToleranceMs, 120);
  });
}
