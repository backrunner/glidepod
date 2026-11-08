package dev.glidepod.fixture;

import android.app.Activity;
import android.location.Location;
import android.location.LocationListener;
import android.location.LocationManager;
import android.os.Bundle;
import android.widget.TextView;
import java.io.FileOutputStream;
import org.json.JSONObject;

/** Original unprivileged GPS acceptance fixture, separate from the input tests. */
public final class LocationActivity extends Activity implements LocationListener {
    private LocationManager manager;
    private TextView label;
    private void record(JSONObject value) {
        try (FileOutputStream file = openFileOutput("location.json", MODE_PRIVATE)) {
            file.write(value.toString().getBytes("UTF-8"));
            file.getFD().sync();
            label.setText(value.toString());
        } catch (Exception error) { throw new IllegalStateException(error); }
    }
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        setTurnScreenOn(true);
        getWindow().addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        label = new TextView(this); setContentView(label);
        manager = getSystemService(LocationManager.class);
        try {
            record(new JSONObject().put("state", "waiting"));
            manager.requestLocationUpdates(LocationManager.GPS_PROVIDER, 1000, 0, this);
        } catch (SecurityException denied) {
            try { record(new JSONObject().put("state", "permission_denied")); } catch (Exception error) { throw new IllegalStateException(error); }
        } catch (Exception error) { throw new IllegalStateException(error); }
    }
    @Override public void onLocationChanged(Location location) {
        try {
            record(new JSONObject().put("state", "received").put("provider", location.getProvider())
                .put("latitude", location.getLatitude()).put("longitude", location.getLongitude())
                .put("accuracy", location.getAccuracy()).put("timestamp", location.getTime()));
        } catch (Exception error) { throw new IllegalStateException(error); }
    }
    @Override public void onProviderEnabled(String provider) { }
    @Override public void onProviderDisabled(String provider) { }
    @Override public void onStatusChanged(String provider, int status, Bundle extras) { }
    @Override public void onDestroy() {
        if (manager != null) manager.removeUpdates(this);
        super.onDestroy();
    }
}
