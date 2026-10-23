package dev.glidepod.notifications;
import android.app.*;
import android.content.pm.*;
import android.service.notification.*;
import org.json.*;

public final class Listener extends NotificationListenerService {
    static volatile Listener current;
    @Override public void onListenerConnected(){current=this;}
    @Override public void onListenerDisconnected(){current=null;}
    @Override public void onDestroy(){if(current==this)current=null;super.onDestroy();}
    private static String text(CharSequence s,int max){String t=s==null?"":s.toString();return t.length()>max?t.substring(0,max):t;}
    JSONArray snapshot() throws Exception {
        JSONArray a=new JSONArray();
        StatusBarNotification[] active=getActiveNotifications();
        if(active==null)return a;
        for(StatusBarNotification s:active) {
            Notification n=s.getNotification();
            if(a.length()>=256)break;
            if(!s.getUser().equals(android.os.Process.myUserHandle()) || (n.flags&Notification.FLAG_GROUP_SUMMARY)!=0 || s.isOngoing() || n.visibility==Notification.VISIBILITY_SECRET)continue;
            ApplicationInfo app;
            try{app=getPackageManager().getApplicationInfo(s.getPackageName(),0);}catch(PackageManager.NameNotFoundException e){continue;}
            if((app.flags&ApplicationInfo.FLAG_SYSTEM)!=0)continue;
            String label=text(app.loadLabel(getPackageManager()),256);
            boolean redacted=n.visibility!=Notification.VISIBILITY_PUBLIC;
            Ranking ranking=new Ranking();
            boolean alerting=getCurrentRanking().getRanking(s.getKey(),ranking) && ranking.getImportance()>=NotificationManager.IMPORTANCE_DEFAULT && ranking.matchesInterruptionFilter();
            Notification shown=redacted?n.publicVersion:n;
            String title=shown==null?label:text(shown.extras.getCharSequence(Notification.EXTRA_TITLE),256);
            CharSequence body=shown==null?"New notification":shown.extras.getCharSequence(Notification.EXTRA_BIG_TEXT);
            if(body==null && shown!=null)body=shown.extras.getCharSequence(Notification.EXTRA_TEXT);
            a.put(new JSONObject().put("key",s.getKey()).put("package_id",s.getPackageName()).put("app_label",label)
                    .put("title",title).put("body",text(body,2048)).put("posted_ms",s.getPostTime()).put("clearable",s.isClearable()).put("redacted",redacted).put("alerting",alerting));
        }
        return a;
    }
    void dismiss(String key) throws Exception {
        if(key==null||key.length()>1024)throw new IllegalArgumentException("Invalid notification key");
        for(StatusBarNotification s:getActiveNotifications()) if(s.getKey().equals(key)) {
            if(!s.getUser().equals(android.os.Process.myUserHandle())||!s.isClearable())throw new SecurityException("Notification cannot be dismissed");
            cancelNotification(key);return;
        }
    }
}
