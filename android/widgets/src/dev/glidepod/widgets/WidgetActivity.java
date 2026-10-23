package dev.glidepod.widgets;
import android.app.Activity;
import android.appwidget.*;
import android.content.*;
import android.os.Bundle;
import android.widget.*;
import java.util.*;

/** Live Android RemoteViews host. Widget selection and binding use Android's consent UI. */
public final class WidgetActivity extends Activity {
    private AppWidgetHost host;
    private AppWidgetManager manager;
    private LinearLayout widgets;
    private int pending = -1;
    private final LinkedHashSet<Integer> ids = new LinkedHashSet<>();
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        host = new AppWidgetHost(this, 47021); manager = AppWidgetManager.getInstance(this);
        if (state != null) pending = state.getInt("pending", -1);
        for (String id : getPreferences(0).getStringSet("ids", Collections.emptySet())) {
            try { ids.add(Integer.parseInt(id)); } catch (NumberFormatException ignored) { }
        }
        LinearLayout root = new LinearLayout(this); root.setOrientation(1); root.setPadding(16,24,16,16);
        Button add = new Button(this); add.setText("Add widget"); root.addView(add);
        add.setOnClickListener(v -> {
            if (pending != -1) return;
            pending = host.allocateAppWidgetId();
            Intent pick = new Intent(AppWidgetManager.ACTION_APPWIDGET_PICK);
            pick.putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, pending);
            try { startActivityForResult(pick, 1); } catch (RuntimeException error) { cancel(); Toast.makeText(this, error.getMessage(), Toast.LENGTH_LONG).show(); }
        });
        ScrollView scroll = new ScrollView(this); widgets = new LinearLayout(this); widgets.setOrientation(1); scroll.addView(widgets); root.addView(scroll);
        setContentView(root); render();
    }
    @Override protected void onStart() { super.onStart(); host.startListening(); }
    @Override protected void onStop() { host.stopListening(); super.onStop(); }
    @Override protected void onSaveInstanceState(Bundle state) { state.putInt("pending",pending); super.onSaveInstanceState(state); }
    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request,result,data);
        if (request != 1 && request != 2) return;
        if (result != RESULT_OK || pending == -1) { cancel(); return; }
        AppWidgetProviderInfo info = manager.getAppWidgetInfo(pending);
        if (info == null) { cancel(); return; }
        if (request == 1 && info.configure != null) {
            Intent configure = new Intent(AppWidgetManager.ACTION_APPWIDGET_CONFIGURE).setComponent(info.configure);
            configure.putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID,pending);
            try { startActivityForResult(configure,2); } catch (RuntimeException error) { cancel(); }
            return;
        }
        ids.add(pending); pending = -1; save(); render();
    }
    private void cancel() { if (pending != -1) host.deleteAppWidgetId(pending); pending = -1; }
    private void save() { Set<String> values = new HashSet<>(); for (int id : ids) values.add(Integer.toString(id)); getPreferences(0).edit().putStringSet("ids",values).apply(); }
    private void render() {
        widgets.removeAllViews();
        for (int id : new ArrayList<>(ids)) {
            AppWidgetProviderInfo info = manager.getAppWidgetInfo(id);
            if (info == null) { ids.remove(id); host.deleteAppWidgetId(id); save(); continue; }
            TextView label = new TextView(this); label.setText(info.loadLabel(getPackageManager())); widgets.addView(label);
            AppWidgetHostView view = host.createView(this,id,info);
            android.graphics.drawable.GradientDrawable background = new android.graphics.drawable.GradientDrawable();
            background.setColor(android.graphics.Color.rgb(49,94,75)); background.setCornerRadius(16); view.setBackground(background);
            int width = Math.max(200, (int)(getResources().getDisplayMetrics().widthPixels / getResources().getDisplayMetrics().density)-32);
            int height = Math.max(120, (int)Math.ceil(info.minHeight/getResources().getDisplayMetrics().density));
            view.updateAppWidgetSize(null,width,height,width,height);
            widgets.addView(view,new LinearLayout.LayoutParams(-1,(int)(height*getResources().getDisplayMetrics().density)));
            Button remove = new Button(this); remove.setText("Remove widget"); remove.setOnClickListener(v -> { ids.remove(id); host.deleteAppWidgetId(id); save(); render(); }); widgets.addView(remove);
        }
    }
}
