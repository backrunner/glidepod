#import <Foundation/Foundation.h>
#import <UserNotifications/UserNotifications.h>
#include <stdlib.h>
#include <string.h>

static NSMutableArray *events;
static NSLock *eventLock;
static void enqueue(NSDictionary *event) {
    [eventLock lock];if(events.count<256)[events addObject:event];[eventLock unlock];
}
@interface GPNotificationDelegate : NSObject <UNUserNotificationCenterDelegate>
@end
@implementation GPNotificationDelegate
- (void)userNotificationCenter:(UNUserNotificationCenter *)center willPresentNotification:(UNNotification *)notification withCompletionHandler:(void (^)(UNNotificationPresentationOptions))completion {
    completion([notification.request.content.userInfo[@"alerting"] boolValue] ? (UNNotificationPresentationOptionBanner|UNNotificationPresentationOptionList) : UNNotificationPresentationOptionList);
}
- (void)userNotificationCenter:(UNUserNotificationCenter *)center didReceiveNotificationResponse:(UNNotificationResponse *)response withCompletionHandler:(void (^)(void))completion {
    NSMutableDictionary *event=[response.notification.request.content.userInfo mutableCopy];
    event[@"kind"]=[response.actionIdentifier isEqual:UNNotificationDismissActionIdentifier]?@"dismiss":@"open";
    event[@"id"]=response.notification.request.identifier;enqueue(event);completion();
}
@end
static GPNotificationDelegate *delegate;
static UNUserNotificationCenter *center;
static void initialize(void) {
    static dispatch_once_t once;
    dispatch_once(&once,^{
        events=[NSMutableArray array];eventLock=[NSLock new];delegate=[GPNotificationDelegate new];
        center=UNUserNotificationCenter.currentNotificationCenter;center.delegate=delegate;
        UNNotificationCategory *category=[UNNotificationCategory categoryWithIdentifier:@"GLIDEPOD_APP" actions:@[] intentIdentifiers:@[] options:UNNotificationCategoryOptionCustomDismissAction];
        [center setNotificationCategories:[NSSet setWithObject:category]];
    });
}
char *glidepod_notifications(const char *json) {
    @autoreleasepool { @try {
        initialize();NSError *error=nil;
        NSDictionary *input=[NSJSONSerialization JSONObjectWithData:[[NSString stringWithUTF8String:json] dataUsingEncoding:NSUTF8StringEncoding] options:0 error:&error];
        if(!input) return strdup("{\"error\":\"Invalid native notification request\"}");
        NSString *op=input[@"op"];id result=@{@"ok":@YES};
        if([op isEqual:@"status"]) {
            // Called from the bridge worker, never from the AppKit event thread.
            if(NSThread.isMainThread)return strdup("{\"error\":\"Notification status requires a worker thread\"}");
            dispatch_semaphore_t ready=dispatch_semaphore_create(0);__block NSInteger authorization=-1;
            [center getNotificationSettingsWithCompletionHandler:^(UNNotificationSettings *s){authorization=s.authorizationStatus;dispatch_semaphore_signal(ready);}];
            if(dispatch_semaphore_wait(ready,dispatch_time(DISPATCH_TIME_NOW,5*NSEC_PER_SEC)))result=@{@"error":@"Notification settings timed out"};
            else result=@{@"authorization":@(authorization)};
        } else if([op isEqual:@"request"]) {
            [center requestAuthorizationWithOptions:UNAuthorizationOptionAlert completionHandler:^(BOOL granted,NSError *e){enqueue(@{@"kind":@"authorization",@"granted":@(granted)});if(e)enqueue(@{@"kind":@"error",@"message":e.localizedDescription});}];
        } else if([op isEqual:@"show"]) {
            UNMutableNotificationContent *content=[UNMutableNotificationContent new];
            content.title=input[@"app_label"];content.subtitle=input[@"title"];content.body=input[@"body"];
            content.categoryIdentifier=@"GLIDEPOD_APP";content.threadIdentifier=input[@"package_id"];
            content.interruptionLevel=[input[@"alerting"] boolValue]?UNNotificationInterruptionLevelActive:UNNotificationInterruptionLevelPassive;
            content.userInfo=@{@"package_id":input[@"package_id"],@"epoch":input[@"epoch"],@"key":input[@"key"],@"clearable":input[@"clearable"],@"alerting":input[@"alerting"]};
            UNNotificationRequest *request=[UNNotificationRequest requestWithIdentifier:input[@"id"] content:content trigger:nil];
            [center addNotificationRequest:request withCompletionHandler:^(NSError *e){if(e)enqueue(@{@"kind":@"error",@"message":e.localizedDescription,@"id":request.identifier});}];
        } else if([op isEqual:@"remove"]) {
            [center removePendingNotificationRequestsWithIdentifiers:input[@"ids"]];[center removeDeliveredNotificationsWithIdentifiers:input[@"ids"]];
        } else if([op isEqual:@"clear"]) {
            [center removeAllPendingNotificationRequests];[center removeAllDeliveredNotifications];
        } else if([op isEqual:@"events"]) {
            [eventLock lock];result=[events copy];[events removeAllObjects];[eventLock unlock];
        } else result=@{@"error":@"Unknown native notification operation"};
        NSData *data=[NSJSONSerialization dataWithJSONObject:result options:0 error:&error];
        return strdup(data?[[[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding] UTF8String]:"{\"error\":\"Native notification encoding failed\"}");
    } @catch(NSException *e) {
        NSData *data=[NSJSONSerialization dataWithJSONObject:@{@"error":e.reason?:@"Native notification failure"} options:0 error:nil];
        return strdup([[[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding] UTF8String]);
    }}
}
void glidepod_notifications_free(char *value){free(value);}
