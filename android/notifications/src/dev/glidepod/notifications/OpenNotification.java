package dev.glidepod.notifications;
import android.app.Activity;
import android.os.Bundle;

/** Launched only by the DUMP-authorized host after an actual macOS notification click. */
public final class OpenNotification extends Activity {
    private boolean sent;
    @Override public void onCreate(Bundle state){super.onCreate(state);android.widget.TextView view=new android.widget.TextView(this);view.setText("Opening notification…");view.setPadding(24,48,24,24);setContentView(view);}
    @Override public void onPostResume(){super.onPostResume();if(sent)return;sent=true;
        getWindow().getDecorView().postDelayed(()->{
            Bridge.completeOpen(getIntent().getStringExtra("token"),this);finish();
        },150);
    }
}
