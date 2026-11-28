#import <Foundation/Foundation.h>
#import <IOSurface/IOSurface.h>

extern int sb_simulator_geometry(void *, unsigned int *, unsigned int *);

static NSUInteger created, destroyed;
static int mode;
static IOSurfaceRef surface;

static void Check(BOOL condition, const char *message) {
    if (!condition) { fprintf(stderr, "FAIL: %s\n", message); exit(1); }
}

@interface GeometryTemporary : NSObject
@end
@implementation GeometryTemporary
- (instancetype)init { if ((self = [super init])) ++created; return self; }
- (void)dealloc { ++destroyed; [super dealloc]; }
@end

// MRC deliberately models autoreleased objects returned by framework getters.
static void Temporary(void) { [[[GeometryTemporary alloc] init] autorelease]; }
@interface GeometryBoundary : NSObject
@end
@implementation GeometryBoundary
- (id)peer { return self; }
- (BOOL)valid { Temporary(); return mode != 1; }
- (id)descriptor { return self; }
- (IOSurfaceRef)framebufferSurface {
    Temporary();
    if (mode == 3)
        @throw [NSException exceptionWithName:@"GeometryFixture" reason:nil userInfo:nil];
    return mode == 0 ? surface : NULL;
}
- (IOSurfaceRef)ioSurface { Temporary(); return mode == 4 ? surface : NULL; }
@end

int main(void) {
    @autoreleasepool {
        surface = IOSurfaceCreate((CFDictionaryRef)@{
            (id)kIOSurfaceWidth: @16, (id)kIOSurfaceHeight: @32,
            (id)kIOSurfaceBytesPerElement: @4
        });
        Check(surface != NULL, "real IOSurface created");
        GeometryBoundary *boundary = [[GeometryBoundary alloc] init];
        for (mode = 0; mode <= 4; ++mode) {
            for (NSUInteger i = 0; i < 1000; ++i) {
                unsigned int width = 0, height = 0;
                int result = sb_simulator_geometry(boundary, &width, &height);
                Check(result == (mode == 3 ? 6 : (mode == 0 || mode == 4 ? 0 : 1)),
                      "success, invalid peer, missing surface and exception preserve status");
                if (result == 0)
                    Check(width == 16 && height == 32, "geometry survives temporary release");
                Check(created == destroyed, "geometry releases temporaries before returning");
            }
        }
        [boundary release];
        CFRelease(surface);
        printf("Geometry ownership passed: 5000 calls, %lu temporaries released\n",
               (unsigned long)destroyed);
    }
    return 0;
}
