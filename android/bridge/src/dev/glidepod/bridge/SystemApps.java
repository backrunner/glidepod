package dev.glidepod.bridge;

import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.util.AtomicFile;
import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;
import org.json.JSONArray;
import org.json.JSONObject;

/** Reversible desktop defaults. Never removes APKs, providers or user data. */
public final class SystemApps {
    private static final String[] OPTIONAL = {
        "com.android.deskclock", "com.android.contacts", "com.android.dialer",
        "com.android.messaging", "com.android.calendar", "com.android.music",
        "com.android.quicksearchbox", "com.android.egg", "com.android.dreams.basic",
        "com.android.dreams.phototable", "com.android.traceur", "com.android.devicediagnostics",
        "com.android.dynsystem", "com.android.threadnetwork.demoapp",
        "org.chromium.webview_shell", "com.android.virtualization.terminal",
        "com.android.stk", "com.android.simappdialog"
    };
    private static final AtomicFile RECORD = new AtomicFile(new File("/data/system/glidepod-system-apps.json"));
    private static void save(JSONObject states) throws Exception {
        byte[] data = new JSONObject().put("version", 1).put("original", states)
            .toString().getBytes(StandardCharsets.UTF_8);
        FileOutputStream stream = RECORD.startWrite();
        try {
            stream.write(data);
            RECORD.finishWrite(stream);
        } catch (Exception error) {
            RECORD.failWrite(stream);
            throw error;
        }
    }
    private static JSONObject read() throws Exception {
        if (!RECORD.getBaseFile().exists() && !new File(RECORD.getBaseFile().getPath() + ".bak").exists()) return new JSONObject();
        byte[] bytes;
        try (java.io.FileInputStream stream = RECORD.openRead()) { bytes = stream.readNBytes(65537); }
        if (bytes.length > 65536) throw new IllegalStateException("Oversized built-in app record");
        JSONObject record = new JSONObject(new String(bytes, StandardCharsets.UTF_8));
        if (record.getInt("version") != 1) throw new IllegalStateException("Unknown built-in app record");
        JSONObject original = record.getJSONObject("original");
        java.util.HashSet<String> allowed = new java.util.HashSet<>(java.util.Arrays.asList(OPTIONAL));
        java.util.Iterator<String> keys = original.keys();
        while (keys.hasNext()) {
            String key = keys.next();
            if (!allowed.contains(key) || original.getInt(key) < 0 || original.getInt(key) > 4)
                throw new IllegalStateException("Invalid built-in app record");
        }
        return original;
    }
    public static void main(String[] args) throws Exception {
        if (args.length != 1 || !(args[0].equals("minimal") || args[0].equals("full") || args[0].equals("status")))
            throw new IllegalArgumentException("Expected minimal, full or status");
        PackageManager pm = Management.context().getPackageManager();
        JSONObject original = read();
        JSONArray apps = new JSONArray();
        for (String name : OPTIONAL) {
            JSONObject item = new JSONObject().put("package_id", name).put("installed", false);
            try {
                ApplicationInfo app = pm.getApplicationInfo(name, 0);
                boolean system = (app.flags & ApplicationInfo.FLAG_SYSTEM) != 0;
                int current = pm.getApplicationEnabledSetting(name);
                if (system && args[0].equals("minimal")) {
                    // Journal the previous state before a package mutation; retries are idempotent.
                    if (!original.has(name)) { original.put(name, current); save(original); }
                    if (current != PackageManager.COMPONENT_ENABLED_STATE_DISABLED_USER)
                        pm.setApplicationEnabledSetting(name, PackageManager.COMPONENT_ENABLED_STATE_DISABLED_USER, PackageManager.SYNCHRONOUS);
                } else if (system && args[0].equals("full") && original.has(name)) {
                    // Preserve a choice changed outside GlidePod after our last operation.
                    if (current == PackageManager.COMPONENT_ENABLED_STATE_DISABLED_USER)
                        pm.setApplicationEnabledSetting(name, original.getInt(name), PackageManager.SYNCHRONOUS);
                    original.remove(name);
                    save(original);
                }
                app = pm.getApplicationInfo(name, 0);
                item.put("installed", true).put("system", system).put("enabled", app.enabled)
                    .put("managed", original.has(name)).put("label", pm.getApplicationLabel(app).toString());
            } catch (PackageManager.NameNotFoundException absent) { /* A different image may omit optional apps. */ }
            apps.put(item);
        }
        System.out.println(new JSONObject().put("version", 1).put("mode", args[0]).put("apps", apps));
        System.exit(0);
    }
}
