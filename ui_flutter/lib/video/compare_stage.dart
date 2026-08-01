import 'package:flutter/material.dart';
import 'package:video_player/video_player.dart';

import 'synced_players.dart';

enum CompareViewMode { grid, solo, wipe }

/// Renders Grid / Solo / Wipe stages from [SyncedPlaybackController].
class CompareStage extends StatelessWidget {
  const CompareStage({
    super.key,
    required this.playback,
    required this.mode,
    required this.soloVideoId,
    required this.wipeSplit,
    required this.onSolo,
    required this.onWipeSplit,
    required this.onExitSolo,
  });

  final SyncedPlaybackController playback;
  final CompareViewMode mode;
  final int? soloVideoId;
  final double wipeSplit;
  final ValueChanged<int> onSolo;
  final ValueChanged<double> onWipeSplit;
  final VoidCallback onExitSolo;

  @override
  Widget build(BuildContext context) {
    final slots = playback.slots;
    if (slots.isEmpty) {
      return const Center(child: Text('勾选或选择视频以播放'));
    }

    switch (mode) {
      case CompareViewMode.solo:
        final id = soloVideoId ?? slots.first.videoId;
        final slot = playback.slotFor(id) ?? slots.first;
        return GestureDetector(
          onDoubleTap: onExitSolo,
          child: _PlayerPane(slot: slot, label: _label(slot), expanded: true),
        );
      case CompareViewMode.wipe:
        return _WipeStage(
          slots: slots.take(2).toList(),
          split: wipeSplit,
          onSplit: onWipeSplit,
        );
      case CompareViewMode.grid:
        return _GridStage(slots: slots, onSolo: onSolo);
    }
  }

  String _label(SyncedPlayerSlot slot) {
    final name = slot.path.split(RegExp(r'[/\\]')).last;
    return '$name  off=${slot.offsetMs}ms';
  }
}

class _GridStage extends StatelessWidget {
  const _GridStage({required this.slots, required this.onSolo});

  final List<SyncedPlayerSlot> slots;
  final ValueChanged<int> onSolo;

  @override
  Widget build(BuildContext context) {
    final n = slots.length;
    final cols = n <= 1
        ? 1
        : n <= 4
            ? 2
            : 3;
    return LayoutBuilder(
      builder: (context, constraints) {
        final rows = (n + cols - 1) ~/ cols;
        return Column(
          children: [
            for (var r = 0; r < rows; r++)
              Expanded(
                child: Row(
                  children: [
                    for (var c = 0; c < cols; c++)
                      Expanded(
                        child: Builder(
                          builder: (context) {
                            final i = r * cols + c;
                            if (i >= n) return const SizedBox.shrink();
                            final slot = slots[i];
                            return Padding(
                              padding: const EdgeInsets.all(3),
                              child: GestureDetector(
                                onDoubleTap: () => onSolo(slot.videoId),
                                child: _PlayerPane(
                                  slot: slot,
                                  label: slot.path.split(RegExp(r'[/\\]')).last,
                                ),
                              ),
                            );
                          },
                        ),
                      ),
                  ],
                ),
              ),
          ],
        );
      },
    );
  }
}

class _WipeStage extends StatelessWidget {
  const _WipeStage({
    required this.slots,
    required this.split,
    required this.onSplit,
  });

  final List<SyncedPlayerSlot> slots;
  final double split;
  final ValueChanged<double> onSplit;

  @override
  Widget build(BuildContext context) {
    if (slots.isEmpty) return const SizedBox.shrink();
    final left = slots.first;
    final right = slots.length > 1 ? slots[1] : slots.first;

    return LayoutBuilder(
      builder: (context, constraints) {
        final w = constraints.maxWidth;
        final x = (split.clamp(0.05, 0.95)) * w;
        return Stack(
          fit: StackFit.expand,
          children: [
            _PlayerPane(slot: right, label: 'B', expanded: true),
            ClipRect(
              clipper: _LeftClipper(x),
              child: _PlayerPane(slot: left, label: 'A', expanded: true),
            ),
            Positioned(
              left: x - 1,
              top: 0,
              bottom: 0,
              child: Container(width: 2, color: Colors.white70),
            ),
            Positioned(
              left: x - 14,
              top: 0,
              bottom: 0,
              width: 28,
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onHorizontalDragUpdate: (d) {
                  onSplit(((x + d.delta.dx) / w).clamp(0.05, 0.95));
                },
                child: Center(
                  child: Container(
                    width: 18,
                    height: 48,
                    decoration: BoxDecoration(
                      color: Colors.black54,
                      borderRadius: BorderRadius.circular(8),
                      border: Border.all(color: Colors.white70),
                    ),
                    child: const Icon(Icons.drag_handle, color: Colors.white, size: 16),
                  ),
                ),
              ),
            ),
            Positioned(
              left: 8,
              bottom: 8,
              child: _chip(context, 'A ${left.path.split(RegExp(r"[/\\]")).last}'),
            ),
            Positioned(
              right: 8,
              bottom: 8,
              child: _chip(context, 'B ${right.path.split(RegExp(r"[/\\]")).last}'),
            ),
          ],
        );
      },
    );
  }

  Widget _chip(BuildContext context, String text) {
    return Material(
      color: Colors.black54,
      borderRadius: BorderRadius.circular(8),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
        child: Text(
          text,
          style: const TextStyle(color: Colors.white, fontSize: 11),
          overflow: TextOverflow.ellipsis,
        ),
      ),
    );
  }
}

class _LeftClipper extends CustomClipper<Rect> {
  _LeftClipper(this.width);
  final double width;

  @override
  Rect getClip(Size size) => Rect.fromLTWH(0, 0, width, size.height);

  @override
  bool shouldReclip(covariant _LeftClipper oldClipper) => oldClipper.width != width;
}

class _PlayerPane extends StatelessWidget {
  const _PlayerPane({
    required this.slot,
    required this.label,
    this.expanded = false,
  });

  final SyncedPlayerSlot slot;
  final String label;
  final bool expanded;

  @override
  Widget build(BuildContext context) {
    final ready = slot.ready;
    final child = !ready
        ? const Center(child: CircularProgressIndicator(strokeWidth: 2))
        : FittedBox(
            fit: BoxFit.contain,
            child: SizedBox(
              width: slot.controller.value.size.width,
              height: slot.controller.value.size.height,
              child: VideoPlayer(slot.controller),
            ),
          );

    return DecoratedBox(
      decoration: BoxDecoration(
        color: Colors.black,
        borderRadius: BorderRadius.circular(expanded ? 12 : 8),
        border: Border.all(color: Colors.white24),
      ),
      child: ClipRRect(
        borderRadius: BorderRadius.circular(expanded ? 12 : 8),
        child: Stack(
          fit: StackFit.expand,
          children: [
            child,
            Positioned(
              left: 6,
              top: 6,
              right: 6,
              child: Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(
                  color: Colors.white,
                  fontSize: 11,
                  shadows: [Shadow(blurRadius: 4, color: Colors.black)],
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Transport controls under the stage.
class TransportBar extends StatelessWidget {
  const TransportBar({
    super.key,
    required this.playback,
    required this.mode,
    required this.onMode,
    required this.onPlayPause,
    required this.onSeek,
    required this.onStep,
    required this.onRate,
    required this.canWipe,
  });

  final SyncedPlaybackController playback;
  final CompareViewMode mode;
  final ValueChanged<CompareViewMode> onMode;
  final VoidCallback onPlayPause;
  final ValueChanged<double> onSeek;
  final ValueChanged<int> onStep;
  final ValueChanged<double> onRate;
  final bool canWipe;

  @override
  Widget build(BuildContext context) {
    final max = playback.maxGlobalMs <= 0 ? 1.0 : playback.maxGlobalMs.toDouble();
    final value = playback.globalPtsMs.toDouble().clamp(0.0, max).toDouble();

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            IconButton(
              tooltip: playback.playing ? '暂停' : '播放',
              onPressed: onPlayPause,
              icon: Icon(playback.playing ? Icons.pause_circle_filled : Icons.play_circle_filled),
              iconSize: 36,
            ),
            IconButton(
              tooltip: '后退一帧',
              onPressed: () => onStep(-40),
              icon: const Icon(Icons.skip_previous),
            ),
            IconButton(
              tooltip: '前进一帧',
              onPressed: () => onStep(40),
              icon: const Icon(Icons.skip_next),
            ),
            const SizedBox(width: 8),
            SegmentedButton<CompareViewMode>(
              segments: [
                const ButtonSegment(value: CompareViewMode.grid, label: Text('宫格')),
                const ButtonSegment(value: CompareViewMode.solo, label: Text('Solo')),
                ButtonSegment(
                  value: CompareViewMode.wipe,
                  label: const Text('Wipe'),
                  enabled: canWipe,
                ),
              ],
              selected: {mode},
              onSelectionChanged: (s) => onMode(s.first),
            ),
            const Spacer(),
            SegmentedButton<double>(
              segments: const [
                ButtonSegment(value: 0.5, label: Text('0.5x')),
                ButtonSegment(value: 1.0, label: Text('1x')),
                ButtonSegment(value: 1.5, label: Text('1.5x')),
              ],
              selected: {playback.rate},
              onSelectionChanged: (s) => onRate(s.first),
            ),
          ],
        ),
        Row(
          children: [
            SizedBox(
              width: 72,
              child: Text(
                _fmt(playback.globalPtsMs),
                style: Theme.of(context).textTheme.labelSmall,
              ),
            ),
            Expanded(
              child: Slider(
                value: value,
                min: 0,
                max: max,
                onChanged: onSeek,
              ),
            ),
            SizedBox(
              width: 72,
              child: Text(
                _fmt(playback.maxGlobalMs),
                textAlign: TextAlign.end,
                style: Theme.of(context).textTheme.labelSmall,
              ),
            ),
          ],
        ),
      ],
    );
  }

  String _fmt(int ms) {
    final s = (ms / 1000).floor();
    final m = s ~/ 60;
    final r = s % 60;
    final frac = ((ms % 1000) / 100).floor();
    return '${m.toString().padLeft(2, '0')}:${r.toString().padLeft(2, '0')}.$frac';
  }
}
