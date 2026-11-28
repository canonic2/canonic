package com.example.acme.diagnostics;

/** Explicit disposable-app calls only. Every destructive action affects this app. */
public final class AcmeFaults {
    private static byte[][] retained;
    public static String exception(boolean corrected) {
        if (!corrected) throw new IllegalStateException("Acme Java fixture failure");
        return "ready";
    }
    public static void mainThreadHang(boolean corrected) {
        if (!corrected) new android.os.Handler(android.os.Looper.getMainLooper()).post(() -> {
            try { Thread.sleep(6000); } catch (InterruptedException e) { Thread.currentThread().interrupt(); }
        });
    }
    public static void backgroundFailure(boolean corrected) {
        if (!corrected) new Thread(() -> { throw new IllegalStateException("Acme Java background failure"); }, "Acme-fault").start();
    }
    public static void killSelf(boolean corrected) { if (!corrected) android.os.Process.killProcess(android.os.Process.myPid()); }
    public static int retain(boolean corrected) { retained = corrected ? null : new byte[16][65536]; return retained == null ? 0 : 1048576; }
    public static void reset() { retained = null; }
    private AcmeFaults() {}
}
