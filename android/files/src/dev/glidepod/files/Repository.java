package dev.glidepod.files;

import android.content.*;
import android.net.Uri;
import android.os.*;
import android.provider.DocumentsContract;
import android.util.AtomicFile;
import java.io.*;
import java.security.MessageDigest;
import java.util.*;
import org.json.*;

/** Private snapshots, stable document IDs and bounded pending UI requests. */
final class Repository {
    static final long MAX_BYTES = 64L * 1024 * 1024 * 1024;
    static final int MAX_ITEMS = 64;
    static final int MAX_DOCUMENTS = 4096;
    static final long RESERVE = 512L * 1024 * 1024;
    static final HandlerThread ioThread = new HandlerThread("GlidePod document IO");
    static { ioThread.start(); }
    static final Handler io = new Handler(ioThread.getLooper());
    static final java.util.concurrent.ExecutorService shares = new java.util.concurrent.ThreadPoolExecutor(1, 1, 0L, java.util.concurrent.TimeUnit.MILLISECONDS, new java.util.concurrent.ArrayBlockingQueue<Runnable>(8));
    static void space(long bytes) throws IOException {
        if (new StatFs(context.getFilesDir().getPath()).getAvailableBytes() < RESERVE + bytes) throw new IOException("Android storage is low; free space before transferring files");
    }
    static final Map<String, Picker> pickers = new LinkedHashMap<>();
    static final Map<String, JSONObject> requests = new LinkedHashMap<>();
    static final Set<String> writers = new HashSet<>();
    static final Map<String, JSONObject> deliveries = new HashMap<>();
    static final Handler main = new Handler(Looper.getMainLooper());
    static Context context;
    static long lastClean;
    static final ArrayList<String> errors = new ArrayList<>();
    static Set<String> readPins(String name) throws Exception {
        AtomicFile file=new AtomicFile(new File(context.getFilesDir(),name));Set<String> ids=new LinkedHashSet<>();
        if(file.getBaseFile().exists())try(InputStream in=file.openRead()){
            JSONArray list=new JSONArray(new String(readBounded(in,32768),java.nio.charset.StandardCharsets.UTF_8));
            if(list.length()>320)throw new IOException("Invalid clipboard retention record");
            for(int i=0;i<list.length();i++)ids.add(valid(list.getString(i)));
        }return ids;
    }
    static void writePins(String name,Collection<String> ids) throws Exception {
        AtomicFile file=new AtomicFile(new File(context.getFilesDir(),name));byte[] bytes=new JSONArray(ids).toString().getBytes(java.nio.charset.StandardCharsets.UTF_8);
        FileOutputStream out=file.startWrite();
        try{out.write(bytes);out.getFD().sync();file.finishWrite(out);}catch(Exception e){file.failWrite(out);throw e;}
        try(InputStream in=file.openRead()){if(!Arrays.equals(bytes,readBounded(in,32768)))throw new IOException("Clipboard retention was not committed");}
    }
    static void pinClipboard(JSONArray ids)throws Exception {
        if(ids.length()>MAX_ITEMS)throw new IllegalArgumentException("Too many clipboard files");
        Set<String> current=new LinkedHashSet<>();for(int i=0;i<ids.length();i++)current.add(valid(ids.getString(i)));
        Set<String> previous=readPins("clipboard-current.json");previous.addAll(readPins("clipboard-retained.json"));
        // Lost replies must not revoke content that ClipboardManager may already
        // have accepted. A later confirmed update supersedes these candidates.
        for(String id:previous)if(!current.contains(id)){
            JSONObject m;try{m=metadata(id);}catch(Exception malformed){continue;}
            if(m.optString("kind").equals("clipboard")){m.put("superseded",System.currentTimeMillis());record(id,m);}
        }
        writePins("clipboard-current.json",current);
        writePins("clipboard-retained.json",Collections.emptySet());
    }
    static void clean()throws Exception{
        if(SystemClock.elapsedRealtime()-lastClean<60000)return;lastClean=SystemClock.elapsedRealtime();
        Set<String> pinned=new HashSet<>();File pin=new File(context.getFilesDir(),"clipboard-current.json");
        if(pin.exists())try(InputStream in=new AtomicFile(pin).openRead()){JSONArray list=new JSONArray(new String(readBounded(in,8192),java.nio.charset.StandardCharsets.UTF_8));for(int i=0;i<list.length();i++)pinned.add(list.getString(i));}
        File[] files=new File(context.getFilesDir(),"documents").listFiles((dir,name)->name.endsWith(".json"));
        if(files!=null)for(File f:files){String id=f.getName().substring(0,f.getName().length()-5);JSONObject m;try{m=metadata(id);}catch(Exception corrupt){continue;}
            boolean temporary=(m.optString("kind").equals("clipboard")&&(!m.optBoolean("delivered")||m.has("superseded")))||(!m.optBoolean("ready")&&Arrays.asList("import","share").contains(m.optString("kind")));
            if(temporary&&!pinned.contains(id)&&!writers.contains(id)&&System.currentTimeMillis()-m.optLong("superseded",m.optLong("created"))>3600000){
                context.revokeUriPermission(uri(id),Intent.FLAG_GRANT_READ_URI_PERMISSION);
                content(m).delete();file(id,".rev-"+(m.optLong("revision")+1)+".bin").delete();
                for(String suffix:new String[]{".bin",".part",".json",".json.bak"})file(id,suffix).delete();
            }
        }
    }

    static synchronized void init(Context c) { context = c.getApplicationContext(); new File(context.getFilesDir(), "documents").mkdirs(); }
    static String id() { return UUID.randomUUID().toString(); }
    static String valid(String id) {
        if (id == null || !id.matches("[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}")) throw new IllegalArgumentException("Invalid document ID");
        return id;
    }
    static File file(String id, String suffix) { return new File(new File(context.getFilesDir(), "documents"), valid(id) + suffix); }
    static Uri uri(String id) { return DocumentsContract.buildDocumentUri("dev.glidepod.files.documents", valid(id)); }
    static String name(String value) {
        if (value == null || value.isEmpty()) return "Untitled";
        String s = value.replaceAll("[\\p{Cntrl}/\\\\]", "_");
        s=s.substring(0,s.offsetByCodePoints(0,Math.min(s.codePointCount(0,s.length()),255)));
        while(s.getBytes(java.nio.charset.StandardCharsets.UTF_8).length>255)s=s.substring(0,s.offsetByCodePoints(s.length(),-1));
        return s;
    }
    static String mime(String value) {
        if (value == null) return "application/octet-stream";
        if (value.length() > 128 || !value.matches("[a-zA-Z0-9!#$&^_.+*-]+/[a-zA-Z0-9!#$&^_.+*-]+")) throw new IllegalArgumentException("Invalid content type");
        return value.toLowerCase(Locale.ROOT);
    }
    static synchronized JSONObject metadata(String id) throws Exception {
        File f = file(id, ".json");
        if (f.length() > 8192) throw new IOException("Invalid document metadata");
        try (InputStream in = new AtomicFile(f).openRead()) {
            JSONObject m=new JSONObject(new String(readBounded(in, 8192), java.nio.charset.StandardCharsets.UTF_8));
            if(!id.equals(m.getString("id"))||m.getLong("size")<0||m.getLong("size")>MAX_BYTES||m.getLong("revision")<0
                ||!Arrays.asList("import","clipboard","save","share").contains(m.getString("kind"))
                ||m.getString("name").getBytes(java.nio.charset.StandardCharsets.UTF_8).length>1024)
                throw new IOException("Invalid document metadata");
            mime(m.getString("mime"));
            if(m.getBoolean("ready")&&(m.getLong("revision")<1||!m.getString("sha256").matches("[0-9a-f]{64}")))throw new IOException("Invalid published document metadata");
            if(m.has("blob_revision")&&m.getLong("blob_revision")!=m.getLong("revision"))throw new IOException("Document revision does not match content");
            m.getLong("created");
            return m;
        }
    }
    static synchronized void record(String id, JSONObject value) throws Exception {
        AtomicFile f = new AtomicFile(file(id, ".json"));
        FileOutputStream out = f.startWrite();
        byte[] bytes=value.toString().getBytes(java.nio.charset.StandardCharsets.UTF_8);
        try { out.write(bytes); out.getFD().sync(); f.finishWrite(out); }
        catch (Exception e) { f.failWrite(out); throw e; }
        // AtomicFile logs some rename/sync failures instead of throwing them.
        // Verify the commit before retiring the previous content revision.
        try(InputStream in=f.openRead()) {
            if(!Arrays.equals(bytes,readBounded(in,8192)))throw new IOException("Document metadata was not committed");
        }
    }
    static byte[] readBounded(InputStream input, int maximum) throws Exception {
        ByteArrayOutputStream out = new ByteArrayOutputStream(); byte[] buffer = new byte[4096]; int n;
        while ((n = input.read(buffer)) != -1) { if (out.size() + n > maximum) throw new IOException("Response exceeds limit"); out.write(buffer, 0, n); }
        return out.toByteArray();
    }
    static String hash(File f) throws Exception {
        MessageDigest hash = MessageDigest.getInstance("SHA-256"); byte[] buffer = new byte[65536];
        try (InputStream in = new FileInputStream(f)) { int n; while ((n = in.read(buffer)) != -1) hash.update(buffer, 0, n); }
        StringBuilder s = new StringBuilder(); for (byte b : hash.digest()) s.append(String.format(java.util.Locale.ROOT, "%02x", b & 255)); return s.toString();
    }
    static synchronized JSONObject create(String displayName, String type, long size, String digest, String kind) throws Exception {
        if (size < 0 || size > MAX_BYTES) throw new IOException("File exceeds transfer limit");
        space((kind.equals("import")||kind.equals("clipboard"))?size*2:size);
        File[] existing = new File(context.getFilesDir(), "documents").listFiles((dir, name) -> name.endsWith(".json"));
        if (existing != null && existing.length >= MAX_DOCUMENTS) throw new IOException("Mac Files contains too many documents; remove unneeded files in Android Files");
        String id = id();
        JSONObject m = new JSONObject().put("id", id).put("name", name(displayName)).put("mime", mime(type))
            .put("size", size).put("sha256", digest).put("kind", kind).put("ready", false).put("revision", 0).put("created", System.currentTimeMillis());
        record(id, m); return m;
    }
    // Content and metadata must commit together. Keep the last published blob
    // until AtomicFile has committed the new revision; a crash can only leave an
    // unreachable new blob, never old metadata pointing at replacement bytes.
    static File content(JSONObject m) throws Exception {
        return file(m.getString("id"), m.has("blob_revision") ? ".rev-" + m.getLong("blob_revision") + ".bin" : ".bin");
    }
    static void syncDirectory() throws Exception {
        java.io.FileDescriptor fd=android.system.Os.open(new File(context.getFilesDir(),"documents").getPath(),android.system.OsConstants.O_RDONLY,0);
        try{android.system.Os.fsync(fd);}finally{android.system.Os.close(fd);}
    }
    static void saveError(String id) {
        synchronized(Repository.class){try{JSONObject m=metadata(id);m.put("error","Could not finish saving "+m.getString("name")+". Check Android storage and save again.");record(id,m);}catch(Exception ignored){}}
    }
    static synchronized void publish(JSONObject m, File part) throws Exception {
        String id=m.getString("id"); JSONObject previous=metadata(id); File old=content(previous);
        long revision=previous.getLong("revision")+1;
        File next=file(id,".rev-"+revision+".bin");
        if(!part.renameTo(next))throw new IOException("Cannot publish document content");
        m.put("blob_revision",revision).put("revision",revision).put("ready",true).put("modified",System.currentTimeMillis());
        syncDirectory();
        m.remove("error");
        record(id,m);
        syncDirectory();
        if(!old.equals(next))old.delete();
        context.getContentResolver().notifyChange(uri(id),null);
    }
    static JSONObject commit(String id) throws Exception {
        JSONObject m = metadata(id); File part = file(id, ".part");
        if (m.getBoolean("ready")) return m;
        if (!part.isFile() || part.length() != m.getLong("size") || !hash(part).equals(m.getString("sha256"))) throw new IOException("File transfer did not match its checksum");
        publish(m, part); return m;
    }
    static synchronized void retain(JSONArray ids) throws Exception {
        if(ids.length()<1||ids.length()>MAX_ITEMS)throw new IOException("Invalid retained file count");
        Set<String> candidates=readPins("clipboard-retained.json");boolean clipboard=false;
        for(int i=0;i<ids.length();i++){String id=ids.getString(i);if(metadata(id).optString("kind").equals("clipboard")){candidates.add(id);clipboard=true;}}
        if(clipboard){if(candidates.size()>256)throw new IOException("Too many unfinished clipboard transfers; copy text to finish recovery");writePins("clipboard-retained.json",candidates);}
        for(int i=0;i<ids.length();i++){String id=ids.getString(i);JSONObject m=metadata(id);
            if(!m.optBoolean("ready"))throw new IOException("File transfer is incomplete");
            m.put("delivered",true);record(id,m);
        }
    }
    static synchronized void discard(String id, boolean userDelete) throws Exception {
        JSONObject m=metadata(id);
        if(m.optBoolean("delivered")&&!userDelete)throw new IOException("Document has already been offered to an app");
        if((m.optBoolean("export")&&m.optLong("acked")<m.getLong("revision")) || writers.contains(id))
            throw new IOException("Document is still in use");
        context.revokeUriPermission(uri(id),Intent.FLAG_GRANT_READ_URI_PERMISSION);
        content(m).delete();file(id,".rev-"+(m.optLong("revision")+1)+".bin").delete();
        for(String suffix:new String[]{".bin",".part",".json",".json.bak"})file(id,suffix).delete();
    }
    static synchronized JSONArray documents() throws Exception {
        errors.clear();
        JSONArray list = new JSONArray(); File[] files = new File(context.getFilesDir(), "documents").listFiles();
        if (files != null) for (File f : files) if (f.getName().endsWith(".json")) {
            String id = f.getName().substring(0, f.getName().length() - 5); JSONObject m;
            try{m=metadata(id);}catch(Exception corrupt){if(errors.size()<8)errors.add("A Mac Files document needs recovery. Its contents have been preserved.");continue;}
            if(m.has("error")&&errors.size()<8)errors.add(m.getString("error"));
            if ((m.optBoolean("ready")||m.optString("kind").equals("save"))&&!m.optString("kind").equals("clipboard")) { if(list.length()>=MAX_DOCUMENTS)throw new IOException("Document index exceeds limit"); list.put(m); }
        }
        return list;
    }
    static synchronized JSONArray exports() throws Exception {
        JSONArray out = new JSONArray(), docs = documents();
        for (int i = 0; i < docs.length() && out.length() < MAX_ITEMS; i++) {
            JSONObject m = docs.getJSONObject(i);
            if (m.optBoolean("export") && m.optInt("acked") < m.getInt("revision")) out.put(m);
        }
        return out;
    }
    static ParcelFileDescriptor open(String id, String mode, boolean control) throws Exception {
        JSONObject m;
        synchronized (Repository.class) {
            m = metadata(id);
            if (mode.equals("r")) {
                if (!m.getBoolean("ready")) throw new FileNotFoundException("File transfer is incomplete");
                return ParcelFileDescriptor.open(content(m), ParcelFileDescriptor.MODE_READ_ONLY);
            }
            if (!control && !m.getString("kind").equals("save")) throw new SecurityException("Imported files are read-only");
            if (!Arrays.asList("w", "wt", "wa", "rw", "rwt").contains(mode)) throw new IllegalArgumentException("Unsupported write mode");
            if (control && m.optBoolean("ready")) throw new IOException("An imported snapshot cannot be overwritten");
            if (!writers.add(id)) throw new IOException("This document already has a writer");
        }
        File part = file(id, ".part");
        try {
            // An interrupted writer may have left bytes in .part. They never
            // belong to the next open, including rw on a not-yet-saved document.
            try (FileOutputStream empty = new FileOutputStream(part)) { empty.getFD().sync(); }
            if ((mode.equals("rw")||mode.equals("wa")) && m.optBoolean("ready")) {
                space(m.getLong("size"));
                try (InputStream in = new FileInputStream(content(m)); OutputStream out = new FileOutputStream(part)) { copy(in, out); }
            }
            if (control) return ParcelFileDescriptor.open(part, ParcelFileDescriptor.MODE_CREATE | ParcelFileDescriptor.MODE_WRITE_ONLY | ParcelFileDescriptor.MODE_TRUNCATE, io, error -> { synchronized(Repository.class){writers.remove(id);} });
            final RandomAccessFile data = new RandomAccessFile(part, "rw");
            if (!mode.equals("rw")&&!mode.equals("wa")) data.setLength(0);
            return context.getSystemService(android.os.storage.StorageManager.class).openProxyFileDescriptor(mode.equals("wa")?ParcelFileDescriptor.MODE_WRITE_ONLY:ParcelFileDescriptor.parseMode(mode), new ProxyFileDescriptorCallback() {
                boolean failed;
                public long onGetSize() throws android.system.ErrnoException {try{return data.length();}catch(IOException e){throw failure(e);}}
                public int onRead(long offset,int size,byte[] bytes) throws android.system.ErrnoException {try{data.seek(offset);return Math.max(0,data.read(bytes,0,size));}catch(IOException e){throw failure(e);}}
                public int onWrite(long offset,int size,byte[] bytes) throws android.system.ErrnoException {
                    try {if(mode.equals("wa"))offset=data.length();if(offset<0||offset>MAX_BYTES-size)throw new IOException("File exceeds transfer limit");space(Math.max(0,offset+size-data.length()));data.seek(offset);data.write(bytes,0,size);return size;}
                    catch(IOException e){failed=true;throw failure(e);}
                }
                public void onFsync() throws android.system.ErrnoException {try{data.getFD().sync();}catch(IOException e){failed=true;throw failure(e);}}
                android.system.ErrnoException failure(IOException e){return new android.system.ErrnoException("document IO",android.system.OsConstants.EIO,e);}
                public void onRelease() {
                    try {
                        data.getFD().sync();data.close();
                        if(failed){part.delete();saveError(id);return;}
                        String digest=hash(part);
                        synchronized(Repository.class) {
                            JSONObject latest=metadata(id);
                            latest.put("size",part.length()).put("sha256",digest).put("export",true);
                            publish(latest,part);
                        }
                    } catch(Exception failure){saveError(id);android.util.Log.e("GlidePodFiles","Document export could not be finalized");}
                    finally {try{data.close();}catch(IOException ignored){} synchronized(Repository.class){writers.remove(id);}}
                }
            }, io);
        } catch (Exception e) { synchronized(Repository.class){writers.remove(id);} throw e; }
    }
    static long copy(InputStream in, OutputStream out) throws IOException {
        byte[] buffer = new byte[65536]; long total = 0; int n;
        while ((n = in.read(buffer)) != -1) { space(n); total += n; if (total > MAX_BYTES) throw new IOException("File exceeds transfer limit"); out.write(buffer, 0, n); }
        return total;
    }
    static synchronized JSONObject poll() throws Exception {
        clean();
        JSONArray pending = new JSONArray();
        for (JSONObject r : requests.values()) pending.put(r);
        return new JSONObject().put("requests", pending).put("exports", exports()).put("errors",new JSONArray(errors)).put("system", (context.getApplicationInfo().flags & android.content.pm.ApplicationInfo.FLAG_SYSTEM) != 0);
    }
}
