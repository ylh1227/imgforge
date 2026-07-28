import 'package:flutter_test/flutter_test.dart';
import 'package:imgforge_ui/app.dart';

void main() {
  testWidgets('shell builds', (tester) async {
    await tester.pumpWidget(const ImgForgeApp());
    expect(find.text('ImgForge'), findsOneWidget);
  });
}
