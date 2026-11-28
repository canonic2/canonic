// Owns the XCTest runtime compatibility check for non-idling automation.
#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN
// Installs only inside the owned runner. Unsupported method signatures refuse
// runner readiness rather than entering XCTest's unbounded implicit idle wait.
FOUNDATION_EXPORT BOOL SBInstallNonWaitingQuiescence(NSError **error);
// Exposed for the host boundary fixture; installation validates every known entry
// point before replacing any implementation.
FOUNDATION_EXPORT BOOL SBInstallNonWaitingQuiescenceForClass(Class _Nullable processClass, NSError **error);
NS_ASSUME_NONNULL_END
