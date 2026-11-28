package io.canonic.testrun.focus;

import android.app.UiAutomation;
import android.accessibilityservice.AccessibilityServiceInfo;
import android.os.HandlerThread;
import android.os.Looper;
import android.os.Process;
import android.view.accessibility.AccessibilityNodeInfo;
import java.io.ByteArrayOutputStream;
import java.nio.charset.StandardCharsets;
import org.json.JSONObject;

/** One bounded automation observation or guarded replacement, with owned cleanup. */
public final class Main {
    static boolean mutationDispatched;
    public static void main(String[] args) {
        // Binder calls may block. Process death also releases the automation connection.
        Thread watchdog = new Thread(new Runnable() {
            @Override public void run() {
                try { Thread.sleep(8000); } catch (InterruptedException ignored) { }
                System.exit(124);
            }
        }, "TestrunFocusDeadline");
        watchdog.setDaemon(true);
        watchdog.start();
        // Android 16 builds the accessibility client's handler on the main looper.
        if (Looper.getMainLooper() == null) Looper.prepareMainLooper();
        HandlerThread callbacks = new HandlerThread("TestrunFocusCallbacks");
        callbacks.start();
        UiAutomation automation = null;
        String result = "{\"version\":1,\"code\":\"AUTOMATION_UNAVAILABLE\"}";
        try {
            if (Process.myUid() != 2000) throw new SecurityException();
            Object connection = Class.forName("android.app.UiAutomationConnection")
                    .getConstructor().newInstance();
            automation = (UiAutomation) UiAutomation.class.getConstructor(
                    Looper.class, Class.forName("android.app.IUiAutomationConnection"))
                    .newInstance(callbacks.getLooper(), connection);
            UiAutomation.class.getMethod("connect", int.class).invoke(automation,
                    UiAutomation.FLAG_DONT_SUPPRESS_ACCESSIBILITY_SERVICES);
            boolean replace = args.length == 1 && "replace".equals(args[0]);
            if (args.length == 1 && ("snapshot".equals(args[0]) || replace)) {
                AccessibilityServiceInfo info = automation.getServiceInfo();
                info.flags |= AccessibilityServiceInfo.FLAG_REPORT_VIEW_IDS
                        | AccessibilityServiceInfo.FLAG_INCLUDE_NOT_IMPORTANT_VIEWS;
                automation.setServiceInfo(info);
            }
            AccessibilityNodeInfo root = automation.getRootInActiveWindow();
            // Connection can complete before the first active-root callback.
            // Retry that read only; never wait for event silence or send input.
            for (int attempt = 0; root == null && attempt < 10; attempt++) {
                Thread.sleep(100);
                root = automation.getRootInActiveWindow();
            }
            result = "{\"version\":1,\"code\":\"ROOT_UNAVAILABLE\"}";
            if (root != null) {
                if (args.length == 1 && "snapshot".equals(args[0])) {
                    result = Tree.snapshot(root, automation);
                } else if (replace) {
                    ByteArrayOutputStream input = new ByteArrayOutputStream();
                    byte[] buffer = new byte[4096];
                    int bytes;
                    while ((bytes = System.in.read(buffer)) != -1) {
                        input.write(buffer, 0, bytes);
                        if (input.size() > 524288) throw new IllegalArgumentException();
                    }
                    result = Tree.replace(root, automation,
                            new JSONObject(new String(input.toByteArray(), StandardCharsets.UTF_8)));
                } else {
                    AccessibilityNodeInfo field = root.findFocus(AccessibilityNodeInfo.FOCUS_INPUT);
                    if (field == null) {
                        result = "{\"version\":1,\"focusedEditable\":false}";
                    } else {
                        result = "{\"version\":1,\"code\":\"FOCUS_STALE\"}";
                        try {
                            if (field.refresh()) {
                                boolean proven = field.isFocused() && field.isEditable()
                                        && field.isEnabled() && field.isVisibleToUser()
                                        && field.getWindowId() == root.getWindowId();
                                result = "{\"version\":1,\"focusedEditable\":" + proven + "}";
                            }
                        } finally { field.recycle(); }
                    }
                }
                root.recycle();
            }
        } catch (Throwable unavailable) {
            // Reflection/platform failures never publish arbitrary exception text or UI data.
            result = mutationDispatched ? "{\"version\":1,\"code\":\"OUTCOME_UNKNOWN\"}"
                    : "{\"version\":1,\"code\":\"AUTOMATION_UNAVAILABLE\"}";
        } finally {
            if (automation != null) {
                try { UiAutomation.class.getMethod("disconnect").invoke(automation); }
                catch (Throwable cleanup) {
                    result = mutationDispatched ? "{\"version\":1,\"code\":\"OUTCOME_UNKNOWN\"}"
                            : "{\"version\":1,\"code\":\"CLEANUP_FAILED\"}";
                }
            }
            callbacks.quitSafely();
        }
        System.out.println(result);
        System.out.flush();
        System.exit(0);
    }
}
