#import <Foundation/Foundation.h>
@interface AcmeObjCFaults : NSObject
+ (void)exceptionCorrected:(BOOL)corrected;
+ (void)mainThreadHangCorrected:(BOOL)corrected;
+ (void)backgroundFailureCorrected:(BOOL)corrected;
@end
