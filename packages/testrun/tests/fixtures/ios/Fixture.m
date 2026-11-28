// A synthetic UIKit app for native Testrun acceptance. It is not a backend helper.
#import <UIKit/UIKit.h>

@interface CounterMarker : UIView
@property(nonatomic) NSUInteger count;
@end
@implementation CounterMarker
- (void)drawRect:(CGRect)rect {
    (void)rect; NSUInteger parity = 0;
    for (NSUInteger i = 0; i < 8; i++) {
        NSUInteger bit = i < 7 ? ((self.count >> i) & 1) : parity;
        if (i < 7) parity ^= bit;
        [(bit ? UIColor.whiteColor : UIColor.blackColor) setFill];
        UIRectFill(CGRectMake(i * self.bounds.size.width / 8, 0, self.bounds.size.width / 8, self.bounds.size.height));
    }
}
@end

@interface TouchSurface : UIView
@property(nonatomic,strong) UILabel *summary;
@property(nonatomic) NSUInteger down;
@property(nonatomic) NSUInteger moved;
@property(nonatomic) NSUInteger up;
@property(nonatomic) NSUInteger cancelled;
@property(nonatomic,copy) void (^changed)(void);
@end
@implementation TouchSurface
- (void)publish { self.summary.text = [NSString stringWithFormat:@"Touches down=%lu moved=%lu up=%lu cancelled=%lu",(unsigned long)self.down,(unsigned long)self.moved,(unsigned long)self.up,(unsigned long)self.cancelled];if (self.changed) self.changed(); }
- (void)touchesBegan:(NSSet<UITouch *> *)touches withEvent:(UIEvent *)event { (void)touches;(void)event;self.down++;[self publish]; }
- (void)touchesMoved:(NSSet<UITouch *> *)touches withEvent:(UIEvent *)event { (void)touches;(void)event;self.moved++;[self publish]; }
- (void)touchesEnded:(NSSet<UITouch *> *)touches withEvent:(UIEvent *)event { (void)touches;(void)event;self.up++;[self publish]; }
- (void)touchesCancelled:(NSSet<UITouch *> *)touches withEvent:(UIEvent *)event { (void)touches;(void)event;self.cancelled++;[self publish]; }
@end

@interface Fixture : UIViewController
@property(nonatomic) NSUInteger count;
@property(nonatomic,strong) UILabel *counter;
@property(nonatomic,strong) CounterMarker *marker;
@property(nonatomic,strong) TouchSurface *surface;
@property(nonatomic,strong) NSDictionary *lastState;
@property(nonatomic) NSUInteger command;
@end
@implementation Fixture
- (NSString *)documents { return NSSearchPathForDirectoriesInDomains(NSDocumentDirectory,NSUserDomainMask,YES).firstObject; }
- (void)publishState {
    CGRect marker = [self.marker convertRect:self.marker.bounds toView:nil];
    NSDictionary *state = @{@"count":@(self.count),@"command":@(self.command),@"down":@(self.surface.down),@"up":@(self.surface.up),@"cancelled":@(self.surface.cancelled),@"move":@(self.surface.moved),@"marker":@{@"x":@(marker.origin.x),@"y":@(marker.origin.y),@"width":@(marker.size.width),@"height":@(marker.size.height)},@"width":@(self.view.bounds.size.width),@"height":@(self.view.bounds.size.height)};
    if (![state isEqual:self.lastState]) {
        NSData *data = [NSJSONSerialization dataWithJSONObject:state options:0 error:nil];
        if ([data writeToFile:[[self documents] stringByAppendingPathComponent:@"state.json"] atomically:YES]) self.lastState = state;
    }
}
- (void)showCount {
    self.counter.text = [NSString stringWithFormat:@"Acme count: %lu",(unsigned long)self.count];
    self.marker.count = self.count;[self.marker setNeedsDisplay];[self publishState];
}
- (void)increment { self.count++;[self showCount]; }
- (void)viewDidLayoutSubviews { [super viewDidLayoutSubviews];[self publishState]; }
- (void)viewDidLoad {
    [super viewDidLoad];self.view.backgroundColor = UIColor.systemBackgroundColor;
    UIStackView *stack = [[UIStackView alloc] init];stack.axis = UILayoutConstraintAxisVertical;stack.spacing = 12;stack.translatesAutoresizingMaskIntoConstraints = NO;
    [self.view addSubview:stack];
    [NSLayoutConstraint activateConstraints:@[[stack.topAnchor constraintEqualToAnchor:self.view.safeAreaLayoutGuide.topAnchor constant:20],[stack.leadingAnchor constraintEqualToAnchor:self.view.leadingAnchor constant:20],[stack.trailingAnchor constraintEqualToAnchor:self.view.trailingAnchor constant:-20]]];
    UILabel *title = [UILabel new];title.text = @"Testrun QA fixture";title.accessibilityIdentifier = @"qa.title";[stack addArrangedSubview:title];
    if ([NSProcessInfo.processInfo.arguments containsObject:@"--continuous-animation"]) {
        [UIView animateWithDuration:0.5 delay:0
                           options:UIViewAnimationOptionRepeat | UIViewAnimationOptionAutoreverse | UIViewAnimationOptionAllowUserInteraction
                        animations:^{ title.alpha = 0.3; } completion:nil];
    }
    self.counter = [UILabel new];self.counter.text = @"Acme count: 0";self.counter.accessibilityIdentifier = @"qa.counter";[stack addArrangedSubview:self.counter];
    self.marker = [CounterMarker new];self.marker.accessibilityIdentifier = @"qa.counter-marker";[self.marker.heightAnchor constraintEqualToConstant:32].active = YES;[stack addArrangedSubview:self.marker];
    UIButton *button = [UIButton buttonWithType:UIButtonTypeSystem];[button setTitle:@"Increment Acme counter" forState:UIControlStateNormal];button.accessibilityIdentifier = @"qa.increment";[button addTarget:self action:@selector(increment) forControlEvents:UIControlEventTouchUpInside];[stack addArrangedSubview:button];
    UITextField *text = [UITextField new];text.text = @"Acme contact";text.borderStyle = UITextBorderStyleRoundedRect;text.accessibilityLabel = @"Contact";text.accessibilityIdentifier = @"qa.contact";[stack addArrangedSubview:text];
    UITextField *secret = [UITextField new];secret.secureTextEntry = YES;secret.text = @"fixture-secret-example";secret.borderStyle = UITextBorderStyleRoundedRect;secret.accessibilityLabel = @"Password";secret.accessibilityIdentifier = @"qa.password";[stack addArrangedSubview:secret];
    UILabel *summary = [UILabel new];summary.accessibilityIdentifier = @"qa.touch-summary";summary.numberOfLines = 0;[stack addArrangedSubview:summary];
    TouchSurface *surface = [TouchSurface new];surface.backgroundColor = UIColor.systemBlueColor;surface.accessibilityIdentifier = @"qa.touch-surface";surface.accessibilityLabel = @"Touch surface";surface.isAccessibilityElement = YES;surface.multipleTouchEnabled = YES;surface.summary = summary;[surface publish];[surface.heightAnchor constraintEqualToConstant:180].active = YES;[stack addArrangedSubview:surface];
    self.surface = surface;__weak Fixture *weak = self;surface.changed = ^{[weak publishState];};
    UIScrollView *scroll = [UIScrollView new];scroll.accessibilityIdentifier = @"qa.scroll";[scroll.heightAnchor constraintEqualToConstant:170].active = YES;[stack addArrangedSubview:scroll];
    UIStackView *rows = [UIStackView new];rows.axis = UILayoutConstraintAxisVertical;rows.spacing = 10;rows.translatesAutoresizingMaskIntoConstraints = NO;[scroll addSubview:rows];
    [NSLayoutConstraint activateConstraints:@[[rows.topAnchor constraintEqualToAnchor:scroll.contentLayoutGuide.topAnchor],[rows.bottomAnchor constraintEqualToAnchor:scroll.contentLayoutGuide.bottomAnchor],[rows.leadingAnchor constraintEqualToAnchor:scroll.contentLayoutGuide.leadingAnchor],[rows.trailingAnchor constraintEqualToAnchor:scroll.contentLayoutGuide.trailingAnchor],[rows.widthAnchor constraintEqualToAnchor:scroll.frameLayoutGuide.widthAnchor]]];
    for (NSUInteger index = 0;index < 30;index++) { UILabel *row = [UILabel new];row.text = [NSString stringWithFormat:@"Acme row %lu",(unsigned long)index];row.accessibilityIdentifier = [NSString stringWithFormat:@"qa.row.%lu",(unsigned long)index];[row.heightAnchor constraintEqualToConstant:30].active = YES;[rows addArrangedSubview:row]; }
    [NSTimer scheduledTimerWithTimeInterval:0.02 repeats:YES block:^(NSTimer *timer) {
        (void)timer;Fixture *value = weak;if (!value) return;
        NSData *data = [NSData dataWithContentsOfFile:[[value documents] stringByAppendingPathComponent:@"command.json"]];
        NSDictionary *command = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
        if ([command isKindOfClass:NSDictionary.class] && [command[@"seq"] isKindOfClass:NSNumber.class] && [command[@"count"] isKindOfClass:NSNumber.class] && [command[@"seq"] unsignedIntegerValue] > value.command && [command[@"count"] unsignedIntegerValue] < 128) {
            value.command = [command[@"seq"] unsignedIntegerValue];value.count = [command[@"count"] unsignedIntegerValue];[value showCount];
        }
    }];
}
@end
@interface Delegate : UIResponder<UIApplicationDelegate>
@property(nonatomic,strong) UIWindow *window;
@end
@implementation Delegate
- (BOOL)application:(UIApplication *)application didFinishLaunchingWithOptions:(NSDictionary *)options {
    (void)application;(void)options;self.window = [[UIWindow alloc] initWithFrame:UIScreen.mainScreen.bounds];self.window.rootViewController = [Fixture new];[self.window makeKeyAndVisible];return YES;
}
@end
int main(int argc,char **argv) { @autoreleasepool { return UIApplicationMain(argc,argv,nil,NSStringFromClass(Delegate.class)); } }
