// Run by tools/check-files-native.py. All pasteboard calls use a unique private
// board in this process; the user's general clipboard is never read or changed.
#import <objc/runtime.h>
#import "../src/files.m"
static NSPasteboard *isolated;
static NSPasteboard *privateBoard(id self, SEL selector) { return isolated; }
static void require(BOOL passed, NSString *message) {
    if(!passed){fprintf(stderr,"%s\n",message.UTF8String);exit(1);}
}
static NSDictionary *call(NSDictionary *request) {
    NSData *data=[NSJSONSerialization dataWithJSONObject:request options:0 error:nil];
    NSString *input=[[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    char *raw=glidepod_files(input.UTF8String);
    NSData *output=[[NSString stringWithUTF8String:raw] dataUsingEncoding:NSUTF8StringEncoding];
    glidepod_files_free(raw);
    return [NSJSONSerialization JSONObjectWithData:output options:0 error:nil];
}
int main(void) {
    @autoreleasepool {
        isolated=[NSPasteboard pasteboardWithUniqueName];
        Method method=class_getClassMethod(NSPasteboard.class,@selector(generalPasteboard));
        IMP original=method_setImplementation(method,(IMP)privateBoard);
        NSDictionary *written=call(@{@"op":@"clipboard_write",@"kind":@"text",@"text":@"中文 😀",@"origin":@"fixture",@"sensitive":@YES});
        NSDictionary *read=call(@{@"op":@"clipboard_read"});
        require([read[@"text"] isEqual:@"中文 😀"]&&[read[@"sensitive"] boolValue],@"Text or sensitive marker did not round trip");
        require([call(@{@"op":@"clipboard_write",@"kind":@"text",@"text":@"obsolete",@"expected":@(-1)})[@"stale"] boolValue],@"Stale writer was accepted");
        require([call(@{@"op":@"clipboard_status"})[@"revision"] isEqual:written[@"revision"]],@"Stale writer changed pasteboard");
        call(@{@"op":@"clipboard_write",@"kind":@"empty"});
        require([call(@{@"op":@"clipboard_read"})[@"kind"] isEqual:@"empty"],@"Clearing left an unsupported origin-only clipboard");

        NSString *folder=[NSTemporaryDirectory() stringByAppendingPathComponent:NSUUID.UUID.UUIDString];
        [[NSFileManager defaultManager] createDirectoryAtPath:folder withIntermediateDirectories:YES attributes:nil error:nil];
        NSArray *paths=@[[folder stringByAppendingPathComponent:@"中文.txt"],[folder stringByAppendingPathComponent:@"two files.txt"]];
        for(NSString *path in paths)[@"fixture" writeToFile:path atomically:YES encoding:NSUTF8StringEncoding error:nil];
        call(@{@"op":@"clipboard_write",@"kind":@"files",@"paths":paths});
        require([call(@{@"op":@"clipboard_read"})[@"paths"] isEqual:paths],@"File URLs lost their names/order");

        NSBitmapImageRep *bitmap=[[NSBitmapImageRep alloc] initWithBitmapDataPlanes:NULL pixelsWide:2 pixelsHigh:2 bitsPerSample:8 samplesPerPixel:3 hasAlpha:NO isPlanar:NO colorSpaceName:NSDeviceRGBColorSpace bytesPerRow:0 bitsPerPixel:0];
        memset(bitmap.bitmapData,180,bitmap.bytesPerRow*2);
        NSString *jpeg=[folder stringByAppendingPathComponent:@"image.jpg"];
        [[bitmap representationUsingType:NSBitmapImageFileTypeJPEG properties:@{}] writeToFile:jpeg atomically:YES];
        require(call(@{@"op":@"clipboard_write",@"kind":@"image",@"paths":@[jpeg]})[@"error"]==nil,@"JPEG clipboard import failed");
        read=call(@{@"op":@"clipboard_read"});
        require([read[@"kind"] isEqual:@"image"],@"JPEG was not materialized as a Mac bitmap");
        CGImageSourceRef image=CGImageSourceCreateWithURL((__bridge CFURLRef)[NSURL fileURLWithPath:read[@"temporary"]],NULL);
        require(image&&CFEqual(CGImageSourceGetType(image),(__bridge CFStringRef)UTTypePNG.identifier),@"Clipboard bitmap was not valid PNG");
        CFRelease(image);[[NSFileManager defaultManager] removeItemAtPath:read[@"temporary"] error:nil];
        NSNumber *revision=call(@{@"op":@"clipboard_status"})[@"revision"];
        require(call(@{@"op":@"clipboard_write",@"kind":@"image",@"paths":@[paths[0]]})[@"error"]!=nil,@"Malformed image was accepted");
        require(call(@{@"op":@"clipboard_write",@"kind":@"unsupported"})[@"error"]!=nil,@"Unknown format was accepted");
        require([call(@{@"op":@"clipboard_status"})[@"revision"] isEqual:revision],@"Invalid content destroyed the previous clipboard");
        [[NSFileManager defaultManager] removeItemAtPath:folder error:nil];
        [isolated releaseGlobally];method_setImplementation(method,original);
        puts("Passed: private native pasteboard text, sensitive marker, stale revision, empty, file URLs, JPEG to PNG, and invalid-content preservation. Native panel interactions are not covered.");
    }
    return 0;
}
