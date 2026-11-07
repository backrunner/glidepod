package dev.glidepod.input;

import android.content.*;
import android.database.*;
import android.net.Uri;
import android.os.*;
import android.provider.OpenableColumns;
import java.io.*;
import org.json.*;

/** Default-IME clipboard access, exposed only to the private shell transport. */
public final class ClipboardBridge extends ContentProvider {
    private ClipboardManager clipboard;
    private long revision;
    private String session;
    @Override public boolean onCreate() {
        clipboard=getContext().getSystemService(ClipboardManager.class);session=java.util.UUID.randomUUID().toString();revision=1;
        clipboard.addPrimaryClipChangedListener(()->{synchronized(this){revision++;}});return true;
    }
    private void authorize(){getContext().enforceCallingPermission("android.permission.DUMP","GlidePod clipboard transport");}
    private String version(){
        ClipDescription description=clipboard.getPrimaryClipDescription();
        return session+":"+revision+":"+(description==null?"empty":Long.toString(description.getTimestamp()));
    }
    @Override public synchronized Bundle call(String method,String arg,Bundle extras) {
        authorize();
        long identity=Binder.clearCallingIdentity();
        try {
            String ime=android.provider.Settings.Secure.getString(getContext().getContentResolver(),android.provider.Settings.Secure.DEFAULT_INPUT_METHOD);
            if(ime==null||!ime.startsWith(getContext().getPackageName()+"/"))throw new IOException("GlidePod input method is not active; reopen the app window");
            JSONObject out;
            if(method.equals("read"))out=read();
            else if(method.equals("set")){
                if(arg==null||arg.length()>524288)throw new IllegalArgumentException("Clipboard payload exceeds limit");
                JSONObject value=new JSONObject(arg);String kind=value.getString("kind");ClipData clip;
                if(value.has("expected")&&!version().equals(value.getString("expected")))throw new IOException("Clipboard changed before transfer completed");
                if(kind.equals("empty")){clipboard.clearPrimaryClip();out=new JSONObject().put("version",version());}
                else {
                    if(kind.equals("text")){String text=value.getString("text");if(text.getBytes(java.nio.charset.StandardCharsets.UTF_8).length>262144)throw new IllegalArgumentException("Clipboard text exceeds limit");clip=ClipData.newPlainText("Mac",text);}
                    else {
                        JSONArray uris=value.getJSONArray("uris");if(uris.length()<1||uris.length()>64)throw new IllegalArgumentException("Invalid clipboard file count");
                        Uri first=allowed(uris.getString(0));clip=ClipData.newUri(getContext().getContentResolver(),"Mac files",first);
                        for(int i=1;i<uris.length();i++)clip.addItem(new ClipData.Item(allowed(uris.getString(i))));
                    }
                    PersistableBundle flags=new PersistableBundle();flags.putString("dev.glidepod.origin",value.getString("origin"));
                    flags.putBoolean("android.content.extra.IS_SENSITIVE",value.optBoolean("sensitive"));clip.getDescription().setExtras(flags);
                    clipboard.setPrimaryClip(clip);out=new JSONObject().put("version",version());
                }
            }else throw new IllegalArgumentException("Unknown clipboard operation");
            Bundle b=new Bundle();b.putString("json",out.toString());return b;
        }catch(Exception e){throw new IllegalStateException(e.getMessage(),e);}finally{Binder.restoreCallingIdentity(identity);}
    }
    private Uri allowed(String value){Uri u=Uri.parse(value);if(!"content".equals(u.getScheme())||!"dev.glidepod.files.documents".equals(u.getAuthority()))throw new SecurityException("Unknown clipboard provider");return u;}
    private JSONObject read() throws Exception {
        ClipData clip=clipboard.getPrimaryClip();JSONObject out=new JSONObject().put("version",version()).put("kind","empty").put("origin","");
        if(clip==null||clip.getItemCount()==0)return out;
        PersistableBundle extras=clip.getDescription().getExtras();
        if(extras!=null)out.put("origin",extras.getString("dev.glidepod.origin","")).put("sensitive",extras.getBoolean("android.content.extra.IS_SENSITIVE"));
        ClipData.Item first=clip.getItemAt(0);
        if(first.getUri()==null&&first.getText()!=null){String text=first.getText().toString();if(text.getBytes(java.nio.charset.StandardCharsets.UTF_8).length>262144)throw new IOException("Clipboard text exceeds limit");return out.put("kind","text").put("text",text);}
        if(clip.getItemCount()>64)throw new IOException("Clipboard file count exceeds limit");
        JSONArray items=new JSONArray();
        for(int i=0;i<clip.getItemCount();i++){
            Uri u=clip.getItemAt(i).getUri();if(u==null||!"content".equals(u.getScheme()))throw new IOException("Unsupported clipboard item");
            String mime,name="Clipboard file";
            try {
                mime=getContext().getContentResolver().getType(u);
                try(Cursor c=getContext().getContentResolver().query(u,new String[]{OpenableColumns.DISPLAY_NAME},null,null,null)){if(c!=null&&c.moveToFirst())name=c.getString(0);}
            }catch(SecurityException|IllegalArgumentException revoked){return out.put("kind","unavailable");}
            items.put(new JSONObject().put("name",name).put("mime",mime==null?"application/octet-stream":mime));
        }
        return out.put("kind",items.length()==1&&items.getJSONObject(0).getString("mime").startsWith("image/")?"image":"files").put("items",items);
    }
    @Override public synchronized ParcelFileDescriptor openFile(Uri u,String mode)throws FileNotFoundException{
        authorize();
        long identity=Binder.clearCallingIdentity();
        try{
            if(!mode.equals("r")||!version().equals(u.getQueryParameter("version")))throw new IOException("Clipboard changed");
            int index=Integer.parseInt(u.getLastPathSegment());ClipData clip=clipboard.getPrimaryClip();
            if(clip==null||index<0||index>=clip.getItemCount())throw new IOException("Clipboard changed");
            Uri source=clip.getItemAt(index).getUri();if(source==null||!"content".equals(source.getScheme()))throw new IOException("Unsupported clipboard item");
            return getContext().getContentResolver().openFileDescriptor(source,"r");
        }catch(Exception e){throw new FileNotFoundException(e.getMessage());}finally{Binder.restoreCallingIdentity(identity);}
    }
    @Override public Cursor query(Uri u,String[] p,String s,String[] a,String o){throw new UnsupportedOperationException();}
    @Override public String getType(Uri u){return "application/octet-stream";}
    @Override public Uri insert(Uri u,ContentValues v){throw new UnsupportedOperationException();}
    @Override public int delete(Uri u,String s,String[] a){throw new UnsupportedOperationException();}
    @Override public int update(Uri u,ContentValues v,String s,String[] a){throw new UnsupportedOperationException();}
}
