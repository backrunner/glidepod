// In-process AppKit adapter checks; no synthetic desktop events or Accessibility calls.
#import "../src/tray.m"
static void require(BOOL passed, NSString *message) {
    if (!passed) { fprintf(stderr, "%s\n", message.UTF8String); exit(1); }
}
static void publish(NSDictionary *snapshot) {
    NSData *data = [NSJSONSerialization dataWithJSONObject:snapshot options:0 error:NULL];
    glidepod_tray_publish(data.bytes, data.length);
}
static NSMenuItem *row(GlidePodTray *tray, NSString *key) {
    for (NSMenuItem *item in tray.menu.itemArray) if ([item.identifier isEqual:key]) return item;
    return nil;
}
int main(void) {
    @autoreleasepool {
        [NSApplication sharedApplication];
        void *handle = glidepod_tray_new();
        GlidePodTray *tray = (__bridge GlidePodTray *)handle;
        require(tray.item.button.image != nil && tray.timer.valid, @"No native icon/timer");
        NSMutableDictionary *sample = [@{@"state": @"Running", @"detail": @"Ready",
            @"cpu": @"142.5%", @"memory": @"2.50 GiB", @"allocation": @"4 vCPUs · 4.00 GiB",
            @"app_count": @2, @"apps": @[@{@"title": @"时钟", @"package_id": @"test.clock"},
                @{@"title": @"Settings", @"package_id": @"test.settings"}]} mutableCopy];
        // Exercise the FFI publication path on a background thread.
        dispatch_sync(dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{ publish(sample); });
        [tray menuWillOpen:tray.menu];
        require([row(tray, @"state").title isEqual:@"Android · Running"], @"Status missing");
        require([row(tray, @"cpu").title isEqual:@"VM CPU: 142.5%"], @"CPU missing");
        require([row(tray, @"apps").title isEqual:@"Open apps: 2"], @"Count missing");
        NSMenuItem *clock = [tray.menu itemWithTitle:@"时钟"];
        require(clock.enabled, @"Unicode app action missing");
        [tray.menu performActionForItemAtIndex:[tray.menu indexOfItem:clock]];
        [tray.menu performActionForItemAtIndex:[tray.menu indexOfItemWithTitle:@"Open GlidePod…"]];
        [tray.menu performActionForItemAtIndex:[tray.menu indexOfItemWithTitle:@"Quit GlidePod"]];
        char *raw = glidepod_tray_drain(handle);
        NSData *data = [[NSString stringWithUTF8String:raw] dataUsingEncoding:NSUTF8StringEncoding];
        free(raw);
        NSArray *actions = [NSJSONSerialization JSONObjectWithData:data options:0 error:NULL];
        require(actions.count == 3 && [actions[0][@"package_id"] isEqual:@"test.clock"] &&
            [actions[1][@"kind"] isEqual:@"open_center"] && [actions[2][@"kind"] isEqual:@"quit"], @"Action routing failed");
        raw = glidepod_tray_drain(handle);
        require(strcmp(raw, "[]") == 0, @"Actions were consumed twice"); free(raw);
        sample[@"cpu"] = @"75.0%";
        sample[@"apps"] = @[];
        sample[@"app_count"] = @0;
        publish(sample);
        NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:2.5];
        while (![row(tray, @"cpu").title isEqual:@"VM CPU: 75.0%"] && deadline.timeIntervalSinceNow > 0) {
            [NSRunLoop.mainRunLoop runMode:NSEventTrackingRunLoopMode beforeDate:deadline];
        }
        require([row(tray, @"cpu").title isEqual:@"VM CPU: 75.0%"], @"Metrics froze during menu tracking");
        require([tray.menu itemWithTitle:@"时钟"] == clock && !clock.enabled, @"Tracking shifted targets or left obsolete action enabled");
        @synchronized(GlidePodTray.class) { received -= 6; }
        [tray refresh];
        require([row(tray, @"state").title isEqual:@"Android · Status unavailable"] &&
            [row(tray, @"memory").title isEqual:@"VM memory: —"], @"Stale metrics remained visible");
        [tray menuDidClose:tray.menu];
        sample[@"state"] = @"Stopped"; sample[@"cpu"] = @"—"; sample[@"memory"] = @"—";
        publish(sample); [tray menuWillOpen:tray.menu];
        require([tray.menu itemWithTitle:@"时钟"] == nil && [row(tray, @"apps").title isEqual:@"Open apps: 0"], @"Stopped app list retained old targets");
        NSTimer *timer = tray.timer;
        glidepod_tray_free(handle);
        require(!timer.valid && tray.item == nil, @"Native teardown leaked timer/status item");
        puts("Passed: native status/menu, background publication, Unicode focus/open/quit actions, common-mode live refresh, stable action targets, stopped/stale states and teardown. No desktop clicks synthesized.");
    }
    return 0;
}
