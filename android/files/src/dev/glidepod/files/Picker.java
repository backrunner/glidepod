package dev.glidepod.files;

import android.app.Activity;
import android.content.*;
import android.net.Uri;
import android.os.*;
import android.provider.MediaStore;
import android.widget.*;
import java.util.*;
import org.json.*;

/** Preserves the original Android activity-result contract while the host chooses content. */
public final class Picker extends Activity {
    private String request;
    private String action;
    private String[] types;
    private int maximum;
    private boolean finished;
    @Override public void onCreate(Bundle saved) {
        super.onCreate(saved); Repository.init(this); setResult(RESULT_CANCELED);
        try {
            String caller = getCallingPackage();
            if (caller == null || getDisplay() == null || getDisplay().getDisplayId() == 0) {
                Intent original=new Intent(getIntent()).setComponent(null).setPackage(null);
                for(android.content.pm.ResolveInfo info:getPackageManager().queryIntentActivities(original,android.content.pm.PackageManager.MATCH_SYSTEM_ONLY|android.content.pm.PackageManager.MATCH_DEFAULT_ONLY)) {
                    if(!info.activityInfo.packageName.equals(getPackageName())) {original.setComponent(new ComponentName(info.activityInfo.packageName,info.activityInfo.name));startActivityForResult(original,7);return;}
                }
                throw new IllegalStateException("No Android file picker is available");
            }
            action = getIntent().getAction();
            boolean photo = MediaStore.ACTION_PICK_IMAGES.equals(action);
            if (!Arrays.asList(Intent.ACTION_GET_CONTENT, Intent.ACTION_OPEN_DOCUMENT, Intent.ACTION_CREATE_DOCUMENT, Intent.ACTION_PICK, MediaStore.ACTION_PICK_IMAGES).contains(action)) throw new IllegalArgumentException("Unsupported file request");
            String type = getIntent().getType();
            types = getIntent().getStringArrayExtra(Intent.EXTRA_MIME_TYPES);
            if (types == null || types.length == 0) types = photo && type == null ? new String[]{"image/*", "video/*"} : new String[]{type == null ? "*/*" : type};
            if (types.length > 32) throw new IllegalArgumentException("Too many content types");
            for (int i=0;i<types.length;i++) types[i]=Repository.mime(types[i]);
            maximum = photo ? getIntent().getIntExtra(MediaStore.EXTRA_PICK_IMAGES_MAX, 1) : getIntent().getBooleanExtra(Intent.EXTRA_ALLOW_MULTIPLE, false) ? Repository.MAX_ITEMS : 1;
            if (maximum < 1) throw new IllegalArgumentException("Invalid selection limit");
            // AndroidX commonly requests the platform maximum (e.g. 150). A
            // smaller valid selection is allowed; do not cancel the whole picker.
            maximum=Math.min(maximum,Repository.MAX_ITEMS);
            if (Intent.ACTION_CREATE_DOCUMENT.equals(action)) maximum = 1;
            request = Repository.id();
            JSONObject descriptor = new JSONObject().put("id", request).put("package", caller).put("display", getDisplay().getDisplayId()).put("task", getTaskId())
                .put("kind", Intent.ACTION_CREATE_DOCUMENT.equals(action) ? "save" : "open").put("mime_types", new JSONArray(Arrays.asList(types))).put("maximum", maximum)
                .put("name", Repository.name(getIntent().getStringExtra(Intent.EXTRA_TITLE)));
            synchronized (Repository.class) {
                if (Repository.requests.size() >= 16) throw new IllegalStateException("Too many file selections");
                Repository.requests.put(request, descriptor); Repository.pickers.put(request, this);
            }
            LinearLayout layout = new LinearLayout(this); layout.setOrientation(LinearLayout.VERTICAL); layout.setPadding(28, 48, 28, 28);
            TextView label = new TextView(this); label.setText("Choose a file on your Mac"); label.setTextSize(20); layout.addView(label);
            Button cancel = new Button(this); cancel.setText("Cancel"); cancel.setOnClickListener(v -> finish()); layout.addView(cancel); setContentView(layout);
            // The owning task/window cancels this request on teardown. A fixed
            // five-minute timer would discard a valid large-file transfer or a
            // native panel the user deliberately left open.
        } catch (Exception e) { finish(); }
    }
    private boolean accepts(String mime) {
        for (String t : types) if (t.equals("*/*") || t.equals(mime) || t.endsWith("/*") && mime.startsWith(t.substring(0, t.length() - 1))) return true;
        return false;
    }
    void complete(JSONObject result) {
        if (finished || isFinishing()) return;
        try {
            if (result.optBoolean("cancel")) { finish(); return; }
            JSONArray ids = result.optJSONArray("ids");
            if (Intent.ACTION_CREATE_DOCUMENT.equals(action)) {
                JSONObject m = Repository.create(result.optString("name",getIntent().getStringExtra(Intent.EXTRA_TITLE)), types[0], 0, "", "save");
                ids = new JSONArray().put(m.getString("id"));
                // Bind the host destination to the request, never to a guest supplied pathname.
                m.put("request", request); Repository.record(m.getString("id"), m);
            }
            if (ids == null || ids.length() < 1 || ids.length() > maximum) throw new IllegalArgumentException("Invalid selection count");
            ArrayList<Uri> uris = new ArrayList<>();
            for (int i = 0; i < ids.length(); i++) {
                JSONObject m = Repository.metadata(ids.getString(i));
                if (!Intent.ACTION_CREATE_DOCUMENT.equals(action) && (!m.getBoolean("ready") || !accepts(m.getString("mime")))) throw new IllegalArgumentException("Selected file has an unsupported type");
                uris.add(Repository.uri(ids.getString(i)));
            }
            if(!Intent.ACTION_CREATE_DOCUMENT.equals(action))Repository.retain(ids);
            Intent data = new Intent().setData(uris.get(0)).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
            ClipData clip = ClipData.newUri(getContentResolver(), "Mac files", uris.get(0));
            for (int i = 1; i < uris.size(); i++) clip.addItem(new ClipData.Item(uris.get(i)));
            data.setClipData(clip);
            if (Intent.ACTION_CREATE_DOCUMENT.equals(action)) data.addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
            if (Intent.ACTION_OPEN_DOCUMENT.equals(action) || Intent.ACTION_CREATE_DOCUMENT.equals(action)) data.addFlags(Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
            setResult(RESULT_OK, data); finished = true; finish();
        } catch (Exception e) { finish(); }
    }
    @Override protected void onDestroy() {
        synchronized (Repository.class) { if (request != null) { Repository.requests.remove(request); Repository.pickers.remove(request); } }
        super.onDestroy();
    }
    @Override protected void onActivityResult(int requestCode,int resultCode,Intent data){super.onActivityResult(requestCode,resultCode,data);if(requestCode==7){setResult(resultCode,data);finish();}}
}
