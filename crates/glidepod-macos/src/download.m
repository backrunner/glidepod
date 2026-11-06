#import <Foundation/Foundation.h>
#include <stdbool.h>

@interface GPDownload : NSObject <NSURLSessionDownloadDelegate>
@property NSString *destination;
@property uint64_t expected;
@property(atomic) uint64_t received;
@property dispatch_semaphore_t done;
@property NSInteger result;
@end
@implementation GPDownload
- (void)URLSession:(NSURLSession *)session downloadTask:(NSURLSessionDownloadTask *)task didWriteData:(int64_t)bytes totalBytesWritten:(int64_t)total totalBytesExpectedToWrite:(int64_t)expected {
    self.received=MAX(total,0);
    if(total>(int64_t)self.expected || expected>(int64_t)self.expected)[task cancel];
}
- (void)URLSession:(NSURLSession *)session task:(NSURLSessionTask *)task willPerformHTTPRedirection:(NSHTTPURLResponse *)response newRequest:(NSURLRequest *)request completionHandler:(void (^)(NSURLRequest *))completion {
    completion([request.URL.scheme isEqualToString:@"https"]?request:nil);
}
- (void)URLSession:(NSURLSession *)session downloadTask:(NSURLSessionDownloadTask *)task didFinishDownloadingToURL:(NSURL *)location {
    NSHTTPURLResponse *response=(NSHTTPURLResponse *)task.response;
    NSNumber *size=nil; [location getResourceValue:&size forKey:NSURLFileSizeKey error:nil];
    if(response.statusCode!=200 || size.unsignedLongLongValue!=self.expected)return;
    NSError *error=nil;
    if([[NSFileManager defaultManager] moveItemAtURL:location toURL:[NSURL fileURLWithPath:self.destination] error:&error])self.result=0;
}
- (void)URLSession:(NSURLSession *)session task:(NSURLSessionTask *)task didCompleteWithError:(NSError *)error {
    if(error)self.result=1;
    dispatch_semaphore_signal(self.done);
}
@end
int glidepod_download(const char *url,const char *path,uint64_t bytes,bool (*callback)(uint64_t,void *),void *context) {
    @autoreleasepool {
        GPDownload *delegate=[GPDownload new];delegate.destination=[NSString stringWithUTF8String:path];delegate.expected=bytes;delegate.result=1;delegate.done=dispatch_semaphore_create(0);
        NSURLSessionConfiguration *configuration=NSURLSessionConfiguration.ephemeralSessionConfiguration;
        configuration.timeoutIntervalForRequest=30;configuration.timeoutIntervalForResource=1800;
        NSURLSession *session=[NSURLSession sessionWithConfiguration:configuration delegate:delegate delegateQueue:nil];
        NSURLSessionDownloadTask *task=[session downloadTaskWithURL:[NSURL URLWithString:[NSString stringWithUTF8String:url]]];
        [task resume];BOOL cancelled=NO;
        while(dispatch_semaphore_wait(delegate.done,dispatch_time(DISPATCH_TIME_NOW,100*NSEC_PER_MSEC))) {
            if(!cancelled && !callback(delegate.received,context)){cancelled=YES;[task cancel];}
        }
        [session finishTasksAndInvalidate];
        if(cancelled){[[NSFileManager defaultManager] removeItemAtPath:delegate.destination error:nil];return 2;}
        return (int)delegate.result;
    }
}
