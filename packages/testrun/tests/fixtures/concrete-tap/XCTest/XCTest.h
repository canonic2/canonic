// Host fixture: no device or XCTest daemon is contacted.
#import <Foundation/Foundation.h>
typedef NS_ENUM(NSUInteger, XCUIElementType) {
    XCUIElementTypeAny = 0,
    XCUIElementTypeWindow = 4, XCUIElementTypeSheet = 5,
    XCUIElementTypeAlert = 7, XCUIElementTypeButton = 9,
    XCUIElementTypeTextField = 49, XCUIElementTypeSecureTextField = 50,
    XCUIElementTypeTextView = 52, XCUIElementTypeSearchField = 45
};
@class XCUIElementQuery;
@interface XCUIElement : NSObject
@property XCUIElementType elementType;
@property BOOL exists;
@property(getter=isEnabled) BOOL enabled;
@property(getter=isHittable) BOOL hittable;
@property BOOL hasKeyboardFocus;
- (XCUIElementQuery *)descendantsMatchingType:(XCUIElementType)type;
@end
@interface XCUIElementQuery : NSObject
@property NSArray<XCUIElement *> *elements;
@property BOOL filtered;
@property(readonly, copy) NSArray<XCUIElement *> *allElementsBoundByIndex;
- (XCUIElementQuery *)matchingPredicate:(NSPredicate *)predicate;
@end
@interface XCUIDevice : NSObject
@property(class, readonly) XCUIDevice *sharedDevice;
@end
@interface XCTIssue : NSObject
@end
@interface XCTestCase : NSObject
@property(nonatomic, readonly) NSUInteger recordedIssues;
- (void)recordIssue:(XCTIssue *)issue;
@end
