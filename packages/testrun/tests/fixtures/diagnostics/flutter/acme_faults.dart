import 'dart:typed_data';

class AcmeFaults {
  AcmeFaults({this.corrected = false});
  final bool corrected;
  List<Uint8List> retained = [];
  int generation = 1;
  bool disposed = false;
  void check() { if (disposed) throw StateError('Acme fixture disposed'); }
  String exception() { check(); if (!corrected) throw StateError('Acme Dart fixture failure'); return 'ready'; }
  Future<String> bridge() async { check(); if (!corrected) throw StateError('Acme simulated plugin failure'); return 'ready'; }
  Future<int> network() async { check(); if (!corrected) throw StateError('Acme simulated network failure'); return 200; }
  int expensive() { check(); var sum = 0; for (var n = 0; n < (corrected ? 100 : 100000); n++) { sum = (sum + n) & 0xffffffff; } return sum; }
  int retain() { check(); retained = List.generate(16, (_) => Uint8List(65536)); if (corrected) retained = []; return retainedBytes; }
  int get retainedBytes => retained.fold(0, (sum, bytes) => sum + bytes.length);
  int churn() { check(); return corrected ? 1 : 12; }
  void reset() { check(); retained = []; generation++; }
  void dispose() { retained = []; disposed = true; }
}
