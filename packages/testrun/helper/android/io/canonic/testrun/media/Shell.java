package io.canonic.testrun.media;

import android.content.Context;
import android.content.ContextWrapper;
import android.content.AttributionSource;
import android.content.pm.ApplicationInfo;
import android.os.Build;
import android.os.Looper;
import java.lang.reflect.*;

/** The shell process has no Application; initialize framework service context explicitly. */
final class Shell extends ContextWrapper {
    private Shell(Context base) { super(base); }
    @Override public String getPackageName() { return "com.android.shell"; }
    @Override public String getOpPackageName() { return getPackageName(); }
    @Override public Context getApplicationContext() { return this; }
    @Override public AttributionSource getAttributionSource() {
        return new AttributionSource.Builder(2000).setPackageName(getPackageName()).build();
    }
    static Context create() throws Exception {
        if (Build.VERSION.SDK_INT < 31) throw new UnsupportedOperationException("Android 12 or later required");
        if (Looper.getMainLooper() == null) Looper.prepareMainLooper();
        Class<?> at = Class.forName("android.app.ActivityThread");
        Constructor<?> ctor = at.getDeclaredConstructor(); ctor.setAccessible(true);
        Object thread = ctor.newInstance();
        set(at, null, "sCurrentActivityThread", thread);
        set(at, thread, "mSystemThread", true);
        Class<?> bind = Class.forName("android.app.ActivityThread$AppBindData");
        ctor = bind.getDeclaredConstructor(); ctor.setAccessible(true);
        Object data = ctor.newInstance();
        ApplicationInfo info = new ApplicationInfo(); info.packageName = "com.android.shell";
        set(bind, data, "appInfo", info); set(at, thread, "mBoundApplication", data);
        Class<?> cc = Class.forName("android.app.ConfigurationController");
        ctor = cc.getDeclaredConstructor(Class.forName("android.app.ActivityThreadInternal"));
        ctor.setAccessible(true); set(at, thread, "mConfigurationController", ctor.newInstance(thread));
        Class<?> msm = Class.forName("android.media.MediaServiceManager");
        ctor = msm.getDeclaredConstructor(); ctor.setAccessible(true);
        Object manager = ctor.newInstance();
        // Different releases register through different initializers; at least one must work.
        boolean registered = false;
        for (String name : new String[]{"android.media.MediaFrameworkPlatformInitializer", "android.media.MediaFrameworkInitializer"}) {
            try {
                Method method = Class.forName(name).getDeclaredMethod("setMediaServiceManager", msm);
                method.setAccessible(true); method.invoke(null, manager); registered = true;
            } catch (ReflectiveOperationException unavailable) { /* Try the other platform initializer. */ }
        }
        if (!registered) throw new UnsupportedOperationException("Media service initializer unavailable");
        Method system = at.getDeclaredMethod("getSystemContext"); system.setAccessible(true);
        return new Shell((Context) system.invoke(thread));
    }
    private static void set(Class<?> owner, Object instance, String name, Object value) throws Exception {
        Field field = owner.getDeclaredField(name); field.setAccessible(true); field.set(instance, value);
    }
}
