package dev.glidepod.files;

import android.app.*;
import android.content.*;
import android.net.Uri;
import android.os.*;
import java.util.*;
import org.json.*;

/** Translates a host-authorized drop into the target app's standard Share contract. */
public final class Deliver extends Activity {
    @Override public void onCreate(Bundle saved) {
        super.onCreate(saved); Repository.init(this);
        try {
            JSONObject request;
            synchronized (Repository.class) { request = Repository.deliveries.remove(getIntent().getStringExtra("token")); }
            if (request == null || SystemClock.elapsedRealtime() - request.getLong("created") > 30000) throw new IllegalStateException("Delivery expired");
            if (getDisplay() == null || request.getInt("display") != getDisplay().getDisplayId()) throw new SecurityException("Display changed");
            JSONArray ids = request.getJSONArray("ids");
            if (ids.length() < 1 || ids.length() > Repository.MAX_ITEMS) throw new IllegalArgumentException("Invalid file count");
            ArrayList<Uri> uris = new ArrayList<>(); String mime = null;
            for (int i = 0; i < ids.length(); i++) {
                JSONObject m = Repository.metadata(ids.getString(i)); if (!m.getBoolean("ready")) throw new IllegalStateException("File is incomplete");
                String type = m.getString("mime"); mime = mime == null ? type : mime.equals(type) ? mime : "*/*"; uris.add(Repository.uri(ids.getString(i)));
            }
            Intent send = new Intent(uris.size() == 1 ? Intent.ACTION_SEND : Intent.ACTION_SEND_MULTIPLE).setPackage(request.getString("package"))
                .setType(mime).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
            if (uris.size() == 1) send.putExtra(Intent.EXTRA_STREAM, uris.get(0)); else send.putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris);
            ClipData clip = ClipData.newUri(getContentResolver(), "Mac files", uris.get(0));
            for (int i = 1; i < uris.size(); i++) clip.addItem(new ClipData.Item(uris.get(i))); send.setClipData(clip);
            Repository.retain(ids);
            startActivity(send);
        } catch (Exception e) { android.widget.Toast.makeText(this,"This app cannot receive these files. Use its file selection action.",android.widget.Toast.LENGTH_LONG).show(); }
        finish();
    }
}
