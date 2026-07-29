import 'package:flutter_test/flutter_test.dart';
import 'package:imgforge_ui/theme/sun_schedule.dart';

void main() {
  test('Shanghai midsummer is daytime at noon', () {
    const sun = SunSchedule(latitude: 31.2304, longitude: 121.4737);
    final noon = DateTime(2026, 7, 15, 12, 0);
    expect(sun.isDaytime(noon), isTrue);
  });

  test('Shanghai midsummer is night at midnight', () {
    const sun = SunSchedule(latitude: 31.2304, longitude: 121.4737);
    final midnight = DateTime(2026, 7, 15, 0, 30);
    expect(sun.isDaytime(midnight), isFalse);
  });

  test('nextTransition after sunset points to next sunrise', () {
    const sun = SunSchedule(latitude: 31.2304, longitude: 121.4737);
    final evening = DateTime(2026, 7, 15, 21, 0);
    final next = sun.nextTransition(evening);
    expect(next, isNotNull);
    expect(next!.isAfter(evening), isTrue);
    expect(next.hour, lessThan(12));
  });
}
