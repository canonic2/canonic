import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:acme_diagnostics/main.dart';

void main() {
  testWidgets('fault, reset and corrected controls expose observed state', (tester) async {
    await tester.pumpWidget(const MaterialApp(home: AcmeDiagnostics()));
    await tester.tap(find.text('retain')); await tester.pump();
    expect(find.text('retain: 1048576'), findsOneWidget);
    await tester.tap(find.text('reset')); await tester.pump();
    expect(find.text('Acme reset'), findsOneWidget);
    await tester.tap(find.byKey(const Key('acme-corrected'))); await tester.pump();
    await tester.tap(find.text('retain')); await tester.pump();
    expect(find.text('retain: 0'), findsOneWidget);
    await tester.tap(find.text('exception')); await tester.pump();
    expect(find.text('exception: ready'), findsOneWidget);
  });
  testWidgets('compile-time corrected selection remains the initial state', (tester) async {
    await tester.pumpWidget(const MaterialApp(home: AcmeDiagnostics(corrected: true)));
    await tester.tap(find.text('retain')); await tester.pump();
    expect(find.text('retain: 0'), findsOneWidget);
  });
}
