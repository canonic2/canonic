package com.example.acme.diagnostics

object AcmeKotlinFaults {
    fun exception(corrected: Boolean): String {
        if (!corrected) error("Acme Kotlin fixture failure")
        return "ready"
    }
    fun backgroundFailure(corrected: Boolean) {
        if (!corrected) Thread({ error("Acme Kotlin background failure") }, "Acme-Kotlin-fault").start()
    }
    fun mainThreadHang(corrected: Boolean) = AcmeFaults.mainThreadHang(corrected)
    fun killSelf(corrected: Boolean) = AcmeFaults.killSelf(corrected)
    fun reset() = AcmeFaults.reset()
}
