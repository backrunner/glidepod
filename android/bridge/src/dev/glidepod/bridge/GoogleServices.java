package dev.glidepod.bridge;

import android.content.Context;
import android.content.pm.*;
import android.os.Build;
import java.security.MessageDigest;
import org.json.*;

/** Local prerequisites only. Never reads accounts, device IDs or messaging tokens. */
public final class GoogleServices {
    public static void main(String[] args) throws Exception {
        Context context = Management.context();
        PackageManager pm = context.getPackageManager();
        JSONArray packages = new JSONArray();
        for (String name : new String[]{"com.google.android.gms", "com.google.android.gsf", "com.android.vending"}) {
            JSONObject item = new JSONObject().put("package_id", name).put("installed", false)
                .put("enabled", false).put("system", false).put("version", "").put("version_code", 0)
                .put("signer_sha256", new JSONArray());
            try {
                PackageInfo pkg = pm.getPackageInfo(name, PackageManager.GET_SIGNING_CERTIFICATES);
                ApplicationInfo app = pkg.applicationInfo;
                item.put("installed", true).put("enabled", app.enabled)
                    .put("system", (app.flags & ApplicationInfo.FLAG_SYSTEM) != 0)
                    .put("version", pkg.versionName == null ? "" : pkg.versionName)
                    .put("version_code", pkg.getLongVersionCode());
                JSONArray signers = new JSONArray();
                if (pkg.signingInfo != null) for (Signature signature : pkg.signingInfo.getApkContentsSigners()) {
                    byte[] digest = MessageDigest.getInstance("SHA-256").digest(signature.toByteArray());
                    StringBuilder hex = new StringBuilder();
                    for (byte b : digest) hex.append(String.format(java.util.Locale.ROOT, "%02x", b & 255));
                    signers.put(hex.toString());
                }
                item.put("signer_sha256", signers);
            } catch (PackageManager.NameNotFoundException absent) { /* Explicit missing state. */ }
            packages.put(item);
        }
        System.out.println(new JSONObject().put("sdk", Build.VERSION.SDK_INT)
            .put("release", Build.VERSION.RELEASE).put("build_type", Build.TYPE).put("build_tags", Build.TAGS)
            .put("fingerprint", Build.FINGERPRINT).put("abis", new JSONArray(Build.SUPPORTED_ABIS))
            .put("packages", packages));
        System.exit(0);
    }
}
