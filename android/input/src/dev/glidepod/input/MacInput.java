package dev.glidepod.input;
import android.content.*;
import android.inputmethodservice.InputMethodService;
import android.util.Base64;
import android.view.inputmethod.*;
import java.nio.charset.StandardCharsets;

/** Only shell/root senders holding DUMP may commit text, and only to the named focused app. */
public final class MacInput extends InputMethodService {
    private final BroadcastReceiver receiver = new BroadcastReceiver() {
        @Override public void onReceive(Context context, Intent intent) {
            setResultCode(0);
            EditorInfo editor = getCurrentInputEditorInfo();
            InputConnection connection = getCurrentInputConnection();
            String expected = intent.getStringExtra("package");
            String encoded = intent.getStringExtra("text");
            if (editor == null || connection == null || expected == null || !expected.equals(editor.packageName)) return;
            String action = intent.getStringExtra("action");
            if ("copy".equals(action) || "cut".equals(action)) {
                CharSequence selected = connection.getSelectedText(0);
                if (!connection.performContextMenuAction("copy".equals(action) ? android.R.id.copy : android.R.id.cut)) return;
                // The legacy return is a selection snapshot, never a pasteboard write.
                // ClipboardBridge observes the app's completed system Copy/Cut action.
                if (selected == null || selected.length() > 16384) selected="";
                setResultData(Base64.encodeToString(selected.toString().getBytes(StandardCharsets.UTF_8),Base64.NO_WRAP));
                setResultCode(1); return;
            }
            if ("paste".equals(action)) {
                ClipData clip=getSystemService(ClipboardManager.class).getPrimaryClip();
                if(clip!=null&&clip.getItemCount()==1&&clip.getItemAt(0).getUri()!=null&&editor.contentMimeTypes!=null) {
                    for(String accepted:editor.contentMimeTypes)for(int i=0;i<clip.getDescription().getMimeTypeCount();i++) {
                        if(ClipDescription.compareMimeTypes(clip.getDescription().getMimeType(i),accepted)&&connection.commitContent(new InputContentInfo(clip.getItemAt(0).getUri(),clip.getDescription(),null),InputConnection.INPUT_CONTENT_GRANT_READ_URI_PERMISSION,null)){setResultCode(1);return;}
                    }
                }
                if (connection.performContextMenuAction(android.R.id.paste)) setResultCode(1); return;
            }
            if ("select_all".equals(action)) { if (connection.performContextMenuAction(android.R.id.selectAll)) setResultCode(1); return; }
            if (encoded == null || encoded.length() > 32768) return;
            try {
                String text = new String(Base64.decode(encoded, Base64.NO_WRAP),StandardCharsets.UTF_8);
                if (connection.commitText(text, 1)) setResultCode(1);
            } catch (IllegalArgumentException ignored) { }
        }
    };
    @Override public void onCreate() {
        super.onCreate();
        registerReceiver(receiver,new IntentFilter("dev.glidepod.input.COMMIT"),"android.permission.DUMP",null,Context.RECEIVER_EXPORTED);
    }
    @Override public void onDestroy() { unregisterReceiver(receiver); super.onDestroy(); }
    @Override public boolean onEvaluateInputViewShown() { return false; }
}
