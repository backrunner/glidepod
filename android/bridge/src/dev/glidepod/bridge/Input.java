package dev.glidepod.bridge;

import android.app.*;
import android.content.*;
import android.hardware.input.InputManager;
import android.os.*;
import android.view.*;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.*;
import org.json.*;

/** Private, display-bound shell transport. EOF, inactivity and focus loss release all input. */
public final class Input {
    private final int display, width, height;
    private final String packageName;
    private final Object manager;
    private final Method inject, setDisplay;
    private final TreeMap<Integer, Long> keys = new TreeMap<>();
    private final HashMap<Integer, Integer> repeats = new HashMap<>();
    private final TreeMap<Integer, float[]> touches = new TreeMap<>();
    private long touchStart, mouseStart, lastMessage = SystemClock.uptimeMillis();
    private int mouseButtons;
    private Gamepad gamepad;
    private String gamepadError;
    private float mouseX, mouseY;
    private boolean active;

    Input(int display, int width, int height, String packageName) throws Exception {
        this.display = display; this.width = width; this.height = height; this.packageName = packageName;
        Class<?> inputClass = Class.forName("android.hardware.input.InputManagerGlobal");
        manager = inputClass.getMethod("getInstance").invoke(null);
        inject = inputClass.getMethod("injectInputEvent", InputEvent.class, int.class);
        setDisplay = InputEvent.class.getMethod("setDisplayId", int.class);
        try {gamepad=new Gamepad();}catch(Exception e){gamepadError=e.toString();}
    }
    private void send(InputEvent event) throws Exception {
        setDisplay.invoke(event, display);
        try {
            if (!Boolean.TRUE.equals(inject.invoke(manager, event, 1))) throw new IOException("Input rejected by display " + display);
        } finally { if (event instanceof MotionEvent) ((MotionEvent)event).recycle(); }
    }
    private void key(int code, boolean down, int meta) throws Exception {
        if (code < 1 || code > 304) throw new IOException("Invalid key");
        Map<Integer, Long> held = keys;
        long now = SystemClock.uptimeMillis();
        if (!down && !held.containsKey(code)) return;
        boolean repeated = down && held.containsKey(code);
        long start = held.containsKey(code) ? held.get(code) : now;
        int repeat = repeated ? repeats.getOrDefault(code, 0) + 1 : 0;
        if (down) { held.put(code,start); repeats.put(code,repeat); }
        else { held.remove(code); repeats.remove(code); }
        send(new KeyEvent(start,now,down ? KeyEvent.ACTION_DOWN : KeyEvent.ACTION_UP,code,repeat,meta,
            KeyCharacterMap.VIRTUAL_KEYBOARD,0,0,InputDevice.SOURCE_KEYBOARD));
    }
    private void touchEvent(int action) throws Exception {
        MotionEvent.PointerProperties[] props = new MotionEvent.PointerProperties[touches.size()];
        MotionEvent.PointerCoords[] coords = new MotionEvent.PointerCoords[touches.size()];
        int i = 0;
        for (Map.Entry<Integer,float[]> p : touches.entrySet()) {
            props[i] = new MotionEvent.PointerProperties(); props[i].id = p.getKey(); props[i].toolType = MotionEvent.TOOL_TYPE_FINGER;
            coords[i] = new MotionEvent.PointerCoords(); coords[i].x = p.getValue()[0]; coords[i].y = p.getValue()[1]; coords[i].pressure = 1; coords[i].size = 1; i++;
        }
        send(MotionEvent.obtain(touchStart,SystemClock.uptimeMillis(),action,props.length,props,coords,0,0,1,1,0,0,InputDevice.SOURCE_TOUCHSCREEN,0));
    }
    private float finite(JSONArray a, int index, float min, float max) throws Exception {
        double n = a.getDouble(index);
        if (!Double.isFinite(n) || n < min || n > max) throw new IOException("Axis outside range");
        return (float)n;
    }
    private void touch(JSONArray points) throws Exception {
        if (points.length() > 10) throw new IOException("Too many contacts");
        TreeMap<Integer,float[]> next = new TreeMap<>();
        for (int i=0;i<points.length();i++) {
            JSONArray p = points.getJSONArray(i); int id = p.getInt(0);
            if (p.length()!=3 || id<0 || id>31 || next.containsKey(id)) throw new IOException("Invalid pointer");
            next.put(id,new float[]{finite(p,1,0,width-1),finite(p,2,0,height-1)});
        }
        for (int id : new ArrayList<>(touches.keySet())) if (next.containsKey(id)) touches.put(id,next.get(id));
        for (int id : new ArrayList<>(touches.keySet())) if (!next.containsKey(id)) {
            int index = new ArrayList<>(touches.keySet()).indexOf(id);
            touchEvent(touches.size()==1 ? MotionEvent.ACTION_UP : MotionEvent.ACTION_POINTER_UP | (index << 8)); touches.remove(id);
        }
        boolean added = false;
        for (int id : next.keySet()) if (!touches.containsKey(id)) {
            if (touches.isEmpty()) touchStart = SystemClock.uptimeMillis();
            touches.put(id,next.get(id)); int index = new ArrayList<>(touches.keySet()).indexOf(id);
            touchEvent(touches.size()==1 ? MotionEvent.ACTION_DOWN : MotionEvent.ACTION_POINTER_DOWN | (index << 8)); added = true;
        }
        if (!touches.isEmpty() && !added) touchEvent(MotionEvent.ACTION_MOVE);
    }
    private void motion(int action, int source, int device, float x, float y, int buttons, int actionButton, float[] axes) throws Exception {
        MotionEvent.PointerProperties p = new MotionEvent.PointerProperties(); p.id=0; p.toolType = source==InputDevice.SOURCE_MOUSE ? MotionEvent.TOOL_TYPE_MOUSE : MotionEvent.TOOL_TYPE_UNKNOWN;
        MotionEvent.PointerCoords c = new MotionEvent.PointerCoords(); c.x=x; c.y=y;
        if (axes != null) {
            int[] ids = source==InputDevice.SOURCE_MOUSE ? new int[]{MotionEvent.AXIS_HSCROLL,MotionEvent.AXIS_VSCROLL} :
                new int[]{MotionEvent.AXIS_X,MotionEvent.AXIS_Y,MotionEvent.AXIS_Z,MotionEvent.AXIS_RZ,MotionEvent.AXIS_LTRIGGER,MotionEvent.AXIS_RTRIGGER,MotionEvent.AXIS_HAT_X,MotionEvent.AXIS_HAT_Y};
            for(int i=0;i<ids.length;i++) c.setAxisValue(ids[i],axes[i]);
        }
        MotionEvent e = MotionEvent.obtain(mouseStart,SystemClock.uptimeMillis(),action,1,new MotionEvent.PointerProperties[]{p},new MotionEvent.PointerCoords[]{c},0,buttons,1,1,device,0,source,0);
        if (actionButton!=0) MotionEvent.class.getMethod("setActionButton",int.class).invoke(e,actionButton);
        send(e);
    }
    private void mouse(JSONArray a) throws Exception {
        if(a.length()!=5) throw new IOException("Invalid mouse");
        mouseX=finite(a,0,0,width-1); mouseY=finite(a,1,0,height-1); int buttons=a.getInt(2);
        if (buttons<0 || buttons>7) throw new IOException("Invalid mouse buttons");
        float[] scroll={finite(a,3,-100,100),finite(a,4,-100,100)};
        if(mouseButtons==0 && buttons!=0) { mouseStart=SystemClock.uptimeMillis(); motion(MotionEvent.ACTION_DOWN,InputDevice.SOURCE_MOUSE,0,mouseX,mouseY,buttons,0,null); }
        for(int bit=1;bit<=4;bit<<=1) if((mouseButtons&bit)!=(buttons&bit)) motion((buttons&bit)!=0 ? MotionEvent.ACTION_BUTTON_PRESS : MotionEvent.ACTION_BUTTON_RELEASE,InputDevice.SOURCE_MOUSE,0,mouseX,mouseY,buttons,bit,null);
        if(mouseButtons!=0 && buttons==0) motion(MotionEvent.ACTION_UP,InputDevice.SOURCE_MOUSE,0,mouseX,mouseY,0,0,null);
        else motion(buttons==0 ? MotionEvent.ACTION_HOVER_MOVE : MotionEvent.ACTION_MOVE,InputDevice.SOURCE_MOUSE,0,mouseX,mouseY,buttons,0,null);
        mouseButtons=buttons;
        if(scroll[0]!=0 || scroll[1]!=0) motion(MotionEvent.ACTION_SCROLL,InputDevice.SOURCE_MOUSE,0,mouseX,mouseY,buttons,0,scroll);
    }
    private int focusedDisplay() throws Exception {
        Class<?> atm=Class.forName("android.app.ActivityTaskManager"); Object service=atm.getMethod("getService").invoke(null);
        Object task=Class.forName("android.app.IActivityTaskManager").getMethod("getFocusedRootTaskInfo").invoke(service);
        return task==null ? -1 : task.getClass().getField("displayId").getInt(task);
    }
    private void pad(int buttons, JSONArray axes) throws Exception {
        if(gamepad==null) throw new IOException("Guest gamepad unavailable: "+gamepadError);
        if(focusedDisplay()!=display) throw new IOException("Gamepad target is not the focused display");
        gamepad.send(buttons,axes);
    }
    private void focus() throws Exception {
        // Bring the already-owned task to input focus; never launch or move another task.
        Class<?> atm=Class.forName("android.app.ActivityTaskManager"); Object service=atm.getMethod("getService").invoke(null);
        Class<?> api=Class.forName("android.app.IActivityTaskManager");
        long deadline=SystemClock.uptimeMillis()+1000;
        do {
            List<?> tasks=(List<?>)api.getMethod("getAllRootTaskInfosOnDisplay",int.class).invoke(service,display);
            for(Object task:tasks) {
                ComponentName top=(ComponentName)task.getClass().getField("topActivity").get(task);
                if(top!=null && packageName.equals(top.getPackageName())) {
                    int id=task.getClass().getField("taskId").getInt(task);
                    api.getMethod("setFocusedTask",int.class).invoke(service,id);
                    while(focusedDisplay()!=display && SystemClock.uptimeMillis()<deadline) Thread.sleep(5);
                    if(focusedDisplay()!=display) throw new IOException("Display did not acquire focus");
                    active=true; return;
                }
            }
            Thread.sleep(20);
        } while(SystemClock.uptimeMillis()<deadline);
        throw new IOException("The app " + packageName + " no longer owns display " + display + ": " + api.getMethod("getAllRootTaskInfosOnDisplay",int.class).invoke(service,display));
    }
    private synchronized void reset() throws Exception {
        active=false;
        Exception failed=null;
        try { if(!touches.isEmpty()) touchEvent(MotionEvent.ACTION_CANCEL); } catch(Exception e){failed=e;} finally {touches.clear();}
        for(int code:new ArrayList<>(keys.keySet())) try {key(code,false,0);}catch(Exception e){failed=e;}
        try { if(mouseButtons!=0) motion(MotionEvent.ACTION_CANCEL,InputDevice.SOURCE_MOUSE,0,mouseX,mouseY,0,0,null); }catch(Exception e){failed=e;} finally{mouseButtons=0;}
        try {if(gamepad!=null) gamepad.reset();}catch(Exception e){failed=e;}
        if(failed!=null) throw failed;
    }
    private synchronized void command(JSONObject obj) throws Exception {
        lastMessage=SystemClock.uptimeMillis(); String op=obj.getString("op");
        if(op.equals("ping")) return;
        if(op.equals("reset")) {reset(); return;}
        if(op.equals("focus")) {reset(); focus(); return;}
        if(!active) throw new IOException("Window has no input lease");
        switch(op) {
            case "key": key(obj.getInt("code"),obj.getBoolean("down"),obj.getInt("meta")); break;
            case "touch": touch(obj.getJSONArray("points")); break;
            case "mouse": mouse(obj.getJSONArray("state")); break;
            case "pad": pad(obj.getInt("buttons"),obj.getJSONArray("axes")); break;
            default: throw new IOException("Unknown input operation");
        }
    }
    private static String line(InputStream stream) throws IOException {
        ByteArrayOutputStream b=new ByteArrayOutputStream(); int c;
        while((c=stream.read())!=-1) { if(c==10) return b.toString("UTF-8"); if(b.size()>=65536) throw new IOException("Input packet too large"); b.write(c); }
        return null;
    }
    public static void main(String[] args) throws Exception {
        try { run(args); }
        catch(Throwable e) {System.out.println(new JSONObject().put("error",e.toString()+" "+String.valueOf(e.getCause())));System.out.flush();System.exit(1);}
    }
    private static void run(String[] args) throws Exception {
        Looper.prepareMainLooper();
        Class<?> thread = Class.forName("android.app.ActivityThread");
        thread.getMethod("systemMain").invoke(null);
        Input input=new Input(Integer.parseInt(args[0]),Integer.parseInt(args[1]),Integer.parseInt(args[2]),args[3]);
        System.out.println(new JSONObject().put("ready",1).put("display",input.display).put("gamepad",input.gamepad==null?-1:input.gamepad.deviceId)); System.out.flush();
        ScheduledExecutorService watchdog=Executors.newSingleThreadScheduledExecutor();
        watchdog.scheduleAtFixedRate(()->{synchronized(input){if(SystemClock.uptimeMillis()-input.lastMessage>1500) try{input.reset();}catch(Exception ignored){}}},250,250,TimeUnit.MILLISECONDS);
        try {
            String line;
            while((line=line(System.in))!=null) {
                try {input.command(new JSONObject(line)); System.out.println("{\"ok\":true}");}
                catch(Exception e) {System.out.println(new JSONObject().put("error",e.toString()+" "+String.valueOf(e.getCause()))); break;}
                finally {System.out.flush();}
            }
        } finally {watchdog.shutdownNow(); try{input.reset();}finally{System.exit(0);}}
    }
}
