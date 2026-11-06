package dev.glidepod.notifications;
import android.app.*;
import android.content.pm.*;
import android.service.notification.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicLong;
import org.json.*;

public final class Listener extends NotificationListenerService {
    static volatile Listener current;
    private final AtomicLong revision=new AtomicLong(1);
    @Override public void onListenerConnected(){current=this;revision.incrementAndGet();}
    @Override public void onListenerDisconnected(){current=null;revision.incrementAndGet();}
    @Override public void onDestroy(){if(current==this)current=null;super.onDestroy();}
    @Override public void onNotificationPosted(StatusBarNotification n){revision.incrementAndGet();}
    @Override public void onNotificationRemoved(StatusBarNotification n){revision.incrementAndGet();}
    @Override public void onNotificationRankingUpdate(RankingMap r){revision.incrementAndGet();}
    private static String text(CharSequence s,int max){String t=s==null?"":s.toString();if(t.length()<=max)return t;if(Character.isHighSurrogate(t.charAt(max-1)))max--;return t.substring(0,max);}
    JSONObject snapshot(int offset) throws Exception {
        if(offset<0||offset>4096)throw new IllegalArgumentException("Invalid notification page");
        long version=revision.get();JSONArray a=new JSONArray();
        StatusBarNotification[] active=getActiveNotifications();if(active==null)active=new StatusBarNotification[0];
        Arrays.sort(active,Comparator.comparing(StatusBarNotification::getKey));
        for(int index=offset;index<Math.min(active.length,offset+64);index++) {
            StatusBarNotification s=active[index];Notification n=s.getNotification();
            // Relay every entry Android exposes, including system apps, ongoing and group summaries.
            String label=text(s.getPackageName(),256);
            try{ApplicationInfo app=getPackageManager().getApplicationInfo(s.getPackageName(),0);label=text(app.loadLabel(getPackageManager()),256);}
            catch(PackageManager.NameNotFoundException ignored){}
            boolean redacted=n.visibility!=Notification.VISIBILITY_PUBLIC;
            Ranking ranking=new Ranking();
            boolean alerting=getCurrentRanking().getRanking(s.getKey(),ranking) && ranking.getImportance()>=NotificationManager.IMPORTANCE_DEFAULT && ranking.matchesInterruptionFilter();
            Notification shown=n.visibility==Notification.VISIBILITY_SECRET?null:redacted?n.publicVersion:n;
            String title=shown==null?label:text(shown.extras.getCharSequence(Notification.EXTRA_TITLE),256);
            CharSequence body=shown==null?"New notification":shown.extras.getCharSequence(Notification.EXTRA_BIG_TEXT);
            if(body==null&&shown!=null)body=shown.extras.getCharSequence(Notification.EXTRA_TEXT);
            if(body==null&&shown!=null){CharSequence[] lines=shown.extras.getCharSequenceArray(Notification.EXTRA_TEXT_LINES);if(lines!=null)body=android.text.TextUtils.join("\n",lines);}
            a.put(new JSONObject().put("key",s.getKey()).put("package_id",s.getPackageName()).put("app_label",label)
                .put("title",title).put("body",text(body,2048)).put("posted_ms",s.getPostTime()).put("clearable",s.isClearable()).put("redacted",redacted).put("alerting",alerting)
                .put("ongoing",s.isOngoing()).put("group_summary",(n.flags&Notification.FLAG_GROUP_SUMMARY)!=0).put("group",text(s.getGroupKey(),1024)).put("user_id",s.getUserId()).put("can_open",n.contentIntent!=null));
        }
        return new JSONObject().put("items",a).put("total",active.length).put("revision",version).put("consistent",version==revision.get());
    }
    void dismiss(String argument) throws Exception {
        JSONObject request=new JSONObject(argument);String key=request.getString("key");
        if(key.isEmpty()||key.length()>4096)throw new IllegalArgumentException("Invalid notification key");
        for(StatusBarNotification s:getActiveNotifications())if(s.getKey().equals(key)){
            if(!s.getPackageName().equals(request.getString("package_id"))||s.getPostTime()!=request.getLong("posted_ms"))throw new SecurityException("Notification changed; select the current notification");
            if(!s.isClearable())throw new SecurityException("Android does not allow dismissal of this notification");
            cancelNotification(key);return;
        }
    }
    void open(String argument,android.content.Context sender) throws Exception {
        JSONObject request=new JSONObject(argument);String key=request.getString("key");
        int display=request.getInt("display");if(display<=0)throw new IllegalArgumentException("A managed display is required");
        for(StatusBarNotification s:getActiveNotifications())if(s.getKey().equals(key)){
            if(!s.getPackageName().equals(request.getString("package_id"))||s.getPostTime()!=request.getLong("posted_ms"))throw new SecurityException("Notification changed; select the current notification");
            Notification n=s.getNotification();if(n.contentIntent==null)throw new IllegalStateException("This Android notification has no click action");
            ActivityOptions options=ActivityOptions.makeBasic().setLaunchDisplayId(display);
            options.setPendingIntentBackgroundActivityStartMode(ActivityOptions.MODE_BACKGROUND_ACTIVITY_START_ALLOWED);
            n.contentIntent.send(sender,0,null,null,null,null,options.toBundle());
            if((n.flags&Notification.FLAG_AUTO_CANCEL)!=0&&s.isClearable())cancelNotification(key);
            return;
        }
        throw new IllegalStateException("This Android notification is no longer active");
    }
}
