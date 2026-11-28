#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN
// Codes: 1 unsupported before dispatch, 2 uncertain after dispatch, 3 guard refused.
FOUNDATION_EXPORT NSDictionary *_Nullable SBConcreteTap(CGPoint point, NSObject *application,
                                                        NSTimeInterval timeout,
                                                        BOOL (^admit)(NSError **error),
                                                        NSError **error);
// One app hierarchy and a global accessibility hit test; no labels are read.
FOUNDATION_EXPORT BOOL SBCheckTapVisibility(NSObject *application, CGRect appFrame, CGPoint point,
                                            NSError **error);

// Issues can arrive on XCTest's callback queue, including after a command returned.
@interface SBInputIssueTracker : NSObject
@property(nonatomic, readonly) NSUInteger count;
- (NSInteger)beginCommand; // 0 admitted, 1 busy, 2 quarantined
- (BOOL)beginObservation;
- (void)finishObservation;
- (BOOL)finishCommandWithUncertainOutcome:(BOOL)uncertain;
- (void)record;
@end
NS_ASSUME_NONNULL_END
