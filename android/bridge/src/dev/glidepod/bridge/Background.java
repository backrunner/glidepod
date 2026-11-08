package dev.glidepod.bridge;

import android.app.ActivityManager;
import android.content.Context;
import android.content.pm.PackageManager;
import android.database.Cursor;
import android.media.AudioManager;
import org.json.JSONObject;

/** Conservative idle evidence. Failure is unknown, never permission to stop Android. */
public final class Background {
    private static boolean userUid(Context context, int uid) throws Exception {
        if (uid < 10000) return false;
        String[] packages = context.getPackageManager().getPackagesForUid(uid);
        if (packages == null || packages.length == 0) return true;
        for (String name : packages) {
            int flags = context.getPackageManager().getApplicationInfo(name, 0).flags;
            if ((flags & android.content.pm.ApplicationInfo.FLAG_SYSTEM) == 0) return true;
        }
        return false;
    }
    @SuppressWarnings("deprecation")
    public static void main(String[] args) throws Exception {
        Context context = Management.context();
        boolean busy = false;
        // Enabled push infrastructure may receive traffic while otherwise completely idle.
        try {
            busy = context.getPackageManager().getApplicationInfo("com.google.android.gms", 0).enabled;
        } catch (PackageManager.NameNotFoundException absent) { }
        ActivityManager manager = context.getSystemService(ActivityManager.class);
        for (ActivityManager.RunningServiceInfo service : manager.getRunningServices(1000)) {
            // Pinned Cuttlefish device plumbing is always foreground, even with no app work.
            // Exempt this exact component only; user and other system foreground services retain the VM.
            if (service.service.getPackageName().equals("com.android.google.gce.gceservice")
                && service.service.getClassName().equals("com.android.google.gce.gceservice.GceService")) continue;
            if (service.foreground || (service.started && userUid(context, service.uid)
                && !service.service.getPackageName().startsWith("dev.glidepod."))) busy = true;
        }
        if (context.getSystemService(AudioManager.class).isMusicActive()) busy = true;
        // Read only activity status through the Developer image's internal root identity.
        try (Cursor cursor = Files.resolver(context, android.net.Uri.parse("content://downloads/all_downloads")).query(
                android.net.Uri.parse("content://downloads/all_downloads"),
                new String[]{"status", "uid"}, "status < 200", null, null)) {
            if (cursor == null) throw new IllegalStateException("Download state unavailable");
            // Pending system maintenance (e.g. the framework CT list) is not an active download.
            // All running downloads, and pending/paused user-app downloads, retain Android.
            while (cursor.moveToNext()) {
                if (cursor.getInt(0) == 192 || userUid(context, cursor.getInt(1))) busy = true;
            }
        } finally {
            Files.release();
        }
        System.out.println(new JSONObject().put("busy", busy));
        System.exit(0);
    }
}
