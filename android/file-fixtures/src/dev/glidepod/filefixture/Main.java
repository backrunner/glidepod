package dev.glidepod.filefixture;
import android.app.*;import android.content.*;import android.net.Uri;import android.os.*;import android.widget.*;import java.io.*;import org.json.*;
public final class Main extends Activity {
    static Main current;static volatile JSONObject result=new JSONObject();static Uri saved;static EditText editor;
    @Override public void onCreate(Bundle state){super.onCreate(state);current=this;editor=new EditText(this){
        @Override public android.view.inputmethod.InputConnection onCreateInputConnection(android.view.inputmethod.EditorInfo info){
            android.view.inputmethod.InputConnection base=super.onCreateInputConnection(info);info.contentMimeTypes=new String[]{"image/png"};
            return new android.view.inputmethod.InputConnectionWrapper(base,false){
                @Override public boolean commitContent(android.view.inputmethod.InputContentInfo content,int flags,Bundle options){
                    try{if((flags&android.view.inputmethod.InputConnection.INPUT_CONTENT_GRANT_READ_URI_PERMISSION)!=0)content.requestPermission();result=new JSONObject().put("rich_hash",hash(getContentResolver().openInputStream(content.getContentUri())));return true;}
                    catch(Exception e){return false;}finally{content.releasePermission();}
                }
            };
        }
    };editor.setText("Clipboard fixture 中文");setContentView(editor);editor.requestFocus();if(Intent.ACTION_SEND.equals(getIntent().getAction()))receive(getIntent());}
    @Override protected void onNewIntent(Intent intent){super.onNewIntent(intent);if(Intent.ACTION_SEND.equals(intent.getAction())||Intent.ACTION_SEND_MULTIPLE.equals(intent.getAction()))receive(intent);}
    static String hash(InputStream in)throws Exception{java.security.MessageDigest h=java.security.MessageDigest.getInstance("SHA-256");byte[] b=new byte[65536];int n;try(InputStream src=in){while((n=src.read(b))!=-1)h.update(b,0,n);}StringBuilder s=new StringBuilder();for(byte v:h.digest())s.append(String.format("%02x",v&255));return s.toString();}
    void choose(JSONObject a)throws Exception{
        result=new JSONObject().put("waiting",true);String action=a.getString("action");Intent intent=new Intent(action).setType(a.optString("mime","text/plain"));
        if(!action.equals("android.provider.action.PICK_IMAGES")&&!action.equals(Intent.ACTION_PICK))intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE,a.optBoolean("multiple"));intent.putExtra(Intent.EXTRA_TITLE,a.optString("name","fixture.txt"));
        if(a.has("maximum"))intent.putExtra("android.provider.extra.PICK_IMAGES_MAX",a.getInt("maximum"));
        startActivityForResult(intent,1);
    }
    void receive(Intent data){onActivityResult(1,RESULT_OK,data);}
    @Override protected void onActivityResult(int request,int code,Intent data){super.onActivityResult(request,code,data);try{
        JSONObject out=new JSONObject().put("code",code);JSONArray items=new JSONArray();
        if(code==RESULT_OK&&data!=null){java.util.ArrayList<Uri> uris=new java.util.ArrayList<>();if(data.getClipData()!=null){for(int i=0;i<data.getClipData().getItemCount();i++)uris.add(data.getClipData().getItemAt(i).getUri());}else if(data.getData()!=null)uris.add(data.getData());else{Uri u=data.getParcelableExtra(Intent.EXTRA_STREAM);if(u!=null)uris.add(u);}
            for(Uri u:uris){JSONObject item=new JSONObject().put("uri",u.toString());saved=u;
                try(android.database.Cursor c=getContentResolver().query(u,new String[]{android.provider.OpenableColumns.DISPLAY_NAME},null,null,null)){if(c!=null&&c.moveToFirst())item.put("name",c.getString(0));}
                if((data.getFlags()&Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)!=0){getContentResolver().takePersistableUriPermission(u,data.getFlags()&(Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_WRITE_URI_PERMISSION));getPreferences(0).edit().putString("saved",u.toString()).commit();item.put("persistent",true);}
                try{item.put("sha256",hash(getContentResolver().openInputStream(u)));}catch(Exception e){item.put("unwritten",true);}items.put(item);
            }
        }result=out.put("items",items);
    }catch(Exception e){try{result=new JSONObject().put("error",e.toString());}catch(Exception ignored){}}}
    @Override protected void onDestroy(){if(current==this)current=null;super.onDestroy();}
}
