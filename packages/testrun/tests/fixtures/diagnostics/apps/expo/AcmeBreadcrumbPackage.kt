package com.example.acme.expodiagnostics

import android.util.Log
import com.facebook.react.ReactPackage
import com.facebook.react.bridge.NativeModule
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.uimanager.ViewManager

/** Fixture-owned bridge: emit a bounded breadcrumb in this app's native PID. */
class AcmeBreadcrumbModule(context: ReactApplicationContext) : ReactContextBaseJavaModule(context) {
  override fun getName() = "AcmeBreadcrumbs"

  @ReactMethod
  fun mark(message: String) {
    require(message.startsWith("Acme ") && message.length <= 160)
    Log.i("AcmeDiagnostics", message)
  }
}

class AcmeBreadcrumbPackage : ReactPackage {
  override fun createNativeModules(context: ReactApplicationContext): List<NativeModule> =
    listOf(AcmeBreadcrumbModule(context))

  override fun createViewManagers(context: ReactApplicationContext): List<ViewManager<*, *>> =
    emptyList()
}
