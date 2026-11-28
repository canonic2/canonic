// Optional app-owned template. No Flutter dependency, VM settings or registrations.
import 'dart:developer' as developer;

class TestrunDiagnosticProbe {
  TestrunDiagnosticProbe(Map<String, String> metadata, {this.maxEvents = 256})
      : metadata = Map<String, String>.unmodifiable(metadata),
        generation = '${DateTime.now().microsecondsSinceEpoch}-${identityHashCode(Object())}' {
    if (maxEvents < 1 || maxEvents > 4096 || metadata.length > 16 ||
        !metadata.containsKey('appId') || !metadata.containsKey('targetId') ||
        metadata.entries.any((e) => !_text(e.key, 64) || !_text(e.value, 256))) {
      throw ArgumentError('Use bounded app/target/build/runtime metadata');
    }
  }
  final Map<String, String> metadata;
  final String generation;
  final int maxEvents;
  int _sequence = 0;
  bool _active = true;
  static bool _text(String value, int max) => value.isNotEmpty && value.length <= max &&
      !value.runes.any((r) => r < 32 || r == 127);

  // Instrumentation failures never replace application results or exceptions.
  bool mark(String name, {String? actionId, Map<String, Object?> fields = const {}}) {
    return _emit('marker', name, actionId, fields, null);
  }
  Future<T> measure<T>(String name, Future<T> Function() action, {String? actionId}) async {
    final start = developer.Timeline.now;
    try { return await action(); }
    finally { _emit('measure', name, actionId, const {}, developer.Timeline.now - start); }
  }
  bool _emit(String kind, String name, String? actionId, Map<String, Object?> fields, int? duration) {
    if (!_active || _sequence >= maxEvents) return false;
    try {
      if (!_text(name, 128) || (actionId != null && !_text(actionId, 128)) || fields.length > 8) return false;
      final copy = <String, Object?>{};
      for (final e in fields.entries) {
        final v = e.value;
        if (!_text(e.key, 64) || !(v == null || v is bool ||
            (v is num && v.isFinite) || (v is String && v.length <= 128))) return false;
        copy[e.key] = v;
      }
      developer.postEvent('Testrun.Diagnostics', <String, Object?>{
        'schemaVersion': 1, 'kind': kind, 'name': name, 'actionId': actionId,
        'sequence': ++_sequence, 'generation': generation,
        'timestampMicros': developer.Timeline.now, 'durationMicros': duration,
        'metadata': metadata, 'fields': copy, 'maxEvents': maxEvents,
      });
      return true;
    } catch (_) { return false; }
  }
  void dispose() { _active = false; }
}
