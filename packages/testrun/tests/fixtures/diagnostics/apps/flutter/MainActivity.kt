package com.example.acme.acme_diagnostics

import android.util.Log
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.FlutterShellArgs
import io.flutter.plugin.common.MethodChannel

/** Fixture-owned channel: emit a bounded breadcrumb in this app's native PID. */
class MainActivity : FlutterActivity() {
  // Launched without `flutter run`, a Debug engine starts with Dart profiling off. The
  // fixture app enables it so CPU samples exist; Testrun itself never changes it.
  override fun getFlutterShellArgs(): FlutterShellArgs =
    FlutterShellArgs((super.getFlutterShellArgs().toArray().toList() + "--enable-dart-profiling").distinct().toTypedArray())

  override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
    super.configureFlutterEngine(flutterEngine)
    MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "com.example.acme/breadcrumbs").setMethodCallHandler { call, result ->
      val message = call.arguments as? String
      if (call.method == "mark" && message != null && message.startsWith("Acme ") && message.length <= 160) {
        Log.i("AcmeDiagnostics", message)
        result.success(true)
      } else {
        result.error("ACME_BREADCRUMB", "Expected a bounded Acme breadcrumb", null)
      }
    }
  }
}
