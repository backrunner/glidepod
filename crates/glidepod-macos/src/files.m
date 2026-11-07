#import <AppKit/AppKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#import <ImageIO/ImageIO.h>

static NSMutableDictionary<NSString *, NSSavePanel *> *panels;
static NSMutableArray<NSDictionary *> *results;
static NSMutableDictionary<NSString *, id> *delegates;

@interface GPFilePanelDelegate : NSObject <NSOpenSavePanelDelegate>
@property NSUInteger maximum;
@end
@implementation GPFilePanelDelegate
- (BOOL)panel:(id)sender validateURL:(NSURL *)url error:(NSError **)error {
    if([sender isKindOfClass:NSOpenPanel.class] && [(NSOpenPanel *)sender URLs].count > self.maximum) {
        if(error)*error=[NSError errorWithDomain:@"GlidePodFiles" code:1 userInfo:@{NSLocalizedDescriptionKey:[NSString stringWithFormat:@"Choose up to %ld files for this app.",(long)self.maximum]}];
        return NO;
    }
    return YES;
}
@end

// Bound decoded pixels before allocating an image. Android may copy JPEG/WebP
// content even though the Mac pasteboard's bitmap representation is PNG.
static NSData *pngData(NSData *input) {
    if(!input || input.length>32*1024*1024)return nil;
    CGImageSourceRef source=CGImageSourceCreateWithData((__bridge CFDataRef)input,NULL);
    if(!source)return nil;
    NSDictionary *properties=CFBridgingRelease(CGImageSourceCopyPropertiesAtIndex(source,0,NULL));
    uint64_t width=[properties[(__bridge NSString *)kCGImagePropertyPixelWidth] unsignedLongLongValue];
    uint64_t height=[properties[(__bridge NSString *)kCGImagePropertyPixelHeight] unsignedLongLongValue];
    if(!width||!height||width>16384||height>16384||width*height>16*1024*1024){CFRelease(source);return nil;}
    if(CGImageSourceGetType(source)&&CFEqual(CGImageSourceGetType(source),(__bridge CFStringRef)UTTypePNG.identifier)){CFRelease(source);return input;}
    NSMutableData *output=[NSMutableData new];
    CGImageDestinationRef destination=CGImageDestinationCreateWithData((__bridge CFMutableDataRef)output,(__bridge CFStringRef)UTTypePNG.identifier,1,NULL);
    BOOL ok=NO;
    if(destination){CGImageDestinationAddImageFromSource(destination,source,0,NULL);ok=CGImageDestinationFinalize(destination);CFRelease(destination);}
    CFRelease(source);
    return ok&&output.length<=32*1024*1024 ? output : nil;
}
static NSString *const originType = @"dev.glidepod.clipboard-origin";
static NSDictionary *failure(NSString *message) {return @{ @"error":message ?: @"Mac file operation failed" };}

static NSDictionary *pickerResult(NSString *token, NSModalResponse response, NSArray<NSURL *> *urls) {
    NSMutableArray *paths=[NSMutableArray new];
    if(response==NSModalResponseOK)for(NSURL *url in urls)if(url.isFileURL)[paths addObject:url.path];
    // Boxing a C comparison produces NSNumber(0/1), not a JSON boolean.
    // Rust deliberately distinguishes cancellation from an empty selection.
    return @{ @"id":token, @"paths":paths, @"cancel":response==NSModalResponseOK ? @NO : @YES };
}

static NSDictionary *clipboardRead(void) {
    NSPasteboard *p=NSPasteboard.generalPasteboard;
    NSInteger revision=p.changeCount;
    NSString *origin=[p stringForType:originType] ?: @"";
    NSNumber *sensitive=@([p.types containsObject:@"org.nspasteboard.ConcealedType"]);
    NSArray<NSURL *> *urls=[p readObjectsForClasses:@[NSURL.class] options:@{NSPasteboardURLReadingFileURLsOnlyKey:@YES}];
    if(urls.count) {
        if(urls.count>64)return failure(@"The clipboard contains more than 64 files.");
        NSMutableArray *paths=[NSMutableArray new];for(NSURL *u in urls)if(u.isFileURL)[paths addObject:u.path];
        return @{ @"kind":@"files", @"paths":paths, @"revision":@(revision), @"origin":origin, @"sensitive":sensitive };
    }
    NSString *text=[p stringForType:NSPasteboardTypeString];
    if(text) {
        if([text lengthOfBytesUsingEncoding:NSUTF8StringEncoding]>262144)return failure(@"Clipboard text exceeds 256 KiB.");
        return @{ @"kind":@"text", @"text":text, @"revision":@(revision), @"origin":origin, @"sensitive":sensitive };
    }
    NSData *source=[p dataForType:NSPasteboardTypePNG] ?: [p dataForType:NSPasteboardTypeTIFF];
    NSData *data=source ? pngData(source) : nil;
    if(source&&!data)return failure(@"Clipboard image is unsupported or too large (32 MiB / 16 megapixels).");
    if(data) {
        if(data.length>32*1024*1024)return failure(@"Clipboard image exceeds 32 MiB.");
        NSString *directory=[NSTemporaryDirectory() stringByAppendingPathComponent:@"glidepod-clipboard"];
        [[NSFileManager defaultManager] createDirectoryAtPath:directory withIntermediateDirectories:YES attributes:@{NSFilePosixPermissions:@0700} error:nil];
        NSString *path=[directory stringByAppendingPathComponent:[NSUUID.UUID.UUIDString stringByAppendingString:@".png"]];
        if(![data writeToFile:path options:NSDataWritingAtomic error:nil])return failure(@"Cannot prepare clipboard image.");
        [[NSFileManager defaultManager] setAttributes:@{NSFilePosixPermissions:@0600} ofItemAtPath:path error:nil];
        return @{ @"kind":@"image", @"paths":@[path], @"temporary":path, @"revision":@(revision), @"origin":origin, @"sensitive":sensitive };
    }
    return @{ @"kind":p.types.count ? @"unsupported" : @"empty", @"revision":@(revision), @"origin":origin, @"sensitive":sensitive };
}
static NSDictionary *run(NSDictionary *r) {
    NSString *op=r[@"op"];
    if([op isEqual:@"downloads"]) {
        NSURL *url=[[NSFileManager defaultManager] URLsForDirectory:NSDownloadsDirectory inDomains:NSUserDomainMask].firstObject;
        return url ? @{ @"path":url.path } : failure(@"Downloads folder is unavailable.");
    }
    if([op isEqual:@"mime"]) {UTType *type=[UTType typeWithFilenameExtension:r[@"extension"]];return @{ @"mime":type.preferredMIMEType ?: @"application/octet-stream" };}
    if([op isEqual:@"clipboard_status"])return @{ @"revision":@(NSPasteboard.generalPasteboard.changeCount) };
    if([op isEqual:@"clipboard_read"])return clipboardRead();
    if([op isEqual:@"clipboard_write"]) {
        NSPasteboard *p=NSPasteboard.generalPasteboard;
        if(r[@"expected"]&&p.changeCount!=[r[@"expected"] integerValue])return @{ @"stale":@YES };
        NSString *kind=r[@"kind"];
        if(![@[@"text",@"files",@"image",@"empty"] containsObject:kind])return failure(@"Unsupported clipboard format.");
        NSArray *paths=r[@"paths"];
        NSData *image=nil;
        if([kind isEqual:@"image"]) {image=pngData([NSData dataWithContentsOfFile:[paths firstObject] options:NSDataReadingMappedIfSafe error:nil]);if(!image)return failure(@"Clipboard image is unavailable.");}
        // Conversion can take time; do not overwrite a newer copy made meanwhile.
        if(r[@"expected"]&&p.changeCount!=[r[@"expected"] integerValue])return @{ @"stale":@YES };
        [p clearContents];
        if([kind isEqual:@"empty"])return @{ @"revision":@(p.changeCount) };
        if([kind isEqual:@"text"]) [p setString:r[@"text"] ?: @"" forType:NSPasteboardTypeString];
        else if([kind isEqual:@"image"]) [p setData:image forType:NSPasteboardTypePNG];
        else if([kind isEqual:@"files"]) {NSMutableArray *urls=[NSMutableArray new];for(NSString *path in paths)[urls addObject:[NSURL fileURLWithPath:path]];[p writeObjects:urls];}
        [p setString:r[@"origin"] ?: @"GlidePod" forType:originType];
        if([r[@"sensitive"] boolValue])[p setData:[NSData data] forType:@"org.nspasteboard.ConcealedType"];
        return @{ @"revision":@(p.changeCount) };
    }
    if(!NSThread.isMainThread)return failure(@"File panels require the main thread.");
    if(!panels){panels=[NSMutableDictionary new];results=[NSMutableArray new];delegates=[NSMutableDictionary new];}
    if([op isEqual:@"picker_results"]) {NSArray *copy=[results copy];[results removeAllObjects];return @{ @"items":copy };}
    NSString *token=r[@"id"];
    if([op isEqual:@"picker_result"]) {
        for(NSDictionary *item in [results copy])if([item[@"id"] isEqual:token]){[results removeObject:item];NSMutableDictionary *out=[item mutableCopy];out[@"ready"]=@YES;return out;}
        return @{ @"ready":@NO };
    }
    if([op isEqual:@"cancel_picker"]) {NSSavePanel *p=panels[token];[p cancel:nil];return @{};}
    if([op isEqual:@"pick"]) {
        if(panels[token])return @{};
        NSView *view=(__bridge NSView *)(void *)(uintptr_t)[r[@"view"] unsignedLongLongValue];
        NSWindow *owner=view.window;if(!owner)return failure(@"The app window has closed.");
        BOOL save=[r[@"kind"] isEqual:@"save"];
        NSSavePanel *panel=save ? NSSavePanel.savePanel : NSOpenPanel.openPanel;
        if(!save) {NSOpenPanel *open=(NSOpenPanel *)panel;open.canChooseFiles=YES;open.canChooseDirectories=NO;open.allowsMultipleSelection=[r[@"maximum"] integerValue]>1;}
        else panel.nameFieldStringValue=r[@"name"] ?: @"Untitled";
        NSMutableArray<UTType *> *types=[NSMutableArray new];BOOL any=NO;
        for(NSString *mime in r[@"mime_types"]) {
            if([mime isEqual:@"*/*"]){any=YES;break;}
            UTType *type=[mime isEqual:@"image/*"] ? UTTypeImage : [mime isEqual:@"video/*"] ? UTTypeMovie : [mime isEqual:@"audio/*"] ? UTTypeAudio : [UTType typeWithMIMEType:mime];
            if(type)[types addObject:type];
        }
        if(!any && types.count)panel.allowedContentTypes=types;
        panel.canCreateDirectories=save;panel.message=save ? @"Save a copy from Android. Existing files are kept; matching names receive a number." : @"Choose files for this Android app";
        if(save)panel.prompt=@"Save a Copy";
        GPFilePanelDelegate *delegate=[GPFilePanelDelegate new];delegate.maximum=MAX(1,MIN(64,[r[@"maximum"] integerValue]));panel.delegate=delegate;delegates[token]=delegate;
        panels[token]=panel;
        [panel beginSheetModalForWindow:owner completionHandler:^(NSModalResponse response) {
            NSArray<NSURL *> *urls=response==NSModalResponseOK ? (save ? @[panel.URL] : [(NSOpenPanel *)panel URLs]) : @[];
            if(results.count>=64)[results removeObjectAtIndex:0];[results addObject:pickerResult(token,response,urls)];[panels removeObjectForKey:token];[delegates removeObjectForKey:token];
        }];
        return @{};
    }
    return failure(@"Unknown Mac file operation.");
}
char *glidepod_files(const char *input) {
    @autoreleasepool {
        @try {
            NSData *data=[[NSString stringWithUTF8String:input] dataUsingEncoding:NSUTF8StringEncoding];
            NSDictionary *request=[NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
            NSDictionary *response=request ? run(request) : failure(@"Invalid file operation.");
            NSData *output=[NSJSONSerialization dataWithJSONObject:response options:0 error:nil];
            return strdup([[NSString alloc] initWithData:output encoding:NSUTF8StringEncoding].UTF8String);
        } @catch(NSException *e) {return strdup("{\"error\":\"Native file operation failed\"}");}
    }
}
void glidepod_files_free(char *response) {free(response);}
