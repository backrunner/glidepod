package dev.glidepod.bridge;

import android.os.SystemClock;
import android.view.InputDevice;
import java.io.*;
import java.nio.*;
import java.nio.file.Files;
import java.util.*;
import org.json.JSONArray;

/** Feed the registered virtio gamepad through evdev so Android retains device identity/ranges. */
final class Gamepad implements Closeable {
    private static final int[] BUTTONS={0x130,0x131,0x133,0x134,0x136,0x137,-1,-1,0x13a,0x13b,0x13c,0x13d,0x13e};
    private static final int[] AXES={0,1,3,4,2,5,16,17};
    final int deviceId;
    private final FileOutputStream device;
    private int buttons;
    private final int[] axes=new int[8];
    Gamepad() throws Exception {
        int id=-1;
        for(int candidate:InputDevice.getDeviceIds()) {
            InputDevice d=InputDevice.getDevice(candidate);
            if(d!=null && d.getVendorId()==0x045e && d.getProductId()==0x02ea && d.supportsSource(InputDevice.SOURCE_JOYSTICK)) {id=candidate;break;}
        }
        if(id<0) throw new IOException("Registered guest gamepad unavailable");
        File event=null;
        File[] entries=new File("/sys/class/input").listFiles();
        if(entries!=null) for(File entry:entries) if(entry.getName().matches("event[0-9]+")) {
            String unique=new String(Files.readAllBytes(new File(entry,"device/uniq").toPath()),"UTF-8").trim();
            String name=new String(Files.readAllBytes(new File(entry,"device/name").toPath()),"UTF-8").trim();
            if(unique.equals("xbox_wireless_controller") && name.equals(InputDevice.getDevice(id).getName())) {event=new File("/dev/input",entry.getName());break;}
        }
        if(event==null) throw new IOException("Registered virtio gamepad event node unavailable");
        device=new FileOutputStream(event); deviceId=id;
    }
    private static void event(ByteBuffer b,int type,int code,int value) {
        long now=SystemClock.uptimeMillis();
        b.putLong(now/1000).putLong((now%1000)*1000).putShort((short)type).putShort((short)code).putInt(value);
    }
    void send(int nextButtons,JSONArray values) throws Exception {
        if(nextButtons<0 || nextButtons>=(1<<13) || values.length()!=8) throw new IOException("Invalid gamepad report");
        int[] nextAxes=new int[8];
        for(int i=0;i<8;i++) {
            double n=values.getDouble(i),min=(i==4||i==5)?0:-1;
            if(!Double.isFinite(n)||n<min||n>1) throw new IOException("Gamepad axis outside range");
            nextAxes[i]=(int)Math.round(n*(i<4 ? (n<0?32768:32767) : i<6?1023:1));
        }
        write(nextButtons,nextAxes);
    }
    private void write(int nextButtons,int[] nextAxes) throws IOException {
        ByteBuffer b=ByteBuffer.allocate(24*22).order(ByteOrder.LITTLE_ENDIAN);
        for(int i=0;i<BUTTONS.length;i++) if(BUTTONS[i]>=0 && (buttons&(1<<i))!=(nextButtons&(1<<i))) event(b,1,BUTTONS[i],(nextButtons&(1<<i))==0?0:1);
        for(int i=0;i<8;i++) if(axes[i]!=nextAxes[i]) event(b,3,AXES[i],nextAxes[i]);
        if(b.position()>0) {event(b,0,0,0);device.write(b.array(),0,b.position());}
        buttons=nextButtons;System.arraycopy(nextAxes,0,axes,0,8);
    }
    void reset() throws IOException {write(0,new int[8]);}
    @Override public void close() throws IOException {try{reset();}finally{device.close();}}
}
