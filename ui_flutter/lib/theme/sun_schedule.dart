import 'package:dart_suncalc/suncalc.dart';

/// Solar schedule helpers for day/night theme switching.
class SunSchedule {
  const SunSchedule({
    required this.latitude,
    required this.longitude,
  });

  final double latitude;
  final double longitude;

  /// Whether [moment] falls between today's sunrise and sunset (local time).
  bool isDaytime(DateTime moment) {
    final times = SunCalc.getTimes(moment, lat: latitude, lng: longitude);
    final sunrise = times.sunrise?.toLocal();
    final sunset = times.sunset?.toLocal();
    if (sunrise == null || sunset == null) {
      // Polar edge case: approximate civil day 06:00–18:00 local.
      return moment.hour >= 6 && moment.hour < 18;
    }
    return !moment.isBefore(sunrise) && moment.isBefore(sunset);
  }

  /// Next local time when the day/night theme should flip.
  DateTime? nextTransition(DateTime moment) {
    final times = SunCalc.getTimes(moment, lat: latitude, lng: longitude);
    final sunrise = times.sunrise?.toLocal();
    final sunset = times.sunset?.toLocal();

    if (isDaytime(moment)) {
      return sunset ?? moment.copyWith(hour: 18, minute: 0, second: 0);
    }

    if (sunrise != null && moment.isBefore(sunrise)) {
      return sunrise;
    }

    final tomorrow = SunCalc.getTimes(
      moment.add(const Duration(days: 1)),
      lat: latitude,
      lng: longitude,
    );
    return tomorrow.sunrise?.toLocal() ??
        moment.add(const Duration(days: 1)).copyWith(hour: 6, minute: 0, second: 0);
  }

  ({DateTime? sunrise, DateTime? sunset}) todayTimes(DateTime moment) {
    final times = SunCalc.getTimes(moment, lat: latitude, lng: longitude);
    return (sunrise: times.sunrise?.toLocal(), sunset: times.sunset?.toLocal());
  }
}
