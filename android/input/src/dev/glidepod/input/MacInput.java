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
                if (selected == null || selected.length() > 16384) return;
                setResultData(Base64.encodeToString(selected.toString().getBytes(StandardCharsets.UTF_8),Base64.NO_WRAP));
                if ("cut".equals(action)) connection.commitText("",1);
                setResultCode(1); return;
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
