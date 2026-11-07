package dev.glidepod.filefixture;
import android.content.*;import android.database.*;import android.net.Uri;import android.os.*;import android.provider.MediaStore;import java.io.*;import org.json.*;
public final class Control extends ContentProvider {
    public boolean onCreate(){return true;}
    public Bundle call(String method,String arg,Bundle extras){getContext().enforceCallingPermission("android.permission.DUMP","fixture control");try{
        JSONObject a=arg==null?new JSONObject():new JSONObject(arg),out=new JSONObject();
        if(method.equals("choose")){Main.current.runOnUiThread(()->{try{Main.current.choose(a);}catch(Exception e){throw new RuntimeException(e);}});}
        else if(method.equals("result"))out=Main.result;
        else if(method.equals("write")){try(OutputStream stream=getContext().getContentResolver().openOutputStream(Main.saved,"wt")){stream.write(a.getString("text").getBytes("UTF-8"));}}
        else if(method.equals("write_at")){
            try(ParcelFileDescriptor fd=getContext().getContentResolver().openFileDescriptor(Main.saved,a.optString("mode","rw"))){
                byte[] bytes=a.getString("text").getBytes("UTF-8");
                android.system.Os.pwrite(fd.getFileDescriptor(),bytes,0,bytes.length,a.optLong("offset"));
                android.system.Os.fsync(fd.getFileDescriptor());
            }
        }
        else if(method.equals("read_saved")){String saved=getContext().getSharedPreferences("Main",0).getString("saved","");out.put("sha256",Main.hash(getContext().getContentResolver().openInputStream(Uri.parse(saved))));}
        else if(method.equals("download")){
            ContentValues values=new ContentValues();values.put(MediaStore.Downloads.DISPLAY_NAME,a.getString("name"));values.put(MediaStore.Downloads.MIME_TYPE,"text/plain");values.put(MediaStore.Downloads.IS_PENDING,1);
            Uri uri=getContext().getContentResolver().insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI,values);
            try(OutputStream stream=getContext().getContentResolver().openOutputStream(uri)){stream.write(a.getString("text").getBytes("UTF-8"));}out.put("uri",uri.toString());
        }else if(method.equals("publish")){ContentValues values=new ContentValues();values.put(MediaStore.Downloads.IS_PENDING,0);getContext().getContentResolver().update(Uri.parse(a.getString("uri")),values,null,null);}
        else if(method.equals("download_manager")){
            android.app.DownloadManager.Request r=new android.app.DownloadManager.Request(Uri.parse("https://example.com/"));
            String name="glidepod-dm-"+System.currentTimeMillis()+".html";
            if(a.optBoolean("private"))r.setDestinationInExternalFilesDir(getContext(),Environment.DIRECTORY_DOWNLOADS,name);else r.setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS,name);
            long id=getContext().getSystemService(android.app.DownloadManager.class).enqueue(r);out.put("id",id);
        }else if(method.equals("download_status")){
            android.app.DownloadManager manager=getContext().getSystemService(android.app.DownloadManager.class);long id=a.getLong("id");
            try(Cursor c=manager.query(new android.app.DownloadManager.Query().setFilterById(id))){if(c!=null&&c.moveToFirst()){int status=c.getInt(c.getColumnIndexOrThrow(android.app.DownloadManager.COLUMN_STATUS));out.put("status",status);out.put("reason",c.getInt(c.getColumnIndexOrThrow(android.app.DownloadManager.COLUMN_REASON)));if(status==8)out.put("sha256",Main.hash(new ParcelFileDescriptor.AutoCloseInputStream(manager.openDownloadedFile(id))));}}
        }
        else if(method.equals("clipboard")){Main.current.runOnUiThread(()->getContext().getSystemService(ClipboardManager.class).setPrimaryClip(ClipData.newPlainText("fixture",a.optString("text"))));}
        else if(method.equals("share")){Main.current.runOnUiThread(()->{Uri u=Uri.parse("content://dev.glidepod.filefixture.shared/attachment");Intent send=new Intent(Intent.ACTION_SEND).setComponent(new ComponentName("dev.glidepod.files","dev.glidepod.files.SaveToMac")).setType("text/plain").putExtra(Intent.EXTRA_STREAM,u).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);send.setClipData(ClipData.newUri(getContext().getContentResolver(),"private fixture",u));Main.current.startActivity(send);});}
        else if(method.equals("editor")){out.put("text",Main.editor.getText().toString());}
        else if(method.equals("private")||method.equals("private_read")){File f=new File(getContext().getFilesDir(),"sentinel.txt");if(method.equals("private")){try(OutputStream s=new FileOutputStream(f)){s.write("Private app data stays private".getBytes("UTF-8"));}}out.put("sha256",Main.hash(new FileInputStream(f)));}
        else if(method.equals("security")){boolean denied=false;try{getContext().getContentResolver().call(Uri.parse("content://dev.glidepod.files.bridge"),"poll",null,null);}catch(SecurityException e){denied=true;}out.put("denied",denied);}
        else throw new IllegalArgumentException("Unknown fixture command");
        Bundle b=new Bundle();b.putString("json",out.toString());return b;
    }catch(Exception e){throw new IllegalStateException(e);}}
    public Cursor query(Uri u,String[] p,String s,String[] a,String o){throw new UnsupportedOperationException();}public String getType(Uri u){return "text/plain";}public Uri insert(Uri u,ContentValues v){throw new UnsupportedOperationException();}public int delete(Uri u,String s,String[] a){throw new UnsupportedOperationException();}public int update(Uri u,ContentValues v,String s,String[] a){throw new UnsupportedOperationException();}
}
