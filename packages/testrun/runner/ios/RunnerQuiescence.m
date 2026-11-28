// The caller observes readiness explicitly; an animation is not an input lock.
#import "RunnerQuiescence.h"
#import <objc/runtime.h>
#include <stdbool.h>
#include <string.h>

static void SBNoIdleWait(id object, SEL selector, bool animations) {}
static void SBNoPreEventIdleWait(id object, SEL selector, bool animations, bool preEvent) {}

static BOOL SBVoidBooleanMethod(Method method, unsigned int count) {
    if (method_getNumberOfArguments(method) != count)
        return NO;
    char type[16];
    method_getReturnType(method, type, sizeof(type));
    if (strcmp(type, @encode(void)) != 0)
        return NO;
    for (unsigned int index = 2; index < count; ++index) {
        method_getArgumentType(method, index, type, sizeof(type));
        if (strcmp(type, @encode(bool)) != 0 && strcmp(type, @encode(BOOL)) != 0)
            return NO;
    }
    return YES;
}

BOOL SBInstallNonWaitingQuiescenceForClass(Class processClass, NSError **error) {
    @synchronized(NSProcessInfo.processInfo) {
        SEL selectors[] = {
            NSSelectorFromString(@"waitForQuiescenceIncludingAnimationsIdle:"),
            NSSelectorFromString(@"waitForQuiescenceIncludingAnimationsIdle:isPreEvent:")
        };
        Method methods[2] = {NULL, NULL};
        BOOL found = NO;
        for (unsigned int index = 0; index < 2; ++index) {
            methods[index] = class_getInstanceMethod(processClass, selectors[index]);
            if (!methods[index])
                continue;
            found = YES;
            if (!SBVoidBooleanMethod(methods[index], index + 3)) {
                found = NO;
                break;
            }
        }
        if (!found) {
            if (error)
                *error = [NSError errorWithDomain:@"TestrunQuiescence" code:1 userInfo:@{
                    NSLocalizedDescriptionKey : @"This XCTest runtime does not expose compatible idle-wait methods"
                }];
            return NO;
        }
        for (unsigned int index = 0; index < 2; ++index) {
            if (methods[index]) {
                IMP implementation = index == 0 ? (IMP)SBNoIdleWait : (IMP)SBNoPreEventIdleWait;
                // Add an override if the method is inherited: never modify a
                // superclass shared by a different native runtime class.
                if (!class_addMethod(processClass, selectors[index], implementation,
                                     method_getTypeEncoding(methods[index])))
                    method_setImplementation(methods[index], implementation);
            }
        }
        return YES;
    }
}

BOOL SBInstallNonWaitingQuiescence(NSError **error) {
    return SBInstallNonWaitingQuiescenceForClass(NSClassFromString(@"XCUIApplicationProcess"), error);
}
