#import <CoreLocation/CoreLocation.h>
#import <Foundation/Foundation.h>

// One owner, called only on its Rust worker thread. Coordinates never enter logs.
typedef struct {
    int state;
    double latitude, longitude, altitude, accuracy, timestamp;
} GPLocationSample;

@interface GPLocation : NSObject <CLLocationManagerDelegate>
@property(nonatomic, strong) CLLocationManager *manager;
@property(nonatomic, strong) CLLocation *fix;
@property(nonatomic) BOOL active;
@property(nonatomic) BOOL unavailable;
@end
@implementation GPLocation
- (void)locationManager:(CLLocationManager *)manager didUpdateLocations:(NSArray<CLLocation *> *)locations {
    (void)manager;
    CLLocation *fix = locations.lastObject;
    if (self.active && fix.horizontalAccuracy >= 0) { self.fix = fix; self.unavailable = NO; }
}
- (void)locationManager:(CLLocationManager *)manager didFailWithError:(NSError *)error {
    (void)manager; (void)error;
    self.unavailable = YES;
    self.fix = nil;
}
- (void)locationManagerDidChangeAuthorization:(CLLocationManager *)manager {
    if (manager.authorizationStatus != kCLAuthorizationStatusAuthorizedAlways) self.fix = nil;
}
@end

void *gp_location_create(void) {
    @autoreleasepool {
        GPLocation *owner = [GPLocation new];
        owner.manager = [CLLocationManager new];
        owner.manager.delegate = owner;
        owner.manager.desiredAccuracy = kCLLocationAccuracyHundredMeters;
        owner.manager.distanceFilter = 50;
        return (__bridge_retained void *)owner;
    }
}
int gp_location_poll(void *handle, int active, GPLocationSample *sample) {
    @autoreleasepool {
        GPLocation *owner = (__bridge GPLocation *)handle;
        *sample = (GPLocationSample){0};
        if (!active) {
            [owner.manager stopUpdatingLocation];
            owner.active = NO;
            owner.fix = nil;
            return 0;
        }
        if (![[NSBundle mainBundle] objectForInfoDictionaryKey:@"NSLocationUsageDescription"]) {
            sample->state = 6;
            return 0;
        }
        if (!owner.active) {
            owner.active = YES;
            owner.unavailable = NO;
            [owner.manager requestWhenInUseAuthorization];
            [owner.manager startUpdatingLocation];
        }
        [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.1]];
        CLAuthorizationStatus auth = owner.manager.authorizationStatus;
        if (auth == kCLAuthorizationStatusNotDetermined) sample->state = 1;
        else if (auth == kCLAuthorizationStatusDenied) sample->state = 2;
        else if (auth == kCLAuthorizationStatusRestricted) sample->state = 3;
        else if (![CLLocationManager locationServicesEnabled] || owner.unavailable) sample->state = 6;
        else if (!owner.fix || -owner.fix.timestamp.timeIntervalSinceNow > 60
                 || owner.fix.timestamp.timeIntervalSinceNow > 5) sample->state = 4;
        else {
            CLLocation *fix = owner.fix;
            sample->state = 5;
            sample->latitude = fix.coordinate.latitude;
            sample->longitude = fix.coordinate.longitude;
            sample->altitude = fix.verticalAccuracy >= 0 ? fix.altitude : 0;
            sample->accuracy = fix.horizontalAccuracy;
            sample->timestamp = fix.timestamp.timeIntervalSince1970;
        }
        return 0;
    }
}
void gp_location_destroy(void *handle) {
    @autoreleasepool {
        GPLocation *owner = (__bridge_transfer GPLocation *)handle;
        [owner.manager stopUpdatingLocation];
        owner.manager.delegate = nil;
    }
}
