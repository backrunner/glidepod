// Adapter contract test: in-process event objects, no events posted to the desktop.
#import "../src/gestures.m"
#include <assert.h>
#include <math.h>
@interface GPTestApplication : NSApplication @end
@implementation GPTestApplication
- (BOOL)isActive { return YES; }
@end
@interface GPTestWindow : NSWindow
@property BOOL testKey;
@end
@implementation GPTestWindow
- (BOOL)isKeyWindow { return self.testKey; }
@end
@interface GPTestEvent : NSEvent
@property NSEventType testType;
@property NSEventPhase testPhase, testMomentum;
@property(nonatomic, strong) NSWindow *testWindow;
@property NSPoint testPoint;
@property double testDX, testDY;
@property BOOL precise;
@end
@implementation GPTestEvent
- (NSEventType)type { return self.testType; }
- (NSEventPhase)phase { return self.testPhase; }
- (NSEventPhase)momentumPhase { return self.testMomentum; }
- (NSWindow *)window { return self.testWindow; }
- (NSPoint)locationInWindow { return self.testPoint; }
- (CGFloat)scrollingDeltaX { return self.testDX; }
- (CGFloat)scrollingDeltaY { return self.testDY; }
- (CGFloat)magnification { return self.testDX; }
- (float)rotation { return self.testDX; }
- (BOOL)hasPreciseScrollingDeltas { return self.precise; }
@end
int main(void) {
    @autoreleasepool {
        [GPTestApplication sharedApplication];
        GPTestWindow *window = [[GPTestWindow alloc] initWithContentRect:NSMakeRect(0,0,600,800)
            styleMask:NSWindowStyleMaskBorderless backing:NSBackingStoreBuffered defer:NO];
        window.testKey = YES;
        NSView *view = window.contentView;
        void *handle = glidepod_gestures_new((__bridge void *)view);
        GPGestures *state = (__bridge GPGestures *)handle;
        glidepod_gestures_configure(handle,100,50,360,540,true,true);
        GPTestEvent *e = [GPTestEvent new];
        e.testWindow = window; e.testPoint = NSMakePoint(280,480);
        e.testType = NSEventTypeScrollWheel; e.testPhase = NSEventPhaseBegan;
        e.precise = YES; e.testDX = 5; e.testDY = -10;
        assert([state handle:e] == nil);
        GPGestureEvent value;
        assert(glidepod_gestures_next(handle,&value));
        assert(value.kind == 1 && value.phase == 1 && value.x == 280 && value.y == 320);
        assert(value.dx == 5 && value.dy == -10); // no Retina or natural-scroll inversion
        e.testPoint = NSMakePoint(590,10); e.testPhase = NSEventPhaseEnded;
        assert([state handle:e] == nil); // end still delivered outside the content
        assert(glidepod_gestures_next(handle,&value) && value.phase == 3);
        e.testPhase = NSEventPhaseNone; e.testMomentum = NSEventPhaseChanged;
        assert([state handle:e] == nil && !glidepod_gestures_next(handle,&value));
        e.testMomentum = NSEventPhaseNone; e.precise = NO;
        assert([state handle:e] == e); // ordinary mouse wheel stays with GPUI
        e.precise = YES; e.testPhase = NSEventPhaseBegan;
        assert([state handle:e] == e); // a gesture beginning on letterboxing is not captured
        e.testPoint = NSMakePoint(280,480); e.testPhase = NSEventPhaseChanged;
        assert([state handle:e] == e); // entering the surface mid-gesture cannot steal it
        e.testPhase = NSEventPhaseBegan; window.testKey = NO;
        assert([state handle:e] == e);
        window.testKey = YES;
        e.testWindow = nil; assert([state handle:e] == e); e.testWindow = window;
        glidepod_gestures_configure(handle,100,50,360,540,true,false);
        assert([state handle:e] == e); // Mouse mode retains precise wheels and inertia
        e.testType = NSEventTypeMagnify; e.testDX = 0.25;
        assert([state handle:e] == nil);
        assert(glidepod_gestures_next(handle,&value) && value.kind == 2 && value.dx == 0.25);
        e.testType = NSEventTypeRotate; e.testDX = 90;
        assert([state handle:e] == nil);
        e.testPhase = NSEventPhaseCancelled;
        assert([state handle:e] == nil);
        assert(glidepod_gestures_next(handle,&value) && value.kind == 3 && value.dx == 90);
        assert(glidepod_gestures_next(handle,&value) && value.phase == 4);
        glidepod_gestures_configure(handle,100,50,360,540,false,false);
        assert(!glidepod_gestures_next(handle,&value) && state.captured == 0);
        assert([state handle:e] == e);
        glidepod_gestures_configure(handle,100,50,360,540,true,true);
        e.testType = NSEventTypeMagnify; e.testPhase = NSEventPhaseBegan;
        [state handle:e]; e.testPhase = NSEventPhaseChanged;
        for (int i=0;i<256;i++) [state handle:e];
        assert(glidepod_gestures_next(handle,&value) && value.kind == 4);
        assert(!glidepod_gestures_next(handle,&value) && state.captured == 0);
        glidepod_gestures_free(handle);
        puts("PASS: native gesture routing, coordinates, phases, inertia, mode, isolation and overflow (synthetic events; no physical trackpad claim)");
    }
    return 0;
}
