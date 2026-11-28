// Fixture-owned bridge: emit a bounded breadcrumb in this app's native process log.
#import <React/RCTBridgeModule.h>
#import <os/log.h>

@interface AcmeBreadcrumbs : NSObject <RCTBridgeModule>
@end

@implementation AcmeBreadcrumbs

RCT_EXPORT_MODULE();

RCT_EXPORT_METHOD(mark:(NSString *)message)
{
  if (![message hasPrefix:@"Acme "] || message.length > 160) {
    [NSException raise:NSInvalidArgumentException format:@"Acme breadcrumbs accept bounded Acme messages only"];
  }
  static os_log_t log;
  static dispatch_once_t once;
  dispatch_once(&once, ^{ log = os_log_create("com.example.acme.diagnostics", "breadcrumbs"); });
  os_log(log, "%{public}@", message);
}

@end
