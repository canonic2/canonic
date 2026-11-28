#import "AcmeObjCFaults.h"
#import <dispatch/dispatch.h>
#include <stdio.h>
@implementation AcmeObjCFaults
+ (void)exceptionCorrected:(BOOL)corrected { if (!corrected) [NSException raise:@"AcmeFixtureException" format:@"Acme Objective-C fixture failure"]; }
+ (void)mainThreadHangCorrected:(BOOL)corrected { if (!corrected) dispatch_async(dispatch_get_main_queue(), ^{ [NSThread sleepForTimeInterval:6]; }); }
+ (void)backgroundFailureCorrected:(BOOL)corrected { if (!corrected) dispatch_async(dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0), ^{ fputs("Acme Objective-C background exception requested\n", stderr); fflush(stderr); [NSException raise:@"AcmeBackgroundException" format:@"Acme Objective-C background failure"]; }); }
@end
