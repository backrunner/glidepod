#import <AppKit/AppKit.h>

typedef struct {
    uint32_t kind, phase; // scroll=1, magnify=2, rotate=3, overflow=4; begin=1, move=2, end=3, cancel=4
    double x, y, dx, dy;
} GPGestureEvent;

@interface GPGestures : NSObject
@property(nonatomic, weak) NSView *view;
@property(nonatomic, strong) id monitor;
@property(nonatomic, strong) NSMutableArray<NSValue *> *queue;
@property(nonatomic) NSRect surface;
@property(nonatomic) BOOL enabled, touchScroll;
@property(nonatomic) NSUInteger captured;
- (NSEvent *)handle:(NSEvent *)event;
@end

@implementation GPGestures
- (NSEvent *)handle:(NSEvent *)event {
    NSView *view = self.view;
    if (!self.enabled || !view || event.window != view.window || !view.window.keyWindow || !NSApp.active)
        return event;
    uint32_t kind;
    switch (event.type) {
        case NSEventTypeScrollWheel:
            if (!self.touchScroll || !event.hasPreciseScrollingDeltas ||
                (event.phase == NSEventPhaseNone && event.momentumPhase == NSEventPhaseNone)) return event;
            kind = 1; break;
        case NSEventTypeMagnify: kind = 2; break;
        case NSEventTypeRotate: kind = 3; break;
        default: return event;
    }
    NSPoint p = [view convertPoint:event.locationInWindow fromView:nil];
    if (!view.flipped) p.y = NSMaxY(view.bounds) - p.y;
    p.x -= view.bounds.origin.x;
    NSUInteger bit = 1 << kind;
    // Android supplies fling inertia after touch-up. Swallow AppKit momentum for
    // the captured scroll so it cannot also reach GPUI as a mouse wheel.
    if (kind == 1 && event.momentumPhase != NSEventPhaseNone) {
        return (self.captured & bit) ? nil : event;
    }
    if (event.phase & NSEventPhaseMayBegin) return event;
    BOOL began = (event.phase & NSEventPhaseBegan) != 0;
    if (began) {
        self.captured &= ~bit;
        if (NSPointInRect(p, self.surface)) self.captured |= bit;
    }
    if (!(self.captured & bit)) return event;
    uint32_t phase = began ? 1 : 2;
    if (event.phase & NSEventPhaseEnded) phase = 3;
    if (event.phase & NSEventPhaseCancelled) phase = 4;
    GPGestureEvent value = {kind, phase, p.x, p.y, 0, 0};
    if (kind == 1) { value.dx = event.scrollingDeltaX; value.dy = event.scrollingDeltaY; }
    if (kind == 2) value.dx = event.magnification;
    if (kind == 3) value.dx = event.rotation;
    if (self.queue.count >= 256) {
        [self.queue removeAllObjects];
        value = (GPGestureEvent){4, 4, 0, 0, 0, 0};
        self.captured = 0;
    }
    [self.queue addObject:[NSValue valueWithBytes:&value objCType:@encode(GPGestureEvent)]];
    if (kind != 1 && phase >= 3) self.captured &= ~bit;
    return nil;
}
@end

void *glidepod_gestures_new(void *rawView) {
    NSCAssert(NSThread.isMainThread, @"Gesture monitoring belongs to the UI thread");
    GPGestures *state = [GPGestures new];
    state.view = (__bridge NSView *)rawView;
    state.queue = [NSMutableArray new];
    __weak GPGestures *weakState = state;
    state.monitor = [NSEvent addLocalMonitorForEventsMatchingMask:
        NSEventMaskScrollWheel | NSEventMaskMagnify | NSEventMaskRotate
        handler:^NSEvent *(NSEvent *event) {
            GPGestures *strongState = weakState;
            return strongState ? [strongState handle:event] : event;
        }];
    return (__bridge_retained void *)state;
}
void glidepod_gestures_configure(void *handle, double x, double y, double w, double h, bool enabled, bool touchScroll) {
    GPGestures *state = (__bridge GPGestures *)handle;
    NSRect surface = NSMakeRect(x, y, w, h);
    if (!enabled || !NSEqualRects(state.surface, surface) || state.touchScroll != touchScroll) {
        [state.queue removeAllObjects];
        state.captured = 0;
    }
    state.surface = surface; state.enabled = enabled; state.touchScroll = touchScroll;
}
bool glidepod_gestures_next(void *handle, GPGestureEvent *event) {
    GPGestures *state = (__bridge GPGestures *)handle;
    NSValue *value = state.queue.firstObject;
    if (!value) return false;
    [value getValue:event size:sizeof(*event)];
    [state.queue removeObjectAtIndex:0];
    return true;
}
void glidepod_gestures_free(void *handle) {
    GPGestures *state = (__bridge_transfer GPGestures *)handle;
    [NSEvent removeMonitor:state.monitor];
}
