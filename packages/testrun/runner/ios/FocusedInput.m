// Owns fail-closed XCTest focus resolution for typing without caller element refs.
#import "FocusedInput.h"

XCUIElement *SBFocusedEditableElement(NSArray<XCUIElement *> *elements, NSError **error) {
    @try {
        XCUIElement *focused = nil;
        for (XCUIElement *element in elements) {
            if (element.elementType != XCUIElementTypeTextField &&
                element.elementType != XCUIElementTypeSecureTextField &&
                element.elementType != XCUIElementTypeSearchField &&
                element.elementType != XCUIElementTypeTextView)
                continue;
            // XCTest exposes keyboard focus through its query/KVC attribute rather
            // than an iOS public property. Unsupported runtimes must refuse input.
            id value = [element valueForKey:@"hasKeyboardFocus"];
            if (![value isKindOfClass:NSNumber.class]) {
                @throw [NSException exceptionWithName:@"UnsupportedFocus" reason:nil userInfo:nil];
            }
            if (![value boolValue])
                continue;
            if (focused || !element.exists || !element.enabled || !element.hittable) {
                if (error)
                    *error = [NSError errorWithDomain:@"TestrunFocus"
                                                 code:2
                                             userInfo:@{
                                                 NSLocalizedDescriptionKey :
                                                     @"A unique enabled, hittable focused editable "
                                                     @"element is required"
                                             }];
                return nil;
            }
            focused = element;
        }
        if (!focused && error)
            *error = [NSError
                errorWithDomain:@"TestrunFocus"
                           code:2
                       userInfo:@{
                           NSLocalizedDescriptionKey : @"Focus an editable field before typing"
                       }];
        return focused;
    } @catch (NSException *exception) {
        if (error)
            *error =
                [NSError errorWithDomain:@"TestrunFocus"
                                    code:1
                                userInfo:@{
                                    NSLocalizedDescriptionKey :
                                        @"This XCTest runtime cannot prove editable keyboard focus"
                                }];
        return nil;
    }
}

XCUIElement *SBFocusedEditableInApplication(XCUIElement *application, NSError **error) {
    @try {
        NSPredicate *editable = [NSPredicate predicateWithFormat:@"elementType IN %@", @[
            @(XCUIElementTypeTextField), @(XCUIElementTypeSecureTextField),
            @(XCUIElementTypeTextView), @(XCUIElementTypeSearchField)
        ]];
        XCUIElementQuery *query = [[application descendantsMatchingType:XCUIElementTypeAny]
            matchingPredicate:editable];
        return SBFocusedEditableElement(query.allElementsBoundByIndex, error);
    } @catch (NSException *exception) {
        if (error)
            *error = [NSError errorWithDomain:@"TestrunFocus" code:1 userInfo:@{
                NSLocalizedDescriptionKey : @"This XCTest runtime cannot query editable keyboard focus"
            }];
        return nil;
    }
}
