package io.canonic.testrun.media;

import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.os.Build;
import org.json.*;
import java.util.*;

/** One read of installed packages with the names a person sees on the home screen. */
final class Apps {
    private Apps() {}

    static JSONObject list(Context context) throws JSONException {
        PackageManager pm = context.getPackageManager();
        // The home screen shows a launcher activity's label, which can differ
        // from the application label; packages without one cannot be opened.
        Map<String, String> launchers = new HashMap<>();
        Intent launcher = new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER);
        for (ResolveInfo info : pm.queryIntentActivities(launcher, 0)) {
            String id = info.activityInfo.packageName;
            if (!launchers.containsKey(id)) launchers.put(id, label(info.loadLabel(pm)));
        }
        JSONArray apps = new JSONArray();
        for (PackageInfo info : pm.getInstalledPackages(0)) {
            String id = info.packageName;
            String name = launchers.get(id);
            if (name == null && info.applicationInfo != null) name = label(info.applicationInfo.loadLabel(pm));
            long build = Build.VERSION.SDK_INT >= 28 ? info.getLongVersionCode() : info.versionCode;
            apps.put(Main.obj("appId", id, "name", name == null ? "" : name,
                "version", info.versionName == null ? "" : Main.bounded(info.versionName),
                "build", Long.toString(build), "launchable", launchers.containsKey(id)));
        }
        return Main.obj("apps", apps);
    }

    private static String label(CharSequence value) {
        return value == null ? "" : Main.bounded(value.toString().trim());
    }
}
