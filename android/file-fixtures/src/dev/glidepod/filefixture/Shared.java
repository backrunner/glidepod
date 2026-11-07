package dev.glidepod.filefixture;
import android.content.*;import android.database.*;import android.net.Uri;import android.os.*;import java.io.*;
/** Fixture-only private attachment, accessible to another app solely by URI grant. */
public final class Shared extends ContentProvider {
    public boolean onCreate(){return true;}
    public String getType(Uri u){return "text/plain";}
    public Cursor query(Uri u,String[] p,String s,String[] a,String o){MatrixCursor c=new MatrixCursor(p==null?new String[]{"_display_name","_size"}:p);MatrixCursor.RowBuilder row=c.newRow();for(String col:c.getColumnNames())row.add(col,col.equals("_display_name")?"private-attachment.txt":col.equals("_size")?new File(getContext().getFilesDir(),"sentinel.txt").length():null);return c;}
    public ParcelFileDescriptor openFile(Uri u,String mode)throws FileNotFoundException{if(!u.getPath().equals("/attachment")||!mode.equals("r"))throw new FileNotFoundException();return ParcelFileDescriptor.open(new File(getContext().getFilesDir(),"sentinel.txt"),ParcelFileDescriptor.MODE_READ_ONLY);}
    public Uri insert(Uri u,ContentValues v){throw new UnsupportedOperationException();}public int delete(Uri u,String s,String[] a){throw new UnsupportedOperationException();}public int update(Uri u,ContentValues v,String s,String[] a){throw new UnsupportedOperationException();}
}
