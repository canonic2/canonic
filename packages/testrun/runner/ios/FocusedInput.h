#import <XCTest/XCTest.h>

NS_ASSUME_NONNULL_BEGIN
// Resolve focus without tapping a field or reading its text. Codes: 1 unsupported,
// 2 no unique actionable focused editable element.
FOUNDATION_EXPORT XCUIElement *_Nullable SBFocusedEditableElement(NSArray<XCUIElement *> *elements,
                                                                  NSError **error);
// Restrict the native query before materializing elements; never resolve every
// descendant individually just to discover its type.
FOUNDATION_EXPORT XCUIElement *_Nullable SBFocusedEditableInApplication(XCUIElement *application,
                                                                       NSError **error);
NS_ASSUME_NONNULL_END
