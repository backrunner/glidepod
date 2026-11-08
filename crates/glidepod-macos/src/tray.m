#import <AppKit/AppKit.h>
#include <string.h>

// Only these immutable values cross threads. No AppKit object leaves the main thread.
static NSDictionary *latest;
static NSTimeInterval received;

@interface GlidePodTray : NSObject <NSMenuDelegate>
@property NSStatusItem *item;
@property NSMenu *menu;
@property NSTimer *timer;
@property NSMutableArray<NSDictionary *> *actions;
@property BOOL tracking;
@end

static NSString *text(NSDictionary *snapshot, NSString *key, NSString *fallback) {
    id value = snapshot[key];
    return [value isKindOfClass:NSString.class] ? value : fallback;
}

@implementation GlidePodTray
- (instancetype)init {
    self = [super init];
    if (!self) return nil;
    _actions = [NSMutableArray array];
    _menu = [[NSMenu alloc] initWithTitle:@"GlidePod"];
    _menu.autoenablesItems = NO;
    _menu.minimumWidth = 280;
    _menu.delegate = self;
    _item = [NSStatusBar.systemStatusBar statusItemWithLength:NSSquareStatusItemLength];
    _item.button.image = [NSImage imageWithSystemSymbolName:@"square.stack.3d.up" accessibilityDescription:@"GlidePod"];
    _item.button.image.template = YES;
    _item.button.accessibilityLabel = @"GlidePod";
    _item.menu = _menu;
    [self refresh];
    __weak GlidePodTray *weakSelf = self;
    _timer = [NSTimer timerWithTimeInterval:1 repeats:YES block:^(NSTimer *timer) {
        (void)timer;
        [weakSelf refresh];
    }];
    [NSRunLoop.mainRunLoop addTimer:_timer forMode:NSRunLoopCommonModes];
    return self;
}
- (void)dispose {
    [_timer invalidate];
    _timer = nil;
    _menu.delegate = nil;
    [_menu cancelTracking];
    [NSStatusBar.systemStatusBar removeStatusItem:_item];
    _item = nil;
}
- (NSMenuItem *)row:(NSString *)title key:(NSString *)key {
    NSMenuItem *row = [[NSMenuItem alloc] initWithTitle:title action:nil keyEquivalent:@""];
    row.identifier = key;
    row.enabled = NO;
    [_menu addItem:row];
    return row;
}
- (void)action:(NSString *)title payload:(NSDictionary *)payload {
    NSMenuItem *row = [[NSMenuItem alloc] initWithTitle:title action:@selector(selected:) keyEquivalent:@""];
    row.target = self;
    row.representedObject = payload;
    row.enabled = YES;
    [_menu addItem:row];
}
- (void)selected:(NSMenuItem *)sender {
    if (_actions.count < 32) [_actions addObject:sender.representedObject];
}
- (void)refresh {
    NSDictionary *snapshot;
    @synchronized(GlidePodTray.class) {
        snapshot = (NSProcessInfo.processInfo.systemUptime - received <= 5) ? latest : nil;
    }
    NSString *state = text(snapshot, @"state", @"Status unavailable");
    NSString *cpu = text(snapshot, @"cpu", @"—");
    NSString *memory = text(snapshot, @"memory", @"—");
    NSString *allocation = text(snapshot, @"allocation", @"—");
    id count = snapshot[@"app_count"];
    NSString *apps = [count isKindOfClass:NSNumber.class]
        ? [NSString stringWithFormat:@"Open apps: %@", count] : @"Open apps: —";
    _item.button.toolTip = [NSString stringWithFormat:@"GlidePod · %@ · %@", state, apps];
    // Keep action targets stable while the user is navigating the menu.
    if (!_tracking) {
        [_menu removeAllItems];
        [self row:@"" key:@"state"];
        [_menu addItem:NSMenuItem.separatorItem];
        [self row:@"" key:@"cpu"].toolTip = @"VM process CPU. 100% means one fully occupied host CPU core.";
        [self row:@"" key:@"memory"].toolTip = @"Physical memory footprint of the Android VM process on this Mac.";
        [self row:@"" key:@"allocation"].toolTip = @"Resources assigned to Android. This is not measured memory usage.";
        [_menu addItem:NSMenuItem.separatorItem];
        [self row:@"" key:@"apps"].toolTip = @"Android apps with an open GlidePod window in the current VM session.";
        for (NSDictionary *app in snapshot[@"apps"]) {
            [self action:app[@"title"] payload:@{@"kind": @"focus_app", @"package_id": app[@"package_id"]}];
            _menu.itemArray.lastObject.image = [NSImage imageWithSystemSymbolName:@"app" accessibilityDescription:nil];
        }
        [_menu addItem:NSMenuItem.separatorItem];
        [self action:@"Open GlidePod…" payload:@{@"kind": @"open_center"}];
        [self action:@"Quit GlidePod" payload:@{@"kind": @"quit"}];
    }
    NSDictionary *titles = @{@"state": [@"Android · " stringByAppendingString:state],
        @"cpu": [@"VM CPU: " stringByAppendingString:cpu],
        @"memory": [@"VM memory: " stringByAppendingString:memory],
        @"allocation": [@"Assigned: " stringByAppendingString:allocation], @"apps": apps};
    for (NSMenuItem *row in _menu.itemArray) {
        NSString *title = row.identifier ? titles[row.identifier] : nil;
        if (title) row.title = title;
        if ([row.identifier isEqual:@"state"]) row.toolTip = text(snapshot, @"detail", @"Waiting for a current VM status sample.");
        // Do not leave stale app actions enabled after the VM stops or sampling fails.
        if ([row.representedObject[@"kind"] isEqual:@"focus_app"]) {
            NSString *package = row.representedObject[@"package_id"];
            row.enabled = NO;
            for (NSDictionary *app in snapshot[@"apps"]) {
                if ([app[@"package_id"] isEqual:package]) { row.enabled = YES; break; }
            }
        }
    }
}
- (void)menuWillOpen:(NSMenu *)menu {
    _tracking = NO;
    [self refresh];
    _tracking = YES;
}
- (void)menuDidClose:(NSMenu *)menu { _tracking = NO; }
@end

void *glidepod_tray_new(void) {
    if (!NSThread.isMainThread) return NULL;
    return (__bridge_retained void *)[[GlidePodTray alloc] init];
}
void glidepod_tray_free(void *handle) {
    if (!handle) return;
    GlidePodTray *tray = (__bridge_transfer GlidePodTray *)handle;
    [tray dispose];
}
void glidepod_tray_show(void *handle) {
    __weak GlidePodTray *tray = (__bridge GlidePodTray *)handle;
    // Leave the GPUI action callback before entering AppKit's menu tracking loop.
    dispatch_async(dispatch_get_main_queue(), ^{ [tray.item.button performClick:nil]; });
}
void glidepod_tray_publish(const unsigned char *data, size_t len) {
    @autoreleasepool {
        id snapshot = [NSJSONSerialization JSONObjectWithData:[NSData dataWithBytes:data length:len] options:0 error:NULL];
        if (![snapshot isKindOfClass:NSDictionary.class]) return;
        @synchronized(GlidePodTray.class) {
            latest = snapshot;
            received = NSProcessInfo.processInfo.systemUptime;
        }
    }
}
char *glidepod_tray_drain(void *handle) {
    GlidePodTray *tray = (__bridge GlidePodTray *)handle;
    NSData *data = [NSJSONSerialization dataWithJSONObject:tray.actions options:0 error:NULL];
    [tray.actions removeAllObjects];
    return data ? strdup([[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding].UTF8String) : NULL;
}
