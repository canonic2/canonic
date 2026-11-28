#import "ConcreteTap.h"
#import "FocusedInput.h"
#import "RunnerQuiescence.h"
#import <XCTest/XCTest.h>

NS_ASSUME_NONNULL_BEGIN
// Keeps issue routing testable at the XCTest boundary and safe across callback queues.
@interface SBRunnerIssueCase : XCTestCase
@property(nonatomic, readonly) SBInputIssueTracker *inputIssues;
@end
NS_ASSUME_NONNULL_END
