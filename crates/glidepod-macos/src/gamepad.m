#import <Foundation/Foundation.h>
#import <GameController/GameController.h>
#include <stdint.h>
#include <string.h>

typedef struct { uint64_t identity; uint32_t buttons; float axes[8]; } GPGamepad;
static GCController *selected;
static uint64_t identity;
static void readPad(GCExtendedGamepad *p, GPGamepad *out) {
    GCControllerButtonInput *buttons[] = {p.buttonA,p.buttonB,p.buttonX,p.buttonY,p.leftShoulder,p.rightShoulder,p.leftTrigger,p.rightTrigger,p.buttonOptions,p.buttonMenu,p.buttonHome,p.leftThumbstickButton,p.rightThumbstickButton};
    for(int i=0;i<13;i++) if(buttons[i].isPressed) out->buttons |= (1u<<i);
    out->axes[0]=p.leftThumbstick.xAxis.value; out->axes[1]=-p.leftThumbstick.yAxis.value;
    out->axes[2]=p.rightThumbstick.xAxis.value; out->axes[3]=-p.rightThumbstick.yAxis.value;
    out->axes[4]=p.leftTrigger.value; out->axes[5]=p.rightTrigger.value;
    out->axes[6]=p.dpad.xAxis.value; out->axes[7]=-p.dpad.yAxis.value;
}
// Called only by the GPUI main thread. Retain one controller until it disconnects.
int glidepod_gamepad_poll(GPGamepad *out) {
    @autoreleasepool {
        memset(out,0,sizeof(*out));
        if(![NSThread isMainThread]) return -1;
        GCController.shouldMonitorBackgroundEvents=NO;
        NSArray<GCController *> *controllers=GCController.controllers;
        if(selected && ![controllers containsObject:selected]) selected=nil;
        if(!selected) for(GCController *c in controllers) if(c.extendedGamepad) { selected=c; identity++; break; }
        if(!selected) return 0;
        out->identity=identity;
        readPad([selected.extendedGamepad capture],out);
        return 1;
    }
}
// Framework-owned synthetic snapshot: verifies mapping without claiming physical hardware input.
int glidepod_gamepad_mapping_probe(GPGamepad *out) {
    @autoreleasepool {
        memset(out,0,sizeof(*out));
        GCController *c=[GCController controllerWithExtendedGamepad];
        GCExtendedGamepad *p=c.extendedGamepad;
        [p.buttonA setValue:1]; [p.leftShoulder setValue:1];
        [p.leftThumbstick setValueForXAxis:0.5 yAxis:0.75];
        [p.rightTrigger setValue:0.6];
        [p.dpad setValueForXAxis:-1 yAxis:1];
        readPad(p,out); return 1;
    }
}
