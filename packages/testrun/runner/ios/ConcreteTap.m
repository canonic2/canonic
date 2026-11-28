#import "ConcreteTap.h"
#import <XCTest/XCTest.h>
#import <objc/runtime.h>

@protocol SBPath
- (id)initForTouchAtPoint:(CGPoint)point offset:(double)offset;
- (void)liftUpAtOffset:(double)offset;
@end
@protocol SBRecord
- (id)initWithName:(NSString *)name interfaceOrientation:(long long)orientation;
- (void)addPointerEventPath:(id)path;
@end
@protocol SBDevice
- (id)eventSynthesizer;
@end
@protocol SBApplication
- (long long)interfaceOrientation;
- (int)processID;
@end
@protocol SBPointSession
- (id)daemonProxy;
@end
@protocol SBPointProxy
- (void)_XCT_requestElementAtPoint:(CGPoint)point reply:(void (^)(id, NSError *))reply;
@end
@protocol SBAccessibilityElement
- (int)processIdentifier;
@end
@protocol SBSynthesizer
- (id)synthesizeEvent:(id)event completion:(void (^)(_Bool, NSError *))completion;
@end
@protocol SBSnapshotSource
- (id)snapshotWithError:(NSError **)error;
@end
@protocol SBSnapshot
- (CGRect)frame;
- (CGRect)visibleFrame;
- (unsigned long long)elementType;
- (NSArray *)children;
@end

@implementation SBInputIssueTracker {
    NSUInteger _count;
    BOOL _busy, _quarantined, _observing;
}
- (NSUInteger)count {
    @synchronized(self) {
        return _count;
    }
}
- (void)record {
    @synchronized(self) {
        ++_count;
        if (!_observing) _quarantined = YES;
    }
}
- (BOOL)beginObservation {
    @synchronized(self) {
        if (_busy || _observing) return NO;
        _observing = YES;
        return YES;
    }
}
- (void)finishObservation {
    @synchronized(self) { _observing = NO; }
}
- (NSInteger)beginCommand {
    @synchronized(self) {
        if (_busy || _observing)
            return 1;
        if (_quarantined)
            return 2;
        _busy = YES;
        return 0;
    }
}
- (BOOL)finishCommandWithUncertainOutcome:(BOOL)uncertain {
    @synchronized(self) {
        _busy = NO;
        _quarantined |= uncertain;
        return _quarantined;
    }
}
@end

static NSError *Failure(NSInteger code, NSString *message) {
    return [NSError errorWithDomain:@"TestrunConcreteTap"
                               code:code
                           userInfo:@{NSLocalizedDescriptionKey : message}];
}
static BOOL Signature(NSMethodSignature *signature, const char *result,
                      NSArray<NSString *> *arguments) {
    if (!signature || signature.numberOfArguments != arguments.count + 2 ||
        strcmp(signature.methodReturnType, result) != 0)
        return NO;
    for (NSUInteger i = 0; i < arguments.count; ++i) {
        if (strcmp([signature getArgumentTypeAtIndex:i + 2], arguments[i].UTF8String) != 0)
            return NO;
    }
    return YES;
}

static BOOL SnapshotSignature(id snapshot) {
    return Signature([snapshot methodSignatureForSelector:@selector(frame)], @encode(CGRect),
                     @[]) &&
           Signature([snapshot methodSignatureForSelector:@selector(visibleFrame)], @encode(CGRect),
                     @[]) &&
           Signature([snapshot methodSignatureForSelector:@selector(elementType)], "Q", @[]) &&
           Signature([snapshot methodSignatureForSelector:@selector(children)], "@", @[]);
}
static BOOL InspectSnapshot(id<SBSnapshot> root, CGPoint point, NSError **error) {
    NSMutableArray *pending = [NSMutableArray arrayWithObject:root];
    NSMutableSet *visited = [NSMutableSet new];
    BOOL appWindow = NO;
    while (pending.count) {
        id<SBSnapshot> node = pending.lastObject;
        [pending removeLastObject];
        NSValue *identity = [NSValue valueWithNonretainedObject:node];
        if ([visited containsObject:identity]) {
            *error = Failure(1, @"Cyclic visibility snapshot");
            return NO;
        }
        [visited addObject:identity];
        if (visited.count > 2000 || !SnapshotSignature(node)) {
            *error = Failure(1, @"Visibility snapshot is incompatible or exceeds its bound");
            return NO;
        }
        CGRect visible = [node visibleFrame];
        // XCTest uses CGRectNull (infinite origin) for hidden snapshot nodes.
        if (!CGRectIsNull(visible) &&
            (!isfinite(visible.origin.x) || !isfinite(visible.origin.y) ||
             !isfinite(visible.size.width) || !isfinite(visible.size.height))) {
            *error = Failure(1, @"Visibility snapshot has invalid geometry");
            return NO;
        }
        unsigned long long type = [node elementType];
        BOOL shown = !CGRectIsEmpty(visible) && !CGRectIsNull(visible);
        if (shown && (type == XCUIElementTypeAlert || type == XCUIElementTypeSheet)) {
            *error = Failure(3, @"A dialog requires explicit handling before point input");
            return NO;
        }
        if (shown && CGRectContainsPoint(visible, point)) {
            if (type == XCUIElementTypeWindow)
                appWindow = YES;
        }
        id children = [node children];
        if (![children isKindOfClass:NSArray.class] || [children count] > 2000 - visited.count ||
            pending.count + [children count] > 2000) {
            *error =
                Failure(1, @"Visibility snapshot children are unavailable or exceed their bound");
            return NO;
        }
        [pending addObjectsFromArray:children];
    }
    if (!appWindow) {
        *error = Failure(4, @"No visible app window owns the requested point");
        return NO;
    }
    return YES;
}

BOOL SBCheckTapVisibility(NSObject *application, CGRect appFrame, CGPoint point, NSError **error) {
    @try {
        id device = XCUIDevice.sharedDevice;
        if (!Signature([application methodSignatureForSelector:@selector(snapshotWithError:)], "@",
                       @[ @"^@" ]) ||
            !Signature([application methodSignatureForSelector:@selector(processID)], "i", @[]) ||
            !Signature([device methodSignatureForSelector:@selector(eventSynthesizer)], "@", @[])) {
            *error = Failure(1, @"Visibility snapshot API is incompatible");
            return NO;
        }
        int process = [(id<SBApplication>)application processID];
        id session = [(id<SBDevice>)device eventSynthesizer];
        if (process <= 0 ||
            !Signature([session methodSignatureForSelector:@selector(daemonProxy)], "@", @[])) {
            *error = Failure(1, @"Global point ownership API is unavailable");
            return NO;
        }
        id proxy = [(id<SBPointSession>)session daemonProxy];
        if (!Signature(
                [proxy methodSignatureForSelector:@selector(_XCT_requestElementAtPoint:reply:)],
                "v", @[ @(@encode(CGPoint)), @"@?" ])) {
            *error = Failure(1, @"Global point ownership proxy is incompatible");
            return NO;
        }
        NSError *snapshotError = nil;
        id<SBSnapshot> appRoot =
            [(id<SBSnapshotSource>)application snapshotWithError:&snapshotError];
        if (!appRoot || snapshotError || !SnapshotSignature(appRoot)) {
            *error = Failure(1, @"App visibility snapshot is unavailable");
            return NO;
        }
        if (!CGRectEqualToRect([appRoot frame], appFrame)) {
            *error = Failure(4, @"App geometry changed before dispatch");
            return NO;
        }
        if (!InspectSnapshot(appRoot, point, error))
            return NO;
        // SpringBoard's own hierarchy can contain home icons behind another app.
        // A global hit test establishes the process actually owning this screen point.
        NSObject *lock = [NSObject new];
        __block id element = nil;
        __block BOOL done = NO, failed = NO;
        [(id<SBPointProxy>)proxy _XCT_requestElementAtPoint:point
                                                      reply:^(id hit, NSError *innerError) {
                                                          @synchronized(lock) {
                                                              if (!done) {
                                                                  element = hit;
                                                                  failed = innerError != nil;
                                                                  done = YES;
                                                              }
                                                          }
                                                      }];
        NSTimeInterval deadline = NSProcessInfo.processInfo.systemUptime + 0.3;
        for (;;) {
            @synchronized(lock) {
                if (done)
                    break;
                if (NSProcessInfo.processInfo.systemUptime >= deadline) {
                    done = YES;
                    failed = YES;
                    break;
                }
            }
            [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.001]];
        }
        if (!element || failed ||
            !Signature([element methodSignatureForSelector:@selector(processIdentifier)], "i",
                       @[])) {
            *error = Failure(1, @"Global point ownership could not be established");
            return NO;
        }
        if ([(id<SBAccessibilityElement>)element processIdentifier] != process) {
            *error = Failure(3, @"Another process covers the requested point");
            return NO;
        }
        if ([(id<SBApplication>)application processID] != process) {
            *error = Failure(4, @"App process changed during admission");
            return NO;
        }
        return YES;
    } @catch (NSException *exception) {
        *error = Failure(1, @"Visibility snapshot inspection raised an exception");
        return NO;
    }
}

NSDictionary *SBConcreteTap(CGPoint point, NSObject *application, NSTimeInterval timeout,
                            BOOL (^admit)(NSError **), NSError **error) {
    BOOL dispatched = NO;
    NSTimeInterval begin = NSProcessInfo.processInfo.systemUptime;
    @try {
        Class pathClass = NSClassFromString(@"XCPointerEventPath");
        Class recordClass = NSClassFromString(@"XCSynthesizedEventRecord");
        id device = XCUIDevice.sharedDevice;
        if (!isfinite(point.x) || !isfinite(point.y) || timeout <= 0 || timeout > 5 || !pathClass ||
            !recordClass ||
            !Signature(
                [pathClass
                    instanceMethodSignatureForSelector:@selector(initForTouchAtPoint:offset:)],
                "@", @[ @(@encode(CGPoint)), @"d" ]) ||
            !Signature([pathClass instanceMethodSignatureForSelector:@selector(liftUpAtOffset:)],
                       "v", @[ @"d" ]) ||
            !Signature(
                [recordClass
                    instanceMethodSignatureForSelector:@selector(
                                                           initWithName:interfaceOrientation:)],
                "@", @[ @"@", @"q" ]) ||
            !Signature(
                [recordClass instanceMethodSignatureForSelector:@selector(addPointerEventPath:)],
                "v", @[ @"@" ]) ||
            !Signature([device methodSignatureForSelector:@selector(eventSynthesizer)], "@", @[]) ||
            !Signature([application methodSignatureForSelector:@selector(interfaceOrientation)],
                       "q", @[])) {
            *error = Failure(1, @"Concrete touch API is unavailable or incompatible");
            return nil;
        }
        id synthesizer = [(id<SBDevice>)device eventSynthesizer];
        // The measured daemon proxy returns an object, not void. Its return is not acknowledgement.
        if (!Signature(
                [synthesizer methodSignatureForSelector:@selector(synthesizeEvent:completion:)],
                "@", @[ @"@", @"@?" ])) {
            *error = Failure(1, @"Concrete synthesis API is incompatible");
            return nil;
        }
        long long orientation = [(id<SBApplication>)application interfaceOrientation];
        if (orientation < 1 || orientation > 4) {
            *error = Failure(1, @"Interface orientation is unavailable");
            return nil;
        }
        id<SBPath> path = [(id<SBPath>)[pathClass alloc] initForTouchAtPoint:point offset:0];
        [path liftUpAtOffset:0.05];
        id<SBRecord> record = [(id<SBRecord>)[recordClass alloc] initWithName:@"Testrun tap"
                                                         interfaceOrientation:orientation];
        if (!path || !record) {
            *error = Failure(1, @"Concrete event construction failed");
            return nil;
        }
        [record addPointerEventPath:path];
        if (!admit(error)) {
            if (!*error)
                *error = Failure(3, @"Tap admission refused");
            return nil;
        }
        if ([(id<SBApplication>)application interfaceOrientation] != orientation) {
            *error = Failure(3, @"Interface orientation changed before dispatch");
            return nil;
        }
        NSTimeInterval built = NSProcessInfo.processInfo.systemUptime;
        NSObject *completionLock = [NSObject new];
        __block BOOL done = NO, success = NO;
        __block NSError *callbackError = nil;
        dispatched = YES;
        [(id<SBSynthesizer>)synthesizer
            synthesizeEvent:record
                 completion:^(_Bool result, NSError *innerError) {
                     @synchronized(completionLock) {
                         if (done)
                             return; // Ignore duplicate and late callbacks; never replay input.
                         success = result;
                         callbackError = innerError;
                         done = YES;
                     }
                 }];
        for (;;) {
            @synchronized(completionLock) {
                if (done) {
                    if (!success || callbackError) {
                        *error = Failure(2, @"Touch callback failed; outcome unknown");
                        return nil;
                    }
                    break;
                }
                if (NSProcessInfo.processInfo.systemUptime >= built + timeout) {
                    done = YES;
                    *error = Failure(2, @"Touch callback timed out; outcome unknown");
                    return nil;
                }
            }
            [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.001]];
        }
        return @{
            @"constructionMs" : @((built - begin) * 1000),
            @"callbackMs" : @((NSProcessInfo.processInfo.systemUptime - built) * 1000),
            @"callbackSuccess" : @YES
        };
    } @catch (NSException *exception) {
        // Exception text can contain application data. Retain only the dispatch stage.
        *error = Failure(dispatched ? 2 : 1,
                         dispatched ? @"Touch synthesis raised an exception; outcome unknown"
                                    : @"Concrete event preparation raised an exception");
        return nil;
    }
}
