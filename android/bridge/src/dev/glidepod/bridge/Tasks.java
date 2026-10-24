package dev.glidepod.bridge;

import android.content.ComponentName;
import java.util.List;
import org.json.JSONObject;

/** Only manages tasks belonging to the package on its exclusively leased app display. */
public final class Tasks {
    private static boolean owned(Object task, String packageName) throws Exception {
        ComponentName base = (ComponentName) task.getClass().getField("baseActivity").get(task);
        return base != null && packageName.equals(base.getPackageName());
    }
    static int query(int display, String packageName, boolean close) throws Exception {
        if (display <= 0) throw new IllegalArgumentException("Never manage the default display");
        Class<?> api = Class.forName("android.app.IActivityTaskManager");
        Object service = Class.forName("android.app.ActivityTaskManager").getMethod("getService").invoke(null);
        List<?> tasks = (List<?>) api.getMethod("getAllRootTaskInfosOnDisplay", int.class).invoke(service, display);
        int count = 0;
        for (Object task : tasks) {
            if (!owned(task, packageName)) continue;
            if (close) {
                int id = task.getClass().getField("taskId").getInt(task);
                if (!Boolean.TRUE.equals(api.getMethod("removeTask", int.class).invoke(service, id)))
                    throw new IllegalStateException("Android did not remove task " + id);
            }
            count++;
        }
        return count;
    }
    public static void main(String[] args) throws Exception {
        if (args.length != 3 || !(args[0].equals("close") || args[0].equals("count")))
            throw new IllegalArgumentException("Expected close/count display package");
        System.out.println(new JSONObject().put("count", query(Integer.parseInt(args[1]), args[2], args[0].equals("close"))));
    }
}
