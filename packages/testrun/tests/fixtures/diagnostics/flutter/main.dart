// Mount in an explicitly owned Flutter app; not a binary or SDK installer.
import 'dart:async';
import 'dart:developer' as developer;
import 'dart:ui' show PlatformDispatcher;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'acme_faults.dart';

// Present only in apps that register the fixture's native channel.
const _breadcrumbs = MethodChannel('com.example.acme/breadcrumbs');
Future<bool> breadcrumb(String message) async {
  try { return await _breadcrumbs.invokeMethod<bool>('mark', message) ?? false; }
  on MissingPluginException { return false; }
  on PlatformException { return false; }
}

void main() {
  // App-owned report of otherwise unhandled zone errors; returning false keeps them unhandled.
  PlatformDispatcher.instance.onError = (error, stack) {
    // The VM reports the error object as a reference; keep its text in the message too.
    developer.log('Acme unhandled error: $error', name: 'acme', level: 1000, error: error, stackTrace: stack);
    return false;
  };
  runApp(const MaterialApp(home: AcmeDiagnostics(corrected: bool.fromEnvironment('ACME_CORRECTED'), uncaught: bool.fromEnvironment('ACME_UNCAUGHT'))));
}

int acmeProfileSink = 0;
/// Bounded named CPU work for profiler stacks: about 60 ms per call. Returns the
/// completed 1000-step rounds, so equal-time bursts compare throughput.
int acmeProfileWork() {
  final watch = Stopwatch()..start();
  var sum = 0, rounds = 0;
  while (watch.elapsedMilliseconds < 60) { for (var n = 0; n < 1000; n++) { sum = (sum + n * n) & 0xffffffff; } rounds++; }
  acmeProfileSink ^= sum;
  return rounds;
}

class AcmeDiagnostics extends StatefulWidget {
  const AcmeDiagnostics({super.key, this.corrected = false, this.uncaught = false});
  final bool corrected;
  final bool uncaught;
  @override State<AcmeDiagnostics> createState() => _AcmeDiagnosticsState();
}
class _AcmeDiagnosticsState extends State<AcmeDiagnostics> {
  late bool corrected = widget.corrected;
  late bool uncaught = widget.uncaught;
  late AcmeFaults faults = AcmeFaults(corrected: corrected);
  String result = 'Acme ready';
  String bridge = 'Acme native bridge pending';
  int frames = 0;
  String profileRounds = 'rounds: none';
  Timer? timer;
  Timer? work;
  @override void initState() {
    super.initState();
    breadcrumb('Acme native bridge requested').then((ok) { if (mounted) setState(() => bridge = ok ? 'Acme native bridge available' : 'Acme native bridge unavailable'); });
  }
  Future<void> run(String name, FutureOr<Object?> Function() action) async {
    final owner = faults; final generation = owner.generation;
    try { final value = await action(); if (mounted && !owner.disposed && generation == owner.generation) setState(() => result = '$name: $value'); }
    catch (error) {
      developer.log('Acme caught exception: $error', name: 'acme', level: 900, error: error);
      if (mounted && !owner.disposed && generation == owner.generation) setState(() => result = '$name: $error');
    }
  }
  void select({bool? fixed, bool? unhandled}) {
    timer?.cancel(); work?.cancel(); faults.dispose();
    setState(() { corrected = fixed ?? corrected; uncaught = unhandled ?? uncaught; faults = AcmeFaults(corrected: corrected); frames = 0; result = 'Acme ready'; });
  }
  @override void didUpdateWidget(covariant AcmeDiagnostics oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.corrected != widget.corrected || oldWidget.uncaught != widget.uncaught) select(fixed: widget.corrected, unhandled: widget.uncaught);
  }
  void exception() {
    if (!uncaught) { run('exception', faults.exception); return; }
    developer.log('Acme uncaught exception requested', name: 'acme', level: 800);
    breadcrumb('Acme uncaught exception requested');
    // Thrown outside any handler, so it reaches the zone's unhandled-error route.
    unawaited(Future<void>(faults.exception));
  }
  void retain() { breadcrumb('Acme trigger retain corrected=$corrected'); run('retain', faults.retain); }
  void profile() {
    work?.cancel(); var bursts = 0, rounds = 0;
    setState(() => profileRounds = 'rounds: pending');
    work = Timer.periodic(const Duration(milliseconds: 100), (tick) { if (!mounted || ++bursts > 8) { tick.cancel(); if (mounted) setState(() { result = 'profile: done'; profileRounds = 'rounds: $rounds'; }); return; } rounds += acmeProfileWork(); });
  }
  void churn() { timer?.cancel(); setState(() => frames = faults.churn()); timer = Timer.periodic(const Duration(milliseconds: 16), (tick) { if (!mounted || frames == 0) { tick.cancel(); return; } setState(() => frames--); }); }
  @override void dispose() { timer?.cancel(); work?.cancel(); faults.dispose(); super.dispose(); }
  Widget slot(String id, String text) => Semantics(container: true, identifier: id, child: Text(text, key: Key(id)));
  Widget toggle(String id, String label, bool value, ValueChanged<bool> changed) => Row(children: [
    Semantics(container: true, identifier: id, child: Switch(key: Key(id), value: value, onChanged: changed)), Text(label)]);
  @override Widget build(BuildContext context) => Scaffold(appBar: AppBar(title: const Text('Acme diagnostics')), body: ListView(children: [
    slot('acme-result', result), slot('acme-frame', '$frames'), slot('acme-native-bridge', bridge), slot('acme-profile-rounds', profileRounds),
    toggle('acme-corrected', 'corrected', corrected, (value) => select(fixed: value)),
    toggle('acme-uncaught', 'uncaught', uncaught, (value) => select(unhandled: value)),
    ElevatedButton(onPressed: exception, child: const Text('exception')),
    for (final action in <String, FutureOr<Object?> Function()>{'bridge': faults.bridge, 'network': faults.network, 'expensive': faults.expensive}.entries)
      ElevatedButton(onPressed: () => run(action.key, action.value), child: Text(action.key)),
    ElevatedButton(onPressed: retain, child: const Text('retain')),
    ElevatedButton(onPressed: profile, child: const Text('Profile workload')),
    ElevatedButton(onPressed: churn, child: const Text('frame churn')),
    ElevatedButton(onPressed: () { timer?.cancel(); work?.cancel(); faults.reset(); setState(() { frames = 0; result = 'Acme reset'; }); }, child: const Text('reset')),
  ]));
}
