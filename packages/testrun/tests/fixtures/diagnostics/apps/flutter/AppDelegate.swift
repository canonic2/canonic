import Flutter
import UIKit
import os

/// Fixture-owned channel: emit a bounded breadcrumb in this app's native process log.
@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate {
  private let breadcrumbs = OSLog(subsystem: "com.example.acme.diagnostics", category: "breadcrumbs")

  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    GeneratedPluginRegistrant.register(with: engineBridge.pluginRegistry)
    guard let registrar = engineBridge.pluginRegistry.registrar(forPlugin: "AcmeBreadcrumbs") else { return }
    let channel = FlutterMethodChannel(name: "com.example.acme/breadcrumbs", binaryMessenger: registrar.messenger())
    channel.setMethodCallHandler { [breadcrumbs] call, result in
      guard call.method == "mark", let message = call.arguments as? String,
        message.hasPrefix("Acme "), message.count <= 160
      else {
        result(FlutterError(code: "ACME_BREADCRUMB", message: "Expected a bounded Acme breadcrumb", details: nil))
        return
      }
      os_log("%{public}@", log: breadcrumbs, type: .default, message)
      result(true)
    }
  }
}
