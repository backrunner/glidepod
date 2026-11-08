package dev.glidepod.bridge;

import android.content.Context;
import android.net.Uri;
import android.os.Bundle;
import android.os.SystemClock;
import java.io.*;
import java.nio.charset.StandardCharsets;
import org.json.JSONObject;

/** Shell-owned, read-only observations. One process serves all windows in a host store. */
public final class Poll {
    private static volatile long last = SystemClock.uptimeMillis();
    private static void reply(JSONObject value) throws Exception {
        byte[] bytes = (value.toString() + "\n").getBytes(StandardCharsets.UTF_8);
        if (bytes.length > 2 * 1024 * 1024) throw new IOException("Observation exceeds limit");
        System.out.write(bytes); System.out.flush();
    }
    public static void main(String[] args) throws Exception {
        if (args.length != 0) throw new IllegalArgumentException("Unexpected arguments");
        Thread watchdog = new Thread(() -> {
            while (true) {
                try { Thread.sleep(1000); } catch (InterruptedException e) { return; }
                // Covers abandoned streams and blocked provider calls. Binder death releases
                // external provider references even when the host has gone away.
                if (SystemClock.uptimeMillis() - last > 15000) System.exit(0);
            }
        }, "glidepod-poll-idle");
        watchdog.setDaemon(true); watchdog.start();
        try {
            Context context = Management.context();
            reply(new JSONObject().put("ready", 1).put("pid", android.os.Process.myPid()));
            BufferedInputStream input = new BufferedInputStream(System.in);
            while (true) {
                StringBuilder request = new StringBuilder(); int b;
                while ((b = input.read()) != -1 && b != '\n') {
                    if (b < 32 || b > 126 || request.length() >= 32) throw new IOException("Invalid observation");
                    request.append((char)b);
                }
                if (b == -1) break;
                last = SystemClock.uptimeMillis();
                try {
                    String authority, method, arg;
                    if (request.toString().equals("files")) {
                        authority = "dev.glidepod.files.bridge"; method = "poll"; arg = "{}";
                    } else if (request.toString().equals("clipboard")) {
                        authority = "dev.glidepod.input.clipboard"; method = "read"; arg = null;
                    } else throw new IllegalArgumentException("Unknown observation");
                    Uri uri = Uri.parse("content://" + authority);
                    Bundle result = Files.resolver(context, uri).call(uri, method, arg, null);
                    if (result == null) throw new IOException("Provider disconnected");
                    reply(new JSONObject().put("ok", true).put("value", new JSONObject(result.getString("json"))));
                    last = SystemClock.uptimeMillis();
                } catch (Exception error) {
                    reply(new JSONObject().put("ok", false).put("error", error.toString()));
                    break;
                }
            }
        } finally { Files.release(); }
        System.exit(0);
    }
}
