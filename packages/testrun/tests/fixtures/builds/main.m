#import <UIKit/UIKit.h>

@interface AcmeDelegate : UIResponder <UIApplicationDelegate>
@property(nonatomic, strong) UIWindow *window;
@end
@implementation AcmeDelegate
- (BOOL)application:(UIApplication *)application didFinishLaunchingWithOptions:(NSDictionary *)options {
    NSURL *documents = [NSFileManager.defaultManager URLsForDirectory:NSDocumentDirectory inDomains:NSUserDomainMask].firstObject;
    NSURL *record = [documents URLByAppendingPathComponent:@"build-acceptance.json"];
    NSData *saved = [NSData dataWithContentsOfURL:record];
    NSDictionary *previous = saved ? [NSJSONSerialization JSONObjectWithData:saved options:0 error:nil] : nil;
    NSString *marker = previous[@"marker"] ?: NSUUID.UUID.UUIDString;
    NSInteger count = [previous[@"count"] integerValue] + 1;
    NSDictionary *evidence = @{@"marker":marker, @"count":@(count), @"apiUrl":NSProcessInfo.processInfo.environment[@"API_URL"] ?: @"", @"argument":@([NSProcessInfo.processInfo.arguments containsObject:@"--acme-qa"])};
    [[NSJSONSerialization dataWithJSONObject:evidence options:0 error:nil] writeToURL:record atomically:YES];
    self.window = [[UIWindow alloc] initWithFrame:UIScreen.mainScreen.bounds];
    UIViewController *controller = [UIViewController new];
    controller.view.backgroundColor = UIColor.systemBackgroundColor;
    UILabel *label = [[UILabel alloc] initWithFrame:CGRectMake(20, 100, 320, 80)];
    label.text = @"Acme build acceptance";
    label.accessibilityIdentifier = @"acme-build-ready";
    [controller.view addSubview:label];
    self.window.rootViewController = controller;
    [self.window makeKeyAndVisible];
    return YES;
}
@end
int main(int argc, char *argv[]) {
    @autoreleasepool { return UIApplicationMain(argc, argv, nil, NSStringFromClass(AcmeDelegate.class)); }
}
