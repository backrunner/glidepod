package dev.glidepod.files;

import android.app.Activity;
import android.content.*;
import android.database.Cursor;
import android.net.Uri;
import android.os.*;
import android.provider.OpenableColumns;
import android.widget.TextView;
import java.io.*;
import java.util.*;
import org.json.JSONObject;

public final class SaveToMac extends Activity {
    @Override public void onCreate(Bundle state) {
        super.onCreate(state); Repository.init(this);
        TextView text = new TextView(this); text.setPadding(28,48,28,28); text.setText("Preparing files for your Mac…"); setContentView(text);
        ArrayList<Uri> uris = new ArrayList<>();
        if (Intent.ACTION_SEND.equals(getIntent().getAction())) { Uri u = getIntent().getParcelableExtra(Intent.EXTRA_STREAM); if (u != null) uris.add(u); }
        else if (Intent.ACTION_SEND_MULTIPLE.equals(getIntent().getAction())) { ArrayList<Uri> list = getIntent().getParcelableArrayListExtra(Intent.EXTRA_STREAM); if (list != null) uris.addAll(list); }
        if (uris.isEmpty() || uris.size() > Repository.MAX_ITEMS) { finish(); return; }
        try { Repository.shares.execute(() -> {
            try {
                for (Uri u : uris) {
                    if (!"content".equals(u.getScheme())) throw new IOException("A content URI is required");
                    String name = "Shared file", mime = getContentResolver().getType(u);
                    try (Cursor c = getContentResolver().query(u, new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) { if (c != null && c.moveToFirst()) name = c.getString(0); }
                    JSONObject m = Repository.create(name, mime, 0, "", "share"); String id = m.getString("id"); File part = Repository.file(id, ".part");
                    try (InputStream in = getContentResolver().openInputStream(u); FileOutputStream out = new FileOutputStream(part)) {
                        if (in == null) throw new IOException("Cannot read shared file"); Repository.copy(in, out); out.getFD().sync();
                    }
                    m.put("size",part.length()).put("sha256",Repository.hash(part)).put("export",true);
                    Repository.publish(m,part);
                }
                runOnUiThread(this::finish);
            } catch (Exception e) { runOnUiThread(() -> text.setText("Could not prepare this file. Return to the app and share it again.")); }
        }); } catch(java.util.concurrent.RejectedExecutionException e) { text.setText("Other files are still transferring. Try sharing again shortly."); }
    }
}
