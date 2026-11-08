// Read-only system proxy inventory. Never fetch PAC, open a connection or change preferences.
#import <Foundation/Foundation.h>
#import <CFNetwork/CFNetwork.h>
#import <vmnet/vmnet.h>
#include <stdlib.h>
#include <string.h>

static BOOL enabled(NSDictionary *settings, CFStringRef key) {
    id value = settings[(__bridge NSString *)key];
    return [value isKindOfClass:[NSNumber class]] && [value boolValue];
}

static NSNumber *jsonBool(BOOL value) {
    return [NSNumber numberWithBool:value];
}

static NSDictionary *endpoint(NSDictionary *settings, CFStringRef enableKey,
                              CFStringRef hostKey, CFStringRef portKey) {
    id host = settings[(__bridge NSString *)hostKey];
    id port = settings[(__bridge NSString *)portKey];
    return @{
        @"enabled": jsonBool(enabled(settings, enableKey)),
        @"host": [host isKindOfClass:[NSString class]] ? host : [NSNull null],
        @"port": [port isKindOfClass:[NSNumber class]] ? port : [NSNull null],
    };
}

char *glidepod_system_proxy_snapshot(void) {
    @autoreleasepool {
        NSDictionary *settings = CFBridgingRelease(CFNetworkCopySystemProxySettings());
        if (!settings) return NULL;
        id exceptions = settings[(__bridge NSString *)kCFNetworkProxiesExceptionsList];
        // Allowlist only diagnostic fields. PAC URLs may contain credentials or tokens;
        // never serialize them, PAC script bodies, usernames or passwords.
        NSDictionary *report = @{
            @"http": endpoint(settings, kCFNetworkProxiesHTTPEnable,
                              kCFNetworkProxiesHTTPProxy, kCFNetworkProxiesHTTPPort),
            @"https": endpoint(settings, kCFNetworkProxiesHTTPSEnable,
                               kCFNetworkProxiesHTTPSProxy, kCFNetworkProxiesHTTPSPort),
            @"socks": endpoint(settings, kCFNetworkProxiesSOCKSEnable,
                               kCFNetworkProxiesSOCKSProxy, kCFNetworkProxiesSOCKSPort),
            @"pac_enabled": jsonBool(enabled(settings, kCFNetworkProxiesProxyAutoConfigEnable)),
            @"pac_url_present": jsonBool(settings[(__bridge NSString *)kCFNetworkProxiesProxyAutoConfigURLString] != nil),
            @"auto_discovery_enabled": jsonBool(enabled(settings, kCFNetworkProxiesProxyAutoDiscoveryEnable)),
            @"exclude_simple_hostnames": jsonBool(enabled(settings, kCFNetworkProxiesExcludeSimpleHostnames)),
            @"bypass_rule_count": @([exceptions isKindOfClass:[NSArray class]] ? [exceptions count] : 0),
        };
        NSData *json = [NSJSONSerialization dataWithJSONObject:report options:0 error:nil];
        if (!json || json.length > 65536) return NULL;
        char *result = malloc(json.length + 1);
        if (!result) return NULL;
        memcpy(result, json.bytes, json.length);
        result[json.length] = '\0';
        return result;
    }
}

void glidepod_system_proxy_snapshot_free(char *result) {
    free(result);
}

char *glidepod_bridge_interfaces(void) {
    @autoreleasepool {
        xpc_object_t interfaces = vmnet_copy_shared_interface_list();
        NSMutableArray *names = [NSMutableArray new];
        if (interfaces && xpc_get_type(interfaces) == XPC_TYPE_ARRAY) {
            size_t count = MIN(xpc_array_get_count(interfaces), 64);
            for (size_t i = 0; i < count; i++) {
                const char *name = xpc_array_get_string(interfaces, i);
                if (name) { NSString *text = [NSString stringWithUTF8String:name]; if (text) [names addObject:text]; }
            }
        }
        NSData *json = [NSJSONSerialization dataWithJSONObject:names options:0 error:nil];
        if (!json || json.length > 8192) return NULL;
        char *result = malloc(json.length + 1);
        if (!result) return NULL;
        memcpy(result, json.bytes, json.length); result[json.length] = '\0';
        return result;
    }
}
