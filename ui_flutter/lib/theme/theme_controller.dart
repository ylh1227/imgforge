import 'dart:async';

import 'package:flutter/material.dart';
import 'package:geolocator/geolocator.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'sun_schedule.dart';

/// How the app picks light vs dark appearance.
enum ThemeSchedule {
  /// Follow macOS / system appearance.
  system('跟随系统', Icons.brightness_auto_outlined),

  /// Light between sunrise and sunset at the user's location.
  sunCycle('跟随日出日落', Icons.wb_twilight_outlined),

  /// Always light theme.
  light('始终浅色', Icons.light_mode_outlined),

  /// Always dark theme.
  dark('始终深色', Icons.dark_mode_outlined);

  const ThemeSchedule(this.label, this.icon);

  final String label;
  final IconData icon;
}

/// Drives [ThemeMode] from system preference or local solar times.
class ThemeController extends ChangeNotifier {
  ThemeController();

  static const _prefSchedule = 'theme_schedule';
  static const _prefLat = 'theme_latitude';
  static const _prefLng = 'theme_longitude';

  /// Default when location is unavailable (Shanghai).
  static const defaultLatitude = 31.2304;
  static const defaultLongitude = 121.4737;

  ThemeSchedule _schedule = ThemeSchedule.sunCycle;
  double _latitude = defaultLatitude;
  double _longitude = defaultLongitude;
  bool _sunCycleDark = false;
  bool _locationReady = false;

  Timer? _transitionTimer;
  Timer? _watchdogTimer;

  ThemeSchedule get schedule => _schedule;
  double get latitude => _latitude;
  double get longitude => _longitude;
  bool get locationReady => _locationReady;

  SunSchedule get _sun => SunSchedule(latitude: _latitude, longitude: _longitude);

  /// Effective Material theme mode for [MaterialApp.themeMode].
  ThemeMode get themeMode {
    return switch (_schedule) {
      ThemeSchedule.system => ThemeMode.system,
      ThemeSchedule.light => ThemeMode.light,
      ThemeSchedule.dark => ThemeMode.dark,
      ThemeSchedule.sunCycle => _sunCycleDark ? ThemeMode.dark : ThemeMode.light,
    };
  }

  String _formatTime(DateTime? t) =>
      t == null ? '--:--' : '${t.hour.toString().padLeft(2, '0')}:${t.minute.toString().padLeft(2, '0')}';

  String get statusLine {
    if (_schedule != ThemeSchedule.sunCycle) {
      return _schedule.label;
    }
    final times = _sun.todayTimes(DateTime.now());
    final phase = _sunCycleDark ? '夜间' : '日间';
    final loc = _locationReady ? '已定位' : '默认位置';
    return '$phase · 日出 ${_formatTime(times.sunrise)} · 日落 ${_formatTime(times.sunset)} · $loc';
  }

  Future<void> init() async {
    final prefs = await SharedPreferences.getInstance();
    final index = prefs.getInt(_prefSchedule);
    if (index != null && index >= 0 && index < ThemeSchedule.values.length) {
      _schedule = ThemeSchedule.values[index];
    }
    _latitude = prefs.getDouble(_prefLat) ?? defaultLatitude;
    _longitude = prefs.getDouble(_prefLng) ?? defaultLongitude;
    _applySunCycle(notify: false);
    notifyListeners();

    unawaited(_refreshLocation());
    _startWatchdog();
  }

  Future<void> setSchedule(ThemeSchedule value) async {
    if (_schedule == value) return;
    _schedule = value;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setInt(_prefSchedule, value.index);
    _applySunCycle();
    _armTransitionTimer();
  }

  Future<void> cycleSchedule() async {
    final next = ThemeSchedule.values[(_schedule.index + 1) % ThemeSchedule.values.length];
    await setSchedule(next);
  }

  Future<void> _refreshLocation() async {
    try {
      var permission = await Geolocator.checkPermission();
      if (permission == LocationPermission.denied) {
        permission = await Geolocator.requestPermission();
      }
      if (permission == LocationPermission.denied ||
          permission == LocationPermission.deniedForever) {
        return;
      }

      Position? position = await Geolocator.getLastKnownPosition();
      position ??= await Geolocator.getCurrentPosition(
        locationSettings: const LocationSettings(
          accuracy: LocationAccuracy.low,
          timeLimit: Duration(seconds: 8),
        ),
      );

      _latitude = position.latitude;
      _longitude = position.longitude;
      _locationReady = true;

      final prefs = await SharedPreferences.getInstance();
      await prefs.setDouble(_prefLat, _latitude);
      await prefs.setDouble(_prefLng, _longitude);

      _applySunCycle();
      _armTransitionTimer();
    } catch (e) {
      debugPrint('Theme location unavailable: $e');
    }
  }

  void _applySunCycle({bool notify = true}) {
    if (_schedule == ThemeSchedule.sunCycle) {
      _sunCycleDark = !_sun.isDaytime(DateTime.now());
    }
    if (notify) notifyListeners();
  }

  void _armTransitionTimer() {
    _transitionTimer?.cancel();
    if (_schedule != ThemeSchedule.sunCycle) return;

    final next = _sun.nextTransition(DateTime.now());
    if (next == null) return;

    var delay = next.difference(DateTime.now());
    if (delay.isNegative) delay = const Duration(seconds: 1);
    if (delay.inDays > 1) delay = const Duration(hours: 1);

    _transitionTimer = Timer(delay, () {
      _applySunCycle();
      _armTransitionTimer();
    });
  }

  void _startWatchdog() {
    _watchdogTimer?.cancel();
    _watchdogTimer = Timer.periodic(const Duration(minutes: 1), (_) {
      if (_schedule != ThemeSchedule.sunCycle) return;
      final shouldBeDark = !_sun.isDaytime(DateTime.now());
      if (shouldBeDark != _sunCycleDark) {
        _sunCycleDark = shouldBeDark;
        notifyListeners();
      }
      _armTransitionTimer();
    });
    _armTransitionTimer();
  }

  @override
  void dispose() {
    _transitionTimer?.cancel();
    _watchdogTimer?.cancel();
    super.dispose();
  }
}
