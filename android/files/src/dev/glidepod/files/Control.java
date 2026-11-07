package dev.glidepod.files;

import android.content.*;
import android.database.Cursor;
import android.net.Uri;
import android.os.*;
import java.io.*;
import org.json.*;

/** Only the host's shell transport may inspect requests or create transfer objects. */
public final class Control extends ContentProvider {
    @Override public boolean onCreate() { Repository.init(getContext()); return true; }
    private void authorize() { getContext().enforceCallingPermission("android.permission.DUMP", "GlidePod private file transport"); }
    @Override public Bundle call(String method, String arg, Bundle extras) {
        authorize();
        long identity=Binder.clearCallingIdentity();
        try {
            if (arg != null && arg.length() > 32768) throw new IllegalArgumentException("Request exceeds limit");
            JSONObject a = arg == null ? new JSONObject() : new JSONObject(arg), result;
            if (method.equals("commit")) result=Repository.commit(a.getString("id"));
            else synchronized (Repository.class) {
                switch (method) {
                    case "poll": result = Repository.poll(); break;
                    case "create": {
                        String kind=a.optString("kind","import");if(!kind.equals("import")&&!kind.equals("clipboard"))throw new IllegalArgumentException("Invalid import kind");
                        result = Repository.create(a.getString("name"), a.getString("mime"), a.getLong("size"), a.getString("sha256"), kind); break;
                    }
                    case "retain": Repository.retain(a.getJSONArray("ids")); result=new JSONObject(); break;
                    case "discard": Repository.discard(a.getString("id"),false); result=new JSONObject(); break;
                    case "result": {
                        String id = a.getString("request");
                        Picker picker = Repository.pickers.get(id);
                        if (picker == null) throw new IllegalStateException("File selection expired");
                        // Retain before queueing the activity callback: a lost
                        // reply can trigger host rollback while the main thread
                        // has not yet delivered the activity result.
                        JSONArray ids=a.optJSONArray("ids");
                        if(!a.optBoolean("cancel")&&ids!=null&&ids.length()>0)Repository.retain(ids);
                        Repository.main.post(() -> picker.complete(a));
                        result = new JSONObject().put("accepted", true); break;
                    }
                    case "ack": {
                        String id = a.getString("id"); JSONObject m = Repository.metadata(id);
                        int revision = a.getInt("revision");
                        if (revision > m.getInt("revision")) throw new IllegalArgumentException("Unknown document revision");
                        m.put("acked", Math.max(m.optInt("acked"), revision)); Repository.record(id, m); result = m; break;
                    }
                    case "deliver": {
                        Repository.deliveries.entrySet().removeIf(e->SystemClock.elapsedRealtime()-e.getValue().optLong("created")>30000);
                        if (Repository.deliveries.size() >= 16) throw new IllegalStateException("Too many pending file deliveries");
                        Repository.retain(a.getJSONArray("ids"));
                        String id = Repository.id(); a.put("created", SystemClock.elapsedRealtime());
                        Repository.deliveries.put(id, a); result = new JSONObject().put("token", id); break;
                    }
                    case "metadata": result = Repository.metadata(a.getString("id")); result.put("writing",Repository.writers.contains(a.getString("id"))); break;
                    case "clipboard_pin": Repository.pinClipboard(a.getJSONArray("ids"));result=new JSONObject();break;
                    case "clipboard": {
                        JSONArray ids=a.getJSONArray("ids"),uris=new JSONArray();
                        if(ids.length()<1||ids.length()>64)throw new IllegalArgumentException("Invalid clipboard file count");
                        for(int i=0;i<ids.length();i++){
                            String id=ids.getString(i);if(!Repository.metadata(id).getBoolean("ready"))throw new IllegalStateException("File is incomplete");
                            Uri uri=Repository.uri(id);getContext().grantUriPermission("dev.glidepod.input",uri,Intent.FLAG_GRANT_READ_URI_PERMISSION);
                            int uid=getContext().getPackageManager().getApplicationInfo("dev.glidepod.input",0).uid;
                            if(getContext().checkUriPermission(uri,-1,uid,Intent.FLAG_GRANT_READ_URI_PERMISSION)!=android.content.pm.PackageManager.PERMISSION_GRANTED)throw new SecurityException("Clipboard URI grant was not installed");
                            uris.put(uri.toString());
                        }
                        result=new JSONObject().put("uris",uris);break;
                    }
                    default: throw new IllegalArgumentException("Unknown file operation");
                }
            }
            Bundle b = new Bundle(); b.putString("json", result.toString()); return b;
        } catch (Exception e) { throw new IllegalStateException(e.getMessage(), e); }
        finally { Binder.restoreCallingIdentity(identity); }
    }
    @Override public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        authorize();
        try { return Repository.open(uri.getLastPathSegment(), mode, true); }
        catch (Exception e) { throw new FileNotFoundException(e.getMessage()); }
    }
    @Override public Cursor query(Uri u,String[] p,String s,String[] a,String o) { throw new UnsupportedOperationException(); }
    @Override public String getType(Uri u) { return "application/octet-stream"; }
    @Override public Uri insert(Uri u,ContentValues v) { throw new UnsupportedOperationException(); }
    @Override public int delete(Uri u,String s,String[] a) { throw new UnsupportedOperationException(); }
    @Override public int update(Uri u,ContentValues v,String s,String[] a) { throw new UnsupportedOperationException(); }
}
