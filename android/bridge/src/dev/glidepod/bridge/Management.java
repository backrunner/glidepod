package dev.glidepod.bridge;

import android.app.usage.*;
import android.content.*;
import android.content.pm.*;
import android.os.*;
import java.util.*;
import org.json.*;

/** Shell-identity queries. Mutations remain explicit, typed host operations. */
public final class Management {
    public static Context context() throws Exception {
        Looper.prepareMainLooper();
        Class<?> thread=Class.forName("android.app.ActivityThread");
        Object current=thread.getMethod("systemMain").invoke(null);
        Context system=(Context)thread.getMethod("getSystemContext").invoke(current);
        return system.createPackageContext("com.android.shell",Context.CONTEXT_IGNORE_SECURITY);
    }
    public static void main(String[] args) throws Exception {
        Context c=context();
        JSONObject out=new JSONObject();
        if(args.length==1 && args[0].equals("storage")) {
            StatFs fs=new StatFs(Environment.getDataDirectory().getPath());
            out.put("total_bytes",fs.getTotalBytes()).put("available_bytes",fs.getAvailableBytes());
        } else if(args.length==2 && args[0].equals("details")) {
            PackageManager pm=c.getPackageManager();
            PackageInfo pkg=pm.getPackageInfo(args[1],PackageManager.GET_PERMISSIONS);
            ApplicationInfo app=pkg.applicationInfo;
            out.put("package_id",pkg.packageName).put("label",app.loadLabel(pm).toString());
            out.put("version",pkg.versionName==null?"":pkg.versionName).put("uid",app.uid);
            out.put("system",(app.flags&ApplicationInfo.FLAG_SYSTEM)!=0);
            String[] shared=pm.getPackagesForUid(app.uid);
            out.put("shared_uid",shared!=null && shared.length>1);
            try {
                StorageStats s=c.getSystemService(StorageStatsManager.class).queryStatsForPackage(app.storageUuid,pkg.packageName,android.os.Process.myUserHandle());
                out.put("storage",new JSONObject().put("app_bytes",s.getAppBytes()).put("data_bytes",s.getDataBytes()).put("cache_bytes",s.getCacheBytes()));
            } catch(Exception e) { out.put("storage_error",e.toString()); }
            JSONArray permissions=new JSONArray();
            if(pkg.requestedPermissions!=null) for(int i=0;i<pkg.requestedPermissions.length;i++) {
                String name=pkg.requestedPermissions[i];
                try {
                    PermissionInfo info=pm.getPermissionInfo(name,0);
                    if((info.protectionLevel&PermissionInfo.PROTECTION_MASK_BASE)!=PermissionInfo.PROTECTION_DANGEROUS) continue;
                    int flags=(Integer)PackageManager.class.getMethod("getPermissionFlags",String.class,String.class,UserHandle.class).invoke(pm,name,pkg.packageName,android.os.Process.myUserHandle());
                    permissions.put(new JSONObject().put("name",name).put("label",info.loadLabel(pm).toString())
                        .put("granted",(pkg.requestedPermissionsFlags[i]&PackageInfo.REQUESTED_PERMISSION_GRANTED)!=0)
                        .put("fixed",(flags&(4|16))!=0));
                } catch(PackageManager.NameNotFoundException ignored) {}
            }
            out.put("permissions",permissions);
        } else throw new IllegalArgumentException("Unknown management query");
        System.out.println(out); System.exit(0);
    }
}
