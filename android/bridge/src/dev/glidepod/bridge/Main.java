package dev.glidepod.bridge;

import android.content.*;
import android.content.pm.*;
import android.graphics.*;
import android.graphics.drawable.Drawable;
import android.os.Looper;
import android.util.Base64;
import java.io.ByteArrayOutputStream;
import java.util.*;
import org.json.*;

/** Small shell-identity catalog helper. No network, resident service, or third-party code. */
public final class Main {
    public static void main(String[] args) throws Exception {
        Looper.prepareMainLooper();
        Class<?> thread = Class.forName("android.app.ActivityThread");
        Object current = thread.getMethod("systemMain").invoke(null);
        Context system = (Context) thread.getMethod("getSystemContext").invoke(current);
        Context context = system.createPackageContext("com.android.shell", Context.CONTEXT_IGNORE_SECURITY);
        PackageManager pm = context.getPackageManager();
        Intent launcher = new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER);
        JSONArray output = new JSONArray();
        Set<String> seen = new HashSet<>();
        for (ResolveInfo info : pm.queryIntentActivities(launcher, 0)) {
            ActivityInfo activity = info.activityInfo;
            if (!seen.add(activity.packageName)) continue;
            ApplicationInfo app = activity.applicationInfo;
            PackageInfo pkg = pm.getPackageInfo(activity.packageName, 0);
            JSONObject item = new JSONObject();
            item.put("package_id", activity.packageName);
            item.put("component", new ComponentName(activity.packageName, activity.name).flattenToString());
            item.put("label", info.loadLabel(pm).toString());
            item.put("version", pkg.versionName == null ? "" : pkg.versionName);
            item.put("system", (app.flags & ApplicationInfo.FLAG_SYSTEM) != 0);
            item.put("game", app.category == ApplicationInfo.CATEGORY_GAME);
            item.put("updated", pkg.lastUpdateTime);
            Drawable drawable = info.loadIcon(pm);
            Bitmap bitmap = Bitmap.createBitmap(64, 64, Bitmap.Config.ARGB_8888);
            drawable.setBounds(0, 0, 64, 64);
            drawable.draw(new Canvas(bitmap));
            ByteArrayOutputStream png = new ByteArrayOutputStream();
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, png);
            item.put("icon_png", Base64.encodeToString(png.toByteArray(), Base64.NO_WRAP));
            output.put(item);
        }
        System.out.println(output);
        System.exit(0);
    }
}
