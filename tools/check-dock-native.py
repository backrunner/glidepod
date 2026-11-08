#!/usr/bin/env python3
"""Exercise the real AppKit delegate and Unix transport without posting desktop input.

These are native callback tests, not proof of a physical Dock menu click.
"""
from pathlib import Path
import socket
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
HARNESS = r'''
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let delegate = AppDelegate(package: "dev.glidepod.docktest", socketPath: CommandLine.arguments[1])
app.delegate = delegate
delegate.applicationDidFinishLaunching(Notification(name: NSApplication.didFinishLaunchingNotification))
if CommandLine.arguments[2] == "callbacks" {
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
        let menu = delegate.applicationDockMenu(app)!
        precondition(menu.items.count == 1)
        precondition(menu.items[0].title == "Show Window")
        precondition(menu.items[0].target === delegate)
        precondition(menu.items[0].action == #selector(AppDelegate.showWindow))
        // Invoke the same callbacks AppKit uses, without synthesizing user input.
        delegate.showWindow()
        precondition(!delegate.applicationShouldHandleReopen(app, hasVisibleWindows: false))
        delegate.applicationDidBecomeActive(Notification(name: NSApplication.didBecomeActiveNotification))
        precondition(delegate.applicationShouldTerminate(app) == .terminateNow)
        exit(0)
    }
}
DispatchQueue.main.asyncAfter(deadline: .now() + 3) { exit(7) }
RunLoop.main.run()
'''

with tempfile.TemporaryDirectory(prefix="gp-dock-", dir="/tmp") as directory:
    root = Path(directory)
    source = root / "main.swift"
    source.write_text((ROOT / "macos/packaging/AppLauncher.swift").read_text() + HARNESS)
    binary = root / "dock-native"
    subprocess.run(["xcrun", "swiftc", "-warnings-as-errors", "-D", "DOCK_TESTS",
                    str(source), "-o", str(binary)], check=True)
    for mode in ("callbacks", "host-disconnect"):
        path = root / f"{mode}.sock"
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
            server.bind(str(path))
            server.listen(1)
            server.settimeout(5)
            process = subprocess.Popen([str(binary), str(path), mode])
            try:
                client, _ = server.accept()
                with client:
                    client.settimeout(5)
                    client.sendall(b"rea")
                    client.sendall(b"dy\n")
                    if mode == "callbacks":
                        received = b""
                        while chunk := client.recv(1024):
                            received += chunk
                        assert received == b"focus\nfocus\nfocus\nquit\n", received
                assert process.wait(timeout=5) == 0, mode
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
    # A late host-started helper must not cold-launch the just-closed window.
    subprocess.run([str(binary), str(root / "missing.sock"), "closed-before-start",
                    "--glidepod-managed"], check=True, timeout=5)
print("Native Dock callbacks, fragmented handshake, host disconnect, and late-start cleanup passed.")
