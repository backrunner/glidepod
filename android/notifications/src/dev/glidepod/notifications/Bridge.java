package dev.glidepod.notifications;
import android.content.*;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import org.json.*;

/** No content is persisted. The call method explicitly checks its caller's permission. */
public final class Bridge extends ContentProvider {
    private static final java.util.LinkedHashMap<String,PendingOpen> opens=new java.util.LinkedHashMap<>();
    private static final class PendingOpen {String argument;String state="pending";String error="";long created=android.os.SystemClock.elapsedRealtime();PendingOpen(String a){argument=a;}}
    private static synchronized String prepareOpen(String argument){
        if(argument==null||argument.length()>16384)throw new IllegalArgumentException("Invalid notification action");
        opens.entrySet().removeIf(e->android.os.SystemClock.elapsedRealtime()-e.getValue().created>30000);
        while(opens.size()>=32)opens.remove(opens.keySet().iterator().next());
        String token=java.util.UUID.randomUUID().toString();opens.put(token,new PendingOpen(argument));return token;
    }
    static synchronized void completeOpen(String token,android.app.Activity activity){
        PendingOpen p=opens.get(token);if(p==null||!p.state.equals("pending"))return;
        try{if(android.os.SystemClock.elapsedRealtime()-p.created>30000)throw new IllegalStateException("Notification click expired");
            Listener l=Listener.current;if(l==null)throw new IllegalStateException("Notification access disconnected");
            l.open(p.argument,activity);p.state="done";
        }catch(Exception e){p.state="failed";p.error=e.getMessage();}
    }
    private static synchronized JSONObject openStatus(String token) throws Exception {
        PendingOpen p=opens.get(token);if(p==null)throw new IllegalStateException("Notification action expired");
        JSONObject result=new JSONObject().put("state",p.state).put("error",p.error);
        if(!p.state.equals("pending"))opens.remove(token);return result;
    }
    @Override public boolean onCreate(){return true;}
    @Override public Bundle call(String method,String arg,Bundle extras) {
        getContext().enforceCallingPermission("android.permission.DUMP","GlidePod private notification bridge");
        try {
            Listener l=Listener.current;
            JSONObject out=new JSONObject().put("connected",l!=null);
            if(method.equals("snapshot")){out=l==null?new JSONObject().put("items",new JSONArray()).put("total",0).put("revision",0).put("consistent",true):l.snapshot(arg==null?0:Integer.parseInt(arg));out.put("connected",l!=null);}
            else if(method.equals("open")){if(l==null)throw new IllegalStateException("Notification listener is disconnected");out.put("token",prepareOpen(arg));}
            else if(method.equals("open_status")){out=openStatus(arg);}
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
