#import <Foundation/Foundation.h>
#import <CoreServices/CoreServices.h>

int32_t glidepod_register_application(const char *path) {
    @autoreleasepool {
        NSURL *url = [NSURL fileURLWithFileSystemRepresentation:path isDirectory:YES relativeToURL:nil];
        return LSRegisterURL((__bridge CFURLRef)url, true);
    }
}
