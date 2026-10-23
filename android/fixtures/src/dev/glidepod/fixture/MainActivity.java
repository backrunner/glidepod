package dev.glidepod.fixture;
import android.app.Activity;
import android.os.Bundle;
import android.widget.*;
import android.graphics.Color;
import android.view.*;
import android.os.*;
import java.io.*;
import org.json.*;
public final class MainActivity extends Activity {
    int clicks;
    private long ticks;
    private final Handler animation = new Handler(Looper.getMainLooper());
    private void record(String kind, InputEvent event) {
        try {
            JSONObject o=new JSONObject().put("kind",kind).put("display",getDisplay().getDisplayId()).put("time",SystemClock.uptimeMillis());
            if(event!=null) {
                o.put("source",event.getSource()).put("device",event.getDeviceId());
                InputDevice d=event.getDevice();
                o.put("joystick_device",d!=null && d.supportsSource(InputDevice.SOURCE_JOYSTICK));
                o.put("x_range",d!=null && d.getMotionRange(MotionEvent.AXIS_X,InputDevice.SOURCE_JOYSTICK)!=null);
            }
            if(event instanceof KeyEvent) {
                KeyEvent k=(KeyEvent)event; o.put("action",k.getAction()).put("key",k.getKeyCode()).put("repeat",k.getRepeatCount()).put("meta",k.getMetaState()).put("down_time",k.getDownTime());
            }
            if(event instanceof MotionEvent) {
                MotionEvent m=(MotionEvent)event; o.put("action",m.getActionMasked()).put("pointers",m.getPointerCount()).put("x",m.getX()).put("y",m.getY()).put("buttons",m.getButtonState());
                JSONArray axes=new JSONArray(); for(int axis:new int[]{0,1,11,14,17,18,15,16,9,10}) axes.put(m.getAxisValue(axis)); o.put("axes",axes);
            }
            try(FileOutputStream f=openFileOutput("events.jsonl",MODE_APPEND)){f.write((o.toString()+"\n").getBytes("UTF-8"));}
        }catch(Exception e){throw new RuntimeException(e);}
    }
    @Override public boolean dispatchKeyEvent(KeyEvent e){record("key",e);return e.isFromSource(InputDevice.SOURCE_GAMEPAD) || super.dispatchKeyEvent(e);}
    @Override public boolean dispatchTouchEvent(MotionEvent e){record("touch",e);return super.dispatchTouchEvent(e);}
    @Override public boolean dispatchGenericMotionEvent(MotionEvent e){record("motion",e);return e.isFromSource(InputDevice.SOURCE_JOYSTICK) || super.dispatchGenericMotionEvent(e);}
    @Override protected void onDestroy(){animation.removeCallbacksAndMessages(null);super.onDestroy();}
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        try {
            if(getIntent().getBooleanExtra("seed_storage",false)) {
                try(FileOutputStream f=new FileOutputStream(new File(getFilesDir(),"persistent.bin"))){f.write(new byte[2*1024*1024]);}
                try(FileOutputStream f=new FileOutputStream(new File(getCacheDir(),"temporary.bin"))){f.write(new byte[1024*1024]);}
                getSharedPreferences("fixture",0).edit().putString("account","test-account").commit();
            }
            if(getIntent().getBooleanExtra("probe_provider",false)) {
                String result="UNEXPECTED_ACCESS";
                try {getContentResolver().call(android.net.Uri.parse("content://dev.glidepod.notifications.bridge"),"snapshot",null,null);}
                catch(SecurityException e){result="DENIED";}
                try(FileOutputStream f=openFileOutput("provider-probe.txt",MODE_PRIVATE)){f.write(result.getBytes("UTF-8"));}
            }
            String notify=getIntent().getStringExtra("notification");
            if(notify!=null){
                android.app.NotificationManager manager=getSystemService(android.app.NotificationManager.class);
                manager.createNotificationChannel(new android.app.NotificationChannel("fixture","Acceptance messages",android.app.NotificationManager.IMPORTANCE_DEFAULT));
                if(notify.equals("cancel"))manager.cancelAll();
                else {
                    int id=notify.equals("private")?102:notify.equals("secret")?103:101;
                    int visibility=id==102?android.app.Notification.VISIBILITY_PRIVATE:id==103?android.app.Notification.VISIBILITY_SECRET:android.app.Notification.VISIBILITY_PUBLIC;
                    android.app.PendingIntent content=android.app.PendingIntent.getActivity(this,0,new android.content.Intent(this,MainActivity.class),android.app.PendingIntent.FLAG_IMMUTABLE|android.app.PendingIntent.FLAG_UPDATE_CURRENT);
                    manager.notify(id,new android.app.Notification.Builder(this,"fixture").setSmallIcon(android.R.drawable.ic_dialog_info).setContentTitle("GlidePod fixture " + notify).setContentText("Synthetic notification · 中文 😀").setContentIntent(content).setVisibility(visibility).setAutoCancel(true).build());
                }
            }
        }catch(Exception e){throw new RuntimeException(e);}
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL); layout.setPadding(24,48,24,24); layout.setBackgroundColor(Color.rgb(237,244,237));
        TextView heading = new TextView(this); heading.setText("GlidePod Android test"); heading.setTextSize(26); layout.addView(heading);
        TextView display = new TextView(this); display.setText("Display " + getDisplay().getDisplayId()); layout.addView(display);
        EditText input = new EditText(this); input.setSingleLine(true); input.setHint("Type from macOS"); layout.addView(input);
        Button button = new Button(this); button.setText("Tap count: 0"); button.setOnClickListener(v -> { button.setText("Tap count: " + (++clicks)); }); layout.addView(button);
        TextView ticker=new TextView(this); layout.addView(ticker);
        Runnable tick=new Runnable(){public void run(){ticker.setText("Independent frame " + (++ticks)); animation.postDelayed(this,100);}};
        animation.post(tick);
        setContentView(layout); input.requestFocus();
    }
}
