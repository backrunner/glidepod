package dev.glidepod.bridge;

import android.content.*;
import android.database.Cursor;
import android.net.Uri;
import android.os.*;
import android.system.*;
import java.io.*;
import java.security.MessageDigest;
import java.util.*;
import org.json.*;

/** Shell-owned transport: fixed staging paths and read-only public-download discovery. */
public final class Files {
    static final String ROOT = "/data/local/tmp/glidepod/";
    static final long LIMIT = 64L * 1024 * 1024 * 1024;
    static void space(long bytes) throws IOException {if(new StatFs(ROOT).getAvailableBytes()<512L*1024*1024+bytes)throw new IOException("Android storage is low; transfer will retry when space is available");}
    static final Uri CONTROL = Uri.parse("content://dev.glidepod.files.bridge");
    // app_process is not a registered ApplicationThread. Acquire external provider
    // references exactly as Android's content shell command does.
    static final Map<String, ContentResolver> resolvers = new HashMap<>();
    static final Map<String, IBinder> tokens = new HashMap<>();
    static Object manager;
    static ContentResolver resolver(Context context, Uri uri) throws Exception {
        String authority=uri.getAuthority();
        if(resolvers.containsKey(authority))return resolvers.get(authority);
        if(manager==null)manager=Class.forName("android.app.ActivityManager").getMethod("getService").invoke(null);
        Class<?> api=Class.forName("android.app.IActivityManager");IBinder token=new Binder();
        Object holder=api.getMethod("getContentProviderExternal",String.class,int.class,IBinder.class,String.class).invoke(manager,authority,0,token,"GlidePod files");
        if(holder==null)throw new IOException("Content provider is unavailable");
        Object provider=holder.getClass().getField("provider").get(holder);
        ContentProviderClient client=(ContentProviderClient)ContentProviderClient.class.getConstructor(ContentResolver.class,Class.forName("android.content.IContentProvider"),String.class,boolean.class).newInstance(context.getContentResolver(),provider,authority,true);
        ContentResolver wrapped=ContentResolver.wrap(client);resolvers.put(authority,wrapped);tokens.put(authority,token);return wrapped;
    }
    static void release() throws Exception {
        if(manager!=null)for(Map.Entry<String,IBinder> token:tokens.entrySet())Class.forName("android.app.IActivityManager").getMethod("removeContentProviderExternalAsUser",String.class,IBinder.class,int.class).invoke(manager,token.getKey(),token.getValue(),0);
    }

    static JSONObject call(Context c, String method, JSONObject arg) throws Exception {
        Bundle b = resolver(c,CONTROL).call(CONTROL, method, arg == null ? null : arg.toString(), null);
        if (b == null) throw new IOException("File provider disconnected"); return new JSONObject(b.getString("json"));
    }
    static String hash(byte[] bytes) throws Exception {
        byte[] digest=MessageDigest.getInstance("SHA-256").digest(bytes); StringBuilder s=new StringBuilder();
        for(byte b:digest)s.append(String.format(Locale.ROOT,"%02x",b&255));return s.toString();
    }
    static String version(File file) throws Exception {
        // FUSE node/inode IDs can change after cache eviction without a content
        // change. Use the Developer agent's underlying public-file inode.
        String logical=file.getCanonicalPath();String prefix="/storage/emulated/0/Download/";
        if(!logical.startsWith(prefix)||android.os.Process.myUid()!=0)throw new SecurityException("Public download identity requires the image export agent");
        File backing=new File("/data/media/0/Download",logical.substring(prefix.length())).getCanonicalFile();
        if(!backing.getPath().startsWith("/data/media/0/Download/"))throw new SecurityException("Download escaped public storage");
        StructStat s=Os.stat(backing.getPath());
        return hash((file.getCanonicalPath()+":"+s.st_dev+":"+s.st_ino+":"+s.st_size+":"+s.st_mtim.tv_sec+":"+s.st_mtim.tv_nsec+":"+s.st_ctim.tv_sec+":"+s.st_ctim.tv_nsec).getBytes(java.nio.charset.StandardCharsets.UTF_8));
    }
    static File publicDownload(String path) throws Exception {
        if(path==null)return null; File f=new File(path).getCanonicalFile();
        String root=new File("/storage/emulated/0/Download").getCanonicalPath()+"/";
        return f.getPath().startsWith(root)&&f.isFile()&&f.length()<=LIMIT?f:null;
    }
    static String existingVersion(File file) throws Exception {
        try { return version(file); }
        catch(ErrnoException e) { if(e.errno==OsConstants.ENOENT)return null; throw e; }
    }
    static JSONArray downloads(Context c) throws Exception {
        LinkedHashMap<String,JSONObject> rows=new LinkedHashMap<>();
        // DownloadManager's successful public files. Its private app destinations are excluded.
        try(Cursor cursor=resolver(c,Uri.parse("content://downloads/all_downloads")).query(Uri.parse("content://downloads/all_downloads"),
            new String[]{"_id","_data","status","is_visible_in_downloads_ui","mimetype"},"status=200 AND is_visible_in_downloads_ui=1",null,"_id DESC")) {
            if(cursor!=null)while(cursor.moveToNext()) {
                if(rows.size()>=4096)throw new IOException("More than 4096 public downloads; archive older Android downloads before retrying");
                File f=publicDownload(cursor.getString(1)); if(f==null)continue;
                String key=existingVersion(f); if(key==null)continue; rows.put(key,new JSONObject().put("key",key).put("name",f.getName()).put("size",f.length())
                    .put("mime",cursor.getString(4)).put("uri","content://downloads/all_downloads/"+cursor.getLong(0)).put("path",f.getPath()));
            }
        }
        // Apps using MediaStore instead of DownloadManager publish by clearing IS_PENDING.
        try(Cursor cursor=resolver(c,Uri.parse("content://media/external_primary/downloads")).query(Uri.parse("content://media/external_primary/downloads"),
            new String[]{"_id","_data","mime_type"},"is_pending=0 AND is_trashed=0",null,"_id DESC")) {
            if(cursor!=null)while(cursor.moveToNext()) {
                if(rows.size()>=4096)throw new IOException("More than 4096 public downloads; archive older Android downloads before retrying");
                File f=publicDownload(cursor.getString(1)); if(f==null)continue; String key=existingVersion(f); if(key==null)continue;
                if(!rows.containsKey(key))rows.put(key,new JSONObject().put("key",key).put("name",f.getName()).put("size",f.length())
                    .put("mime",cursor.getString(2)).put("uri","content://media/external_primary/downloads/"+cursor.getLong(0)).put("path",f.getPath()));
            }
        }
        return new JSONArray(rows.values());
    }
    static JSONObject snapshot(Context c, JSONObject source) throws Exception {
        File staged=new File(ROOT+"export-"+UUID.randomUUID()+".bin");
        try {
            space(Math.max(0,source.optLong("size",0)));
            MessageDigest digest=MessageDigest.getInstance("SHA-256"); long total=0; byte[] buffer=new byte[65536];
            try(InputStream in=resolver(c,Uri.parse(source.getString("uri"))).openInputStream(Uri.parse(source.getString("uri"))); FileOutputStream out=new FileOutputStream(staged)) {
                if(in==null)throw new IOException("Export source is unavailable"); int n;
                while((n=in.read(buffer))!=-1){if(total%(16*1024*1024)==0)space(n);total+=n;if(total>LIMIT)throw new IOException("Export exceeds transfer limit");out.write(buffer,0,n);digest.update(buffer,0,n);} out.getFD().sync();
            }
            if(source.optLong("size",-1)>=0&&total!=source.getLong("size"))throw new IOException("Export source changed");
            if(source.has("path")&&!version(new File(source.getString("path"))).equals(source.getString("key")))throw new IOException("Download changed during export");
            if(android.os.Process.myUid()==0)Os.chown(staged.getPath(),2000,2000);
            Os.chmod(staged.getPath(),0600);
            StringBuilder sha=new StringBuilder();for(byte b:digest.digest())sha.append(String.format(Locale.ROOT,"%02x",b&255));
            return new JSONObject().put("path",staged.getPath()).put("size",total).put("sha256",sha.toString());
        }catch(Exception e){staged.delete();throw e;}
    }
    public static void main(String[] args) throws Exception {
        Context c=Management.context(); JSONObject out=new JSONObject();
        if(args.length==4&&args[0].equals("can-deliver")) {
            Intent intent=new Intent(args[3].equals("1")?Intent.ACTION_SEND:Intent.ACTION_SEND_MULTIPLE).setPackage(args[1]).setType(args[2]);
            boolean supported=false;
            for(android.content.pm.ResolveInfo info:c.getPackageManager().queryIntentActivities(intent,android.content.pm.PackageManager.MATCH_DEFAULT_ONLY)) {
                String permission=info.activityInfo.permission;
                if(info.activityInfo.exported&&(permission==null||c.getPackageManager().checkPermission(permission,"dev.glidepod.files")==android.content.pm.PackageManager.PERMISSION_GRANTED))supported=true;
            }
            out.put("supported",supported);
        } else if(args.length==1&&args[0].equals("downloads")) {
            JSONArray list=downloads(c);for(int i=0;i<list.length();i++){list.getJSONObject(i).remove("path");list.getJSONObject(i).remove("uri");}
            out.put("items",list);
        } else if(args.length==2&&args[0].equals("download")) {
            JSONObject selected=null;JSONArray list=downloads(c);for(int i=0;i<list.length();i++)if(list.getJSONObject(i).getString("key").equals(args[1]))selected=list.getJSONObject(i);
            if(selected==null)throw new FileNotFoundException("Download changed or was removed"); out=snapshot(c,selected);
        } else if(args.length==3&&args[0].equals("export")) {
            JSONObject m=call(c,"metadata",new JSONObject().put("id",args[1]));
            if(m.getInt("revision")!=Integer.parseInt(args[2]))throw new IOException("Document changed");
            out=snapshot(c,new JSONObject().put("uri",CONTROL.toString()+"/"+args[1]).put("size",m.getLong("size")));
            if(!out.getString("sha256").equals(m.getString("sha256"))) {new File(out.getString("path")).delete();throw new IOException("Document changed during export");}
        } else if(args.length==2&&args[0].equals("clipboard-set")) {
            if(!args[1].matches("/data/local/tmp/glidepod/clipboard-[0-9a-f]{64}\\.json"))throw new IllegalArgumentException("Invalid clipboard transfer");
            File file=new File(args[1]);if(file.length()>524288)throw new IOException("Clipboard transfer exceeds limit");
            byte[] data=new byte[(int)file.length()];try(DataInputStream in=new DataInputStream(new FileInputStream(file))){in.readFully(data);}
            Bundle b=resolver(c,Uri.parse("content://dev.glidepod.input.clipboard")).call(Uri.parse("content://dev.glidepod.input.clipboard"),"set",new String(data,java.nio.charset.StandardCharsets.UTF_8),null);
            out=new JSONObject(b.getString("json"));
        } else if(args.length==3&&args[0].equals("clipboard-item")) {
            Uri control=Uri.parse("content://dev.glidepod.input.clipboard");
            JSONObject before=new JSONObject(resolver(c,control).call(control,"read",null,null).getString("json"));
            if(!before.getString("version").equals(args[1]))throw new IOException("Clipboard changed");
            int index=Integer.parseInt(args[2]);if(index<0||index>=before.getJSONArray("items").length())throw new IOException("Unknown clipboard item");
            Uri uri=control.buildUpon().appendPath(Integer.toString(index)).appendQueryParameter("version",args[1]).build();
            out=snapshot(c,new JSONObject().put("uri",uri.toString()).put("size",-1));
            JSONObject after=new JSONObject(resolver(c,control).call(control,"read",null,null).getString("json"));
            if(!after.getString("version").equals(args[1])){new File(out.getString("path")).delete();throw new IOException("Clipboard changed during export");}
        } else if(args.length==3&&args[0].equals("import")) {
            if(!args[1].matches("[0-9a-f-]{36}")||!args[2].matches("/data/local/tmp/glidepod/import-[0-9a-f-]{36}\\.bin"))throw new IllegalArgumentException("Invalid transfer");
            File source=new File(args[2]);JSONObject m=call(c,"metadata",new JSONObject().put("id",args[1]));
            if(source.length()!=m.getLong("size"))throw new IOException("Import size mismatch");
            ParcelFileDescriptor p=resolver(c,CONTROL).openFileDescriptor(Uri.parse(CONTROL.toString()+"/"+args[1]),"w");
            try(InputStream in=new FileInputStream(source); ParcelFileDescriptor.AutoCloseOutputStream dest=new ParcelFileDescriptor.AutoCloseOutputStream(p)) {
                byte[] buffer=new byte[65536];long total=0;int n;while((n=in.read(buffer))!=-1){if(total%(16*1024*1024)==0)space(n);total+=n;if(total>m.getLong("size"))throw new IOException("Import grew during transfer");dest.write(buffer,0,n);}dest.getFD().sync();
            }
            out=call(c,"commit",new JSONObject().put("id",args[1]));
        } else throw new IllegalArgumentException("Unknown file transport operation");
        release();System.out.println(out);System.exit(0);
    }
}
