#import "ConcreteTap.h"
#import "RunnerIssueCase.h"
#import <XCTest/XCTest.h>
#import <dispatch/dispatch.h>

static NSInteger mode, calls;
static void Check(BOOL value, const char *name);
@implementation XCUIElement
- (XCUIElementQuery *)descendantsMatchingType:(XCUIElementType)type {
    @throw [NSException exceptionWithName:@"FixtureQueryUnavailable" reason:nil userInfo:nil];
}
@end
@implementation XCUIElementQuery
- (XCUIElementQuery *)matchingPredicate:(NSPredicate *)predicate {
    XCUIElementQuery *query = [XCUIElementQuery new];
    query.elements = [self.elements filteredArrayUsingPredicate:predicate];
    query.filtered = YES;
    return query;
}
- (NSArray<XCUIElement *> *)allElementsBoundByIndex {
    Check(self.filtered, "focus filters the native query before materializing descendants");
    return self.elements;
}
@end
@interface FocusApplication : XCUIElement
@property NSArray<XCUIElement *> *elements;
@end
@implementation FocusApplication
- (XCUIElementQuery *)descendantsMatchingType:(XCUIElementType)type {
    Check(type == XCUIElementTypeAny, "editable query covers all descendant containers");
    XCUIElementQuery *query = [XCUIElementQuery new]; query.elements = self.elements; return query;
}
@end
@interface UnsupportedFocus : XCUIElement
@end
@implementation UnsupportedFocus
- (id)valueForKey:(NSString *)key {
    if ([key isEqualToString:@"hasKeyboardFocus"]) {
        @throw [NSException exceptionWithName:@"UnsupportedFocus" reason:nil userInfo:nil];
    }
    return [super valueForKey:key];
}
@end
@implementation XCTIssue
@end
@implementation XCTestCase {
    NSUInteger _recordedIssues;
}
- (NSUInteger)recordedIssues { return _recordedIssues; }
- (void)recordIssue:(XCTIssue *)issue { ++_recordedIssues; }
@end
static BOOL incompatible;
static long long orientation = 1;
static int ownerProcess = 7;
static BOOL missingOwner, ownerFailure, restarted, ownerTimeout;
@interface AccessibilityElement : NSObject
@end
@implementation AccessibilityElement
- (int)processIdentifier { return ownerProcess; }
@end
@interface Accessibility : NSObject
@end
@implementation Accessibility
- (void)_XCT_requestElementAtPoint:(CGPoint)point reply:(void (^)(id, NSError *))reply {
    Check(point.x == 12 && point.y == 24, "global ownership uses exact logical point");
    if (ownerTimeout) return;
    if (restarted) ownerProcess = 8;
    reply(missingOwner ? nil : [AccessibilityElement new], ownerFailure ? [NSError errorWithDomain:@"Fixture" code:1 userInfo:nil] : nil);
}
@end
@interface Snapshot : NSObject
@property CGRect frame;
@property CGRect visibleFrame;
@property unsigned long long elementType;
@property NSArray *children;
@end
@implementation Snapshot
@end
@interface SnapshotSource : NSObject
@property Snapshot *snapshot;
@end
@implementation SnapshotSource
- (id)snapshotWithError:(NSError **)error { return self.snapshot; }
- (int)processID { return restarted && ownerProcess == 8 ? 8 : 7; }
@end
static void Check(BOOL value, const char *name) {
    if (!value) { fprintf(stderr, "FAIL: %s\n", name); exit(1); }
}
@interface Application : NSObject
@end
@implementation Application
- (long long)interfaceOrientation { return orientation; }
@end
static NSInteger idleCalls;
@interface IdleProcess : NSObject
- (void)waitForQuiescenceIncludingAnimationsIdle:(bool)animations;
- (void)waitForQuiescenceIncludingAnimationsIdle:(bool)animations isPreEvent:(bool)preEvent;
@end
@implementation IdleProcess
- (void)waitForQuiescenceIncludingAnimationsIdle:(bool)animations { ++idleCalls; }
- (void)waitForQuiescenceIncludingAnimationsIdle:(bool)animations isPreEvent:(bool)preEvent { ++idleCalls; }
@end
@interface InheritedIdleProcess : IdleProcess
@end
@implementation InheritedIdleProcess
@end
@interface LegacyIdleProcess : NSObject
- (void)waitForQuiescenceIncludingAnimationsIdle:(bool)animations;
@end
@implementation LegacyIdleProcess
- (void)waitForQuiescenceIncludingAnimationsIdle:(bool)animations { ++idleCalls; }
@end
@interface IncompatibleIdleProcess : NSObject
- (void)waitForQuiescenceIncludingAnimationsIdle:(bool)animations;
- (BOOL)waitForQuiescenceIncludingAnimationsIdle:(bool)animations isPreEvent:(bool)preEvent;
@end
@implementation IncompatibleIdleProcess
- (void)waitForQuiescenceIncludingAnimationsIdle:(bool)animations { ++idleCalls; }
- (BOOL)waitForQuiescenceIncludingAnimationsIdle:(bool)animations isPreEvent:(bool)preEvent { return YES; }
@end
@interface XCPointerEventPath : NSObject
@end
@implementation XCPointerEventPath
- (id)initForTouchAtPoint:(CGPoint)point offset:(double)offset {
    Check(point.x == 12 && point.y == 24 && offset == 0, "logical touch point");
    return [super init];
}
- (void)liftUpAtOffset:(double)offset { Check(offset == 0.05, "complete 50 ms contact"); }
@end
@interface XCSynthesizedEventRecord : NSObject
@end
@implementation XCSynthesizedEventRecord
- (id)initWithName:(NSString *)name interfaceOrientation:(long long)value {
    Check(value == 1, "orientation"); return [super init];
}
- (void)addPointerEventPath:(id)path { Check(path != nil, "event owns contact"); }
@end
@interface Synthesizer : NSObject
@end
@implementation Synthesizer
- (id)daemonProxy { return [Accessibility new]; }
- (NSMethodSignature *)methodSignatureForSelector:(SEL)selector {
    if (incompatible && selector == @selector(synthesizeEvent:completion:))
        return [NSMethodSignature signatureWithObjCTypes:"v@:@@?"];
    return [super methodSignatureForSelector:selector];
}
- (id)synthesizeEvent:(id)record completion:(void (^)(_Bool, NSError *))completion {
    ++calls;
    if (mode == 0) completion(YES, nil);
    if (mode == 1) completion(NO, nil);
    if (mode == 2) completion(YES, [NSError errorWithDomain:@"Fixture" code:1 userInfo:nil]);
    if (mode == 4) @throw [NSException exceptionWithName:@"Fixture" reason:nil userInfo:nil];
    if (mode == 5) {
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0), ^{ completion(YES, nil); completion(NO, nil); });
    }
    if (mode == 6) {
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 30 * NSEC_PER_MSEC), dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0), ^{ completion(YES, nil); });
    }
    return nil; // A nil submission object is not a failed acknowledgement.
}
@end
@implementation XCUIDevice
+ (XCUIDevice *)sharedDevice { static XCUIDevice *device; if (!device) device = [self new]; return device; }
- (id)eventSynthesizer { return [Synthesizer new]; }
@end

int main(void) { @autoreleasepool {
    NSError *idleError = nil;
    Check(!SBInstallNonWaitingQuiescenceForClass(Nil, &idleError),
          "missing XCTest process class refuses readiness");
    Check(!SBInstallNonWaitingQuiescenceForClass(NSObject.class, &idleError) && idleError.code == 1,
          "missing idle runtime refuses readiness");
    Check(!SBInstallNonWaitingQuiescenceForClass(IncompatibleIdleProcess.class, &idleError),
          "incompatible idle ABI refuses readiness");
    [[IncompatibleIdleProcess new] waitForQuiescenceIncludingAnimationsIdle:YES];
    Check(idleCalls == 1, "ABI validation cannot partially replace idle methods");
    Check(SBInstallNonWaitingQuiescenceForClass(InheritedIdleProcess.class, &idleError),
          "inherited idle methods are supported");
    [[InheritedIdleProcess new] waitForQuiescenceIncludingAnimationsIdle:YES];
    [[InheritedIdleProcess new] waitForQuiescenceIncludingAnimationsIdle:YES isPreEvent:YES];
    Check(idleCalls == 1, "both idle entry points skip implicit animation waits");
    [[IdleProcess new] waitForQuiescenceIncludingAnimationsIdle:YES];
    Check(idleCalls == 2, "installation leaves the superclass unchanged");
    Check(SBInstallNonWaitingQuiescenceForClass(LegacyIdleProcess.class, &idleError),
          "legacy idle method is supported");
    [[LegacyIdleProcess new] waitForQuiescenceIncludingAnimationsIdle:YES];
    Check(idleCalls == 2, "legacy operations also skip implicit animation waits");
    Check(SBInstallNonWaitingQuiescenceForClass(InheritedIdleProcess.class, &idleError),
          "installing twice is safe");
    XCUIElement *field = [XCUIElement new], *other = [XCUIElement new];
    field.elementType = XCUIElementTypeTextField;
    other.elementType = XCUIElementTypeTextView;
    field.exists = other.exists = YES;
    field.enabled = other.enabled = YES;
    field.hittable = other.hittable = YES;
    NSError *focusError = nil;
    Check(SBFocusedEditableElement(@[field, other], &focusError) == nil && focusError.code == 2, "typing refuses absent focus");
    field.hasKeyboardFocus = YES; focusError = nil;
    Check(SBFocusedEditableElement(@[field, other], &focusError) == field && focusError == nil, "typing resolves the focused editable field without refs");
    field.elementType = XCUIElementTypeSecureTextField;
    Check(SBFocusedEditableElement(@[field], &focusError) == field, "secure fields support focus resolution without reading values");
    other.hasKeyboardFocus = YES;
    Check(SBFocusedEditableElement(@[field, other], &focusError) == nil && focusError.code == 2, "typing refuses ambiguous focus");
    other.hasKeyboardFocus = NO; field.hittable = NO;
    Check(SBFocusedEditableElement(@[field], &focusError) == nil && focusError.code == 2, "typing refuses covered focused fields");
    field.hittable = YES; field.enabled = NO;
    Check(SBFocusedEditableElement(@[field], &focusError) == nil && focusError.code == 2, "typing refuses disabled focused fields");
    field.enabled = YES; field.exists = NO;
    Check(SBFocusedEditableElement(@[field], &focusError) == nil && focusError.code == 2, "typing refuses disappeared focused fields");
    UnsupportedFocus *unsupported = [UnsupportedFocus new]; unsupported.elementType = XCUIElementTypeTextField;
    Check(SBFocusedEditableElement(@[unsupported], &focusError) == nil && focusError.code == 1, "unsupported XCTest focus inspection fails closed");
    field.elementType = XCUIElementTypeSearchField; field.exists = YES;
    other.elementType = XCUIElementTypeButton; other.hasKeyboardFocus = YES;
    FocusApplication *focusApp = [FocusApplication new];
    focusApp.elements = @[other, field]; focusError = nil;
    Check(SBFocusedEditableInApplication(focusApp, &focusError) == field && focusError == nil, "native editable query includes search fields and excludes focused non-editable controls");
    focusApp.elements = @[field, unsupported]; focusError = nil;
    Check(SBFocusedEditableInApplication(focusApp, &focusError) == nil && focusError.code == 1, "filtered focus still refuses unsupported inspection instead of selecting the first field");
    focusError = nil;
    Check(SBFocusedEditableInApplication([XCUIElement new], &focusError) == nil && focusError.code == 1, "unsupported editable queries fail closed");
    Application *application = [Application new];
    NSError *error = nil;
    BOOL (^allow)(NSError **) = ^BOOL(NSError **inner) { return YES; };
    NSDictionary *result = SBConcreteTap(CGPointMake(12, 24), application, 0.01, allow, &error);
    Check(result != nil && error == nil && calls == 1, "successful callback acknowledgement");
    incompatible = YES;
    result = SBConcreteTap(CGPointMake(12, 24), application, 0.01, allow, &error);
    Check(result == nil && error.code == 1 && calls == 1, "signature mismatch never dispatches");
    incompatible = NO; error = nil;
    result = SBConcreteTap(CGPointMake(12, 24), application, 0.01, ^BOOL(NSError **inner) { return NO; }, &error);
    Check(result == nil && error.code == 3 && calls == 1, "occlusion/transition admission refusal never dispatches");
    result = SBConcreteTap(CGPointMake(12, 24), application, 0.01, ^BOOL(NSError **inner) { orientation = 2; return YES; }, &error);
    Check(result == nil && error.code == 3 && calls == 1, "rotation after admission never dispatches");
    orientation = 1;
    for (mode = 1; mode <= 4; ++mode) {
        error = nil;
        result = SBConcreteTap(CGPointMake(12, 24), application, 0.01, allow, &error);
        Check(result == nil && error.code == 2 && calls == mode + 1, "false/error/missing callback/exception is uncertain without replay");
    }
    mode = 5; error = nil;
    result = SBConcreteTap(CGPointMake(12, 24), application, 0.5, allow, &error);
    Check(result != nil && error == nil, "cross-queue duplicate callback takes first completion");
    mode = 6; error = nil;
    result = SBConcreteTap(CGPointMake(12, 24), application, 0.001, allow, &error);
    Check(result == nil && error.code == 2, "late callback cannot turn timeout into completion");
    [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.05]];
    SBInputIssueTracker *tracker = [SBInputIssueTracker new];
    Check([tracker beginCommand] == 0 && [tracker beginCommand] == 1, "reentrant mutation refused");
    [tracker record];
    Check([tracker finishCommandWithUncertainOutcome:NO] && [tracker beginCommand] == 2, "recorded XCTest issue quarantines input despite method return");
    tracker = [SBInputIssueTracker new];
    Check([tracker beginCommand] == 0 && ![tracker finishCommandWithUncertainOutcome:NO] && [tracker beginCommand] == 0, "successful commands release serialization");
    Check([tracker finishCommandWithUncertainOutcome:YES] && [tracker beginCommand] == 2, "callback uncertainty persists until a new tracker/session");
    tracker = [SBInputIssueTracker new]; [tracker record];
    Check([tracker beginCommand] == 2, "late issue prevents next dispatch");
    tracker = [SBInputIssueTracker new];
    Check([tracker beginObservation] && [tracker beginCommand] == 1,
          "inspection excludes concurrent input");
    [tracker record];
    [tracker finishObservation];
    Check(tracker.count == 1 && [tracker beginCommand] == 0,
          "read-only XCTest issue is retained without poisoning later input");
    Check(![tracker beginObservation], "inspection cannot hide an active mutation issue");
    [tracker record];
    Check([tracker finishCommandWithUncertainOutcome:NO], "mutation issue still quarantines input");
    Check([tracker beginObservation], "uncertain input may still be inspected");
    [tracker finishObservation];
    Check([tracker beginCommand] == 2, "inspection cannot clear prior input quarantine");
    SBRunnerIssueCase *issueCase = [SBRunnerIssueCase new];
    Check([issueCase.inputIssues beginCommand] == 0, "issue correlation begins with a live command");
    [issueCase recordIssue:[XCTIssue new]];
    Check(issueCase.recordedIssues == 1 && issueCase.inputIssues.count == 1 &&
          [issueCase.inputIssues finishCommandWithUncertainOutcome:NO] &&
          [issueCase.inputIssues beginCommand] == 2, "production recordIssue forwards to XCTest and prevents false completion/next input");
    CGRect frame = CGRectMake(0, 0, 100, 100);
    Snapshot *app = [Snapshot new], *window = [Snapshot new], *overlay = [Snapshot new];
    app.frame = window.frame = frame;
    app.visibleFrame = window.visibleFrame = frame; window.elementType = XCUIElementTypeWindow;
    app.children = @[window]; window.children = @[]; overlay.children = @[];
    SnapshotSource *appSource = [SnapshotSource new]; appSource.snapshot = app;
    error = nil;
    Check(SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error), "visible scoped window and global app ownership permit point");
    overlay.elementType = XCUIElementTypeAlert; overlay.visibleFrame = frame; app.children = @[window, overlay];
    Check(!SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error) && error.code == 3, "app dialog refused");
    overlay.elementType = XCUIElementTypeSheet;
    Check(!SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error) && error.code == 3, "app sheet refused");
    app.children = @[window]; ownerProcess = 8;
    Check(!SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error) && error.code == 3, "system/foreign point ownership refused");
    ownerProcess = 7; missingOwner = YES;
    Check(!SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error) && error.code == 1, "missing global ownership refused");
    missingOwner = NO; ownerFailure = YES;
    Check(!SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error) && error.code == 1, "global ownership error refused");
    ownerFailure = NO; window.visibleFrame = CGRectZero;
    Check(!SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error) && error.code == 4, "invisible scoped window refused");
    window.visibleFrame = frame; app.frame = CGRectMake(0, 0, 50, 50);
    Check(!SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error) && error.code == 4, "transition geometry refused");
    app.frame = frame; app.children = @[window, overlay]; overlay.elementType = XCUIElementTypeAlert;
    overlay.visibleFrame = CGRectNull; error = nil;
    Check(SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error), "CGRectNull is a hidden node, not invalid geometry");
    overlay.visibleFrame = CGRectMake(NAN, 0, 10, 10);
    Check(!SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error) && error.code == 1, "other nonfinite snapshot geometry refused");
    app.children = @[app];
    Check(!SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error) && error.code == 1, "cyclic hierarchy refused");
    app.children = @[[NSObject new]];
    Check(!SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error) && error.code == 1, "incompatible snapshot refused");
    NSMutableArray *oversized = [NSMutableArray new];
    for (NSUInteger i = 0; i < 2000; ++i) [oversized addObject:window];
    app.children = oversized;
    Check(!SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error) && error.code == 1, "oversized hierarchy refused before traversal");
    app.children = @[window]; restarted = YES; ownerProcess = 7;
    Check(!SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error), "process restart during ownership check refused");
    restarted = NO; ownerProcess = 7; ownerTimeout = YES;
    Check(!SBCheckTapVisibility(appSource, frame, CGPointMake(12, 24), &error) && error.code == 1, "point ownership callback timeout refuses before touch dispatch");
    puts("Concrete tap failure contracts passed");
} return 0; }
