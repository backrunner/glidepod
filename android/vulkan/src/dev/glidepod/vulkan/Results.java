package dev.glidepod.vulkan;
import android.content.*;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
public final class Results extends ContentProvider {
    static volatile String json="{\"ready\":false}";
    @Override public boolean onCreate(){return true;}
    @Override public Bundle call(String method,String arg,Bundle extras){
        getContext().enforceCallingPermission("android.permission.DUMP","Private Vulkan results");
        if(!method.equals("result"))throw new IllegalArgumentException("Unknown operation");
        Bundle b=new Bundle();b.putString("json",json);return b;
    }
    @Override public Cursor query(Uri u,String[] p,String s,String[] a,String o){throw new UnsupportedOperationException();}
    @Override public String getType(Uri u){return null;}
    @Override public Uri insert(Uri u,ContentValues v){throw new UnsupportedOperationException();}
    @Override public int delete(Uri u,String s,String[] a){throw new UnsupportedOperationException();}
    @Override public int update(Uri u,ContentValues v,String s,String[] a){throw new UnsupportedOperationException();}
}
