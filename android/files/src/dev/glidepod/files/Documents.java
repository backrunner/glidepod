package dev.glidepod.files;

import android.database.*;
import android.os.*;
import android.provider.*;
import java.io.*;
import org.json.*;

public final class Documents extends DocumentsProvider {
    static final String[] COLUMNS = {"document_id", "_display_name", "mime_type", "_size", "flags", "last_modified"};
    @Override public boolean onCreate() { Repository.init(getContext()); return true; }
    @Override public Cursor queryRoots(String[] projection) {
        String[] cols = projection == null ? new String[]{"root_id", "document_id", "title", "flags", "mime_types"} : projection;
        MatrixCursor c = new MatrixCursor(cols); MatrixCursor.RowBuilder row = c.newRow();
        for (String col : cols) row.add(col, col.equals("root_id") || col.equals("document_id") ? "mac" : col.equals("title") ? "Mac Files" : col.equals("flags") ? 0 : col.equals("mime_types") ? "*/*" : null);
        return c;
    }
    void row(MatrixCursor c, JSONObject m) throws Exception {
        MatrixCursor.RowBuilder row = c.newRow();
        for (String col : c.getColumnNames()) {
            Object value = null;
            if (col.equals("document_id")) value = m.getString("id");
            if (col.equals("_display_name")) value = m.getString("name");
            if (col.equals("mime_type")) value = m.getString("mime");
            if (col.equals("_size")) value = m.getLong("size");
            if (col.equals("last_modified")) value = m.optLong("modified",m.getLong("created"));
            if (col.equals("flags")) value = (m.getString("kind").equals("save") ? DocumentsContract.Document.FLAG_SUPPORTS_WRITE : 0) | (!m.getString("kind").equals("root")&&(!m.optBoolean("export")||m.optLong("acked")>=m.optLong("revision")) ? DocumentsContract.Document.FLAG_SUPPORTS_DELETE : 0);
            row.add(col, value);
        }
    }
    @Override public Cursor queryDocument(String id, String[] projection) throws FileNotFoundException {
        MatrixCursor c = new MatrixCursor(projection == null ? COLUMNS : projection);
        try {
            JSONObject m = id.equals("mac") ? new JSONObject().put("id","mac").put("name","Mac Files").put("mime",DocumentsContract.Document.MIME_TYPE_DIR).put("size",0).put("created",0).put("kind","root") : Repository.metadata(id);
            row(c, m); return c;
        } catch (Exception e) { throw new FileNotFoundException(e.getMessage()); }
    }
    @Override public Cursor queryChildDocuments(String parent, String[] projection, String sortOrder) throws FileNotFoundException {
        MatrixCursor c = new MatrixCursor(projection == null ? COLUMNS : projection);
        try {
            if (!parent.equals("mac")) throw new FileNotFoundException("Unknown directory");
            JSONArray docs = Repository.documents(); for (int i = 0; i < docs.length(); i++) row(c, docs.getJSONObject(i)); return c;
        } catch (Exception e) { throw new FileNotFoundException(e.getMessage()); }
    }
    @Override public ParcelFileDescriptor openDocument(String id, String mode, CancellationSignal signal) throws FileNotFoundException {
        try { if (signal != null) signal.throwIfCanceled(); return Repository.open(id, mode, false); }
        catch (Exception e) { throw new FileNotFoundException(e.getMessage()); }
    }
    @Override public void deleteDocument(String id) throws FileNotFoundException {
        try {synchronized(Repository.class){
            JSONObject m=Repository.metadata(id);
            Repository.discard(id,true);
        }}catch(Exception e){throw new FileNotFoundException(e.getMessage());}
    }
}
