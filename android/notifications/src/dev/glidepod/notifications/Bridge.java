package dev.glidepod.notifications;
import android.content.*;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import org.json.*;

/** No content is persisted. The call method explicitly checks its caller's permission. */
public final class Bridge extends ContentProvider {
    @Override public boolean onCreate(){return true;}
    @Override public Bundle call(String method,String arg,Bundle extras) {
        getContext().enforceCallingPermission("android.permission.DUMP","GlidePod private notification bridge");
        try {
            Listener l=Listener.current;
            JSONObject out=new JSONObject().put("connected",l!=null);
            if(method.equals("snapshot"))out.put("items",l==null?new JSONArray():l.snapshot());
            else if(method.equals("dismiss")){if(l==null)throw new IllegalStateException("Notification listener is disconnected");l.dismiss(arg);}
            else throw new IllegalArgumentException("Unknown notification operation");
            Bundle b=new Bundle();b.putString("json",out.toString());return b;
        }catch(Exception e){throw new IllegalStateException(e);}
    }
    @Override public Cursor query(Uri u,String[] p,String s,String[] a,String o){throw new UnsupportedOperationException();}
    @Override public String getType(Uri u){return null;}
    @Override public Uri insert(Uri u,ContentValues v){throw new UnsupportedOperationException();}
    @Override public int delete(Uri u,String s,String[] a){throw new UnsupportedOperationException();}
    @Override public int update(Uri u,ContentValues v,String s,String[] a){throw new UnsupportedOperationException();}
}
