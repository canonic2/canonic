// Testrun owns this host-side bridge. Runtime-only selectors are isolated here.
// See specs/ios/simulator-native.md for the private API compatibility boundary.
#import <AVFoundation/AVFoundation.h>
#import <AppKit/AppKit.h>
#import <CoreImage/CoreImage.h>
#import <Foundation/Foundation.h>
#import <IOSurface/IOSurface.h>
#import <VideoToolbox/VideoToolbox.h>
#import <dlfcn.h>
#import <objc/runtime.h>
#import <time.h>
#import <sys/stat.h>
#import <libproc.h>
#import <xpc/xpc.h>

@protocol SBSimulatorObject
+ (id)sharedServiceContextForDeveloperDir:(NSString *)directory error:(NSError **)error;
- (id)defaultDeviceSetWithError:(NSError **)error;
- (id)deviceSetWithPath:(NSString *)path error:(NSError **)error;
- (NSArray *)availableDevices;
- (NSUUID *)UDID;
- (NSString *)devicePath;
- (id)deviceSet;
- (id)serviceContext;
- (id)serviceConnection;
- (id)io;
- (NSArray *)ioPorts;
- (id)descriptor;
- (id)state;
- (unsigned int)displayClass;
- (IOSurfaceRef)framebufferSurface;
- (IOSurfaceRef)ioSurface;
- (id)initWithDevice:(id)device error:(NSError **)error;
- (void)sendWithMessage:(void *)message
           freeWhenDone:(BOOL)freeWhenDone
        completionQueue:(dispatch_queue_t)queue
             completion:(void (^)(NSError *))completion;
- (void)sendAccessibilityRequestAsync:(id)request
                      completionQueue:(dispatch_queue_t)queue
                    completionHandler:(void (^)(id))completion;
+ (id)sharedInstance;
+ (id)emptyResponse;
- (void)setBridgeTokenDelegate:(id)delegate;
- (void)setAccessibilityEnabled:(BOOL)enabled;
- (id)frontmostApplicationWithDisplayId:(unsigned int)display bridgeDelegateToken:(NSString *)token;
- (id)macPlatformElementFromTranslation:(id)translation;
- (id)translation;
- (int)pid;
- (void)setBridgeDelegateToken:(NSString *)token;
- (NSString *)accessibilityRole;
- (NSString *)accessibilitySubrole;
- (NSString *)accessibilityLabel;
- (NSString *)accessibilityIdentifier;
- (id)accessibilityValue;
- (CGRect)accessibilityFrame;
- (NSArray *)accessibilityChildren;
- (void)registerCallbackWithUUID:(NSUUID *)uuid
        damageRectanglesCallback:(void (^)(NSArray *))callback;
- (void)registerCallbackWithUUID:(NSUUID *)uuid ioSurfacesChangeCallback:(void (^)(id))callback;
- (void)unregisterCallbackWithUUID:(NSUUID *)uuid;
@end

static id<SBSimulatorObject> device_for_uuid(const char *developer, const char *uuid);

@interface SBAccessibilityBridge : NSObject
@property(nonatomic, strong) id<SBSimulatorObject> device;
@property(nonatomic, strong) NSString *token;
@property(nonatomic) BOOL incomplete;
@end
@implementation SBAccessibilityBridge
- (id (^)(id))accessibilityTranslationDelegateBridgeCallbackWithToken:(NSString *)token {
    return ^id(id request) {
        if (![token isEqualToString:self.token]) {
            self.incomplete = YES;
            return [(id<SBSimulatorObject>)NSClassFromString(@"AXPTranslatorResponse") emptyResponse];
        }
        dispatch_semaphore_t done = dispatch_semaphore_create(0);
        __block id response = nil;
        [self.device
            sendAccessibilityRequestAsync:request
                          completionQueue:dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0)
                        completionHandler:^(id value) {
                            response = value;
                            dispatch_semaphore_signal(done);
                        }];
        if (dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 500 * NSEC_PER_MSEC)) !=
                0 ||
            !response) {
            self.incomplete = YES;
            return [(id<SBSimulatorObject>)NSClassFromString(@"AXPTranslatorResponse") emptyResponse];
        }
        return response;
    };
}
- (CGRect)accessibilityTranslationConvertPlatformFrameToSystem:(CGRect)frame
                                                     withToken:(NSString *)token {
    (void)token;
    return frame;
}
- (id)accessibilityTranslationRootParentWithToken:(NSString *)token {
    (void)token;
    return nil;
}
@end

static NSDictionary *ax_node(id<SBSimulatorObject> element, SBAccessibilityBridge *delegate, unsigned int depth,
                             unsigned int *count) {
    if (!element || depth >= 60 || *count >= 500) {
        delegate.incomplete = YES;
        return nil;
    }
    (*count)++;
    id<SBSimulatorObject> translation = [element translation];
    if ([(id)translation respondsToSelector:@selector(setBridgeDelegateToken:)])
        [translation setBridgeDelegateToken:delegate.token];
    NSString *role = [element accessibilityRole] ?: @"unknown";
    NSString *subrole = [element accessibilitySubrole] ?: @"";
    BOOL secure =
        [subrole rangeOfString:@"secure" options:NSCaseInsensitiveSearch].location != NSNotFound ||
        [role rangeOfString:@"secure" options:NSCaseInsensitiveSearch].location != NSNotFound;
    CGRect frame = [element accessibilityFrame];
    NSMutableDictionary *node = [@{
        @"elementRef" : [NSString stringWithFormat:@"node-%u", *count],
        @"role" : role,
        @"secure" : @(secure),
        @"frame" : @{
            @"x" : @(frame.origin.x),
            @"y" : @(frame.origin.y),
            @"width" : @(frame.size.width),
            @"height" : @(frame.size.height)
        }
    } mutableCopy];
    for (NSString *key in @[ @"label", @"identifier" ]) {
        NSString *text = [key isEqualToString:@"label"] ? [element accessibilityLabel]
                                                        : [element accessibilityIdentifier];
        if (text.length)
            node[key] = [text substringToIndex:MIN(text.length, 8192)];
    }
    if (!secure) {
        id value = [element accessibilityValue];
        if ([value isKindOfClass:NSString.class])
            node[@"value"] = [value substringToIndex:MIN([value length], 8192)];
        else if ([value isKindOfClass:NSNumber.class])
            node[@"value"] = value;
    }
    NSMutableArray *children = [NSMutableArray array];
    for (id<SBSimulatorObject> child in [element accessibilityChildren]) {
        NSDictionary *node = ax_node(child, delegate, depth + 1, count);
        if (node)
            [children addObject:node];
        if (*count >= 500) {
            delegate.incomplete = YES;
            break;
        }
    }
    node[@"children"] = children;
    NSMutableDictionary *states = [NSMutableDictionary dictionary];
    for (NSString *field in @[ @"enabled", @"focused" ]) {
        NSString *key =
            [field isEqualToString:@"enabled"] ? @"accessibilityEnabled" : @"accessibilityFocused";
        NSString *getter = [field isEqualToString:@"enabled"] ? @"isAccessibilityEnabled"
                                                              : @"isAccessibilityFocused";
        if ([(id)element respondsToSelector:NSSelectorFromString(getter)]) {
            @try {
                id value = [(id)element valueForKey:key];
                if ([value isKindOfClass:NSNumber.class])
                    states[field] = @([value boolValue]);
            } @catch (NSException *exception) {
                (void)exception;
                delegate.incomplete = YES;
            }
        }
    }
    node[@"states"] = states;
    return node;
}

unsigned char *sb_simulator_accessibility(const char *developer, const char *uuid, size_t *size,
                                          int *failure) {
    *size = 0;
    *failure = 1;
    @autoreleasepool {
        @try {
            id<SBSimulatorObject> device = device_for_uuid(developer, uuid);
            if (!device ||
                ![(id)device respondsToSelector:@selector(sendAccessibilityRequestAsync:
                                                          completionQueue:completionHandler:)])
                return NULL;
            if (!dlopen(
                    "/System/Library/PrivateFrameworks/AccessibilityPlatformTranslation.framework/"
                    "AccessibilityPlatformTranslation",
                    RTLD_NOW | RTLD_GLOBAL))
                return NULL;
            Class translatorClass = NSClassFromString(@"AXPTranslator");
            if (!translatorClass)
                return NULL;
            SBAccessibilityBridge *delegate = [SBAccessibilityBridge new];
            delegate.device = device;
            delegate.token = NSUUID.UUID.UUIDString;
            id<SBSimulatorObject> translator = [(id<SBSimulatorObject>)translatorClass sharedInstance];
            [translator setBridgeTokenDelegate:delegate];
            [translator setAccessibilityEnabled:YES];
            id translated = [translator frontmostApplicationWithDisplayId:0
                                                      bridgeDelegateToken:delegate.token];
            id<SBSimulatorObject> element = [translator macPlatformElementFromTranslation:translated];
            unsigned int count = 0;
            NSDictionary *root = ax_node(element, delegate, 0, &count);
            [translator setBridgeTokenDelegate:nil];
            if (!root) {
                *failure = 2;
                return NULL;
            }
            NSMutableDictionary *result = [@{
                @"quality" : @"partial",
                @"coverage" : @"frontmost_app",
                @"nodeCount" : @(count),
                @"truncated" : @(delegate.incomplete),
                @"root" : root
            } mutableCopy];
            // The frontmost app's process lets callers tie the tree to a launch.
            if ([(id)translated respondsToSelector:@selector(pid)] &&
                [(id<SBSimulatorObject>)translated pid] > 0)
                result[@"pid"] = @([(id<SBSimulatorObject>)translated pid]);
            NSData *data = [NSJSONSerialization dataWithJSONObject:result options:0 error:nil];
            if (!data || data.length > 2 * 1024 * 1024) {
                *failure = 3;
                return NULL;
            }
            unsigned char *bytes = malloc(data.length);
            if (!bytes) {
                *failure = 5;
                return NULL;
            }
            memcpy(bytes, data.bytes, data.length);
            *size = data.length;
            *failure = 0;
            return bytes;
        } @catch (NSException *exception) {
            (void)exception;
            *failure = 6;
            return NULL;
        }
    }
}

int sb_simulator_process_identity(int pid, uint64_t *seconds, uint64_t *micros) {
    struct proc_bsdinfo info = {0};
    if (pid <= 0 || proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, sizeof(info)) != sizeof(info))
        return 0;
    *seconds = info.pbi_start_tvsec;
    *micros = info.pbi_start_tvusec;
    return 1;
}
int sb_simulator_peer_alive(int pid, uint64_t seconds, uint64_t micros) {
    uint64_t currentSeconds = 0, currentMicros = 0;
    return sb_simulator_process_identity(pid, &currentSeconds, &currentMicros) &&
           currentSeconds == seconds && currentMicros == micros;
}
@interface SBServicePeer : NSObject
@property(nonatomic, strong) id connection;
@property(nonatomic) pid_t pid;
@property(nonatomic) uint64_t seconds;
@property(nonatomic) uint64_t micros;
- (BOOL)valid;
@end
@implementation SBServicePeer
- (BOOL)valid {
    if (@available(macOS 12.0, *)) {
        char *reason = xpc_connection_copy_invalidation_reason((xpc_connection_t)self.connection);
        if (reason) {
            free(reason);
            return NO;
        }
    }
    return xpc_connection_get_pid((xpc_connection_t)self.connection) == self.pid &&
           sb_simulator_peer_alive(self.pid, self.seconds, self.micros);
}
@end
static SBServicePeer *peer_for_device(id<SBSimulatorObject> device) {
    if (![(id)device respondsToSelector:@selector(deviceSet)])
        return nil;
    id<SBSimulatorObject> set = [device deviceSet];
    if (![(id)set respondsToSelector:@selector(serviceContext)])
        return nil;
    id<SBSimulatorObject> context = [set serviceContext];
    if (![(id)context respondsToSelector:@selector(serviceConnection)])
        return nil;
    id connection = [context serviceConnection];
    if (![connection isKindOfClass:NSClassFromString(@"OS_xpc_connection")])
        return nil;
    pid_t pid = xpc_connection_get_pid((xpc_connection_t)connection);
    struct proc_bsdinfo info = {0};
    if (pid <= 0 || proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, sizeof(info)) != sizeof(info))
        return nil;
    SBServicePeer *peer = [SBServicePeer new];
    peer.connection = connection;
    peer.pid = pid;
    peer.seconds = info.pbi_start_tvsec;
    peer.micros = info.pbi_start_tvusec;
    return [peer valid] ? peer : nil;
}

// All bridge-owned handles expose the same selected-device/service binding.
@protocol SBBoundSimulator <NSObject>
@property(nonatomic, strong) id<SBSimulatorObject> device;
@property(nonatomic, strong) SBServicePeer *peer;
@end

@interface SBIdentityConnection : NSObject <SBBoundSimulator>
@property(nonatomic, strong) id<SBSimulatorObject> device;
@property(nonatomic, strong) SBServicePeer *peer;
@end
@implementation SBIdentityConnection
@end
void *sb_simulator_identity_open(const char *developer, const char *uuid) {
    @autoreleasepool { @try {
        id<SBSimulatorObject> device=device_for_uuid(developer,uuid);
        if (!device) return NULL;
        SBIdentityConnection *connection=[SBIdentityConnection new];
        connection.device=device;connection.peer=peer_for_device(device);
        return connection.peer ? (__bridge_retained void *)connection : NULL;
    } @catch (NSException *exception) { (void)exception;return NULL; } }
}

@interface SBCapture : NSObject <SBBoundSimulator>
@property(nonatomic, strong) id<SBSimulatorObject> device;
@property(nonatomic, strong) id<SBSimulatorObject> descriptor;
@property(nonatomic, strong) SBServicePeer *peer;
@property(nonatomic, strong) CIContext *context;
@property(nonatomic) VTCompressionSessionRef encoder;
@property(nonatomic, strong) NSData *encoded;
@property(nonatomic) dispatch_semaphore_t encodedSignal;
@property(nonatomic) BOOL encodedKeyframe;
@property(nonatomic) unsigned int encodedWidth;
@property(nonatomic) unsigned int encodedHeight;
@property(nonatomic) unsigned int sourceWidth;
@property(nonatomic) unsigned int sourceHeight;
@property(nonatomic, strong) NSUUID *callbackToken;
@property(atomic) unsigned long long damageCounter;
@property(nonatomic) BOOL callbacksSupported;
@property(nonatomic) BOOL forceKeyframe;
@property(nonatomic) unsigned long long encodingStartNs;
@property(nonatomic, strong) NSString *recordPath;
@property(nonatomic, strong) AVAssetWriter *writer;
@property(nonatomic, strong) AVAssetWriterInput *writerInput;
@property(nonatomic) unsigned long long recordingDrops;
@property(nonatomic) BOOL recordingFailed;
@end
@implementation SBCapture
- (void)dealloc {
    if (_callbackToken &&
        [(id)_descriptor respondsToSelector:@selector(unregisterCallbackWithUUID:)]) {
        @try {
            [_descriptor unregisterCallbackWithUUID:_callbackToken];
        } @catch (NSException *exception) {
            (void)exception;
        }
    }
    if (_encoder) {
        VTCompressionSessionInvalidate(_encoder);
        CFRelease(_encoder);
    }
}
@end

static void h264_callback(void *opaque, void *source, OSStatus status, VTEncodeInfoFlags flags,
                          CMSampleBufferRef sample) {
    (void)source;
    (void)flags;
    SBCapture *capture = (__bridge SBCapture *)opaque;
    if (status != noErr || !sample || !CMSampleBufferDataIsReady(sample)) {
        dispatch_semaphore_signal(capture.encodedSignal);
        return;
    }
    CFArrayRef attachments = CMSampleBufferGetSampleAttachmentsArray(sample, false);
    BOOL key = !attachments || CFArrayGetCount(attachments) == 0 ||
               !CFDictionaryContainsKey(CFArrayGetValueAtIndex(attachments, 0),
                                        kCMSampleAttachmentKey_NotSync);
    NSMutableData *output = [NSMutableData data];
    const unsigned char start[] = {0, 0, 0, 1};
    CMFormatDescriptionRef format = CMSampleBufferGetFormatDescription(sample);
    if (capture.recordPath && !capture.recordingFailed) {
        if (!capture.writer) {
            NSError *error = nil;
            capture.writer =
                [[AVAssetWriter alloc] initWithURL:[NSURL fileURLWithPath:capture.recordPath]
                                          fileType:AVFileTypeMPEG4
                                             error:&error];
            capture.writerInput = [AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeVideo
                                                                     outputSettings:nil
                                                                   sourceFormatHint:format];
            capture.writerInput.expectsMediaDataInRealTime = YES;
            if (!capture.writer || ![capture.writer canAddInput:capture.writerInput])
                capture.recordingFailed = YES;
            else {
                [capture.writer addInput:capture.writerInput];
                if (![capture.writer startWriting])
                    capture.recordingFailed = YES;
                else
                    [capture.writer
                        startSessionAtSourceTime:CMSampleBufferGetPresentationTimeStamp(sample)];
            }
        }
        if (!capture.recordingFailed) {
            if (!capture.writerInput.readyForMoreMediaData)
                capture.recordingDrops++;
            else if (![capture.writerInput appendSampleBuffer:sample])
                capture.recordingFailed = YES;
        }
    }
    if (key) {
        size_t count = 0;
        int lengthBytes = 0;
        if (CMVideoFormatDescriptionGetH264ParameterSetAtIndex(format, 0, NULL, NULL, &count,
                                                               &lengthBytes) != noErr ||
            lengthBytes != 4) {
            dispatch_semaphore_signal(capture.encodedSignal);
            return;
        }
        for (size_t i = 0; i < count; i++) {
            const uint8_t *parameter = NULL;
            size_t length = 0;
            if (CMVideoFormatDescriptionGetH264ParameterSetAtIndex(format, i, &parameter, &length,
                                                                   NULL, NULL) != noErr) {
                dispatch_semaphore_signal(capture.encodedSignal);
                return;
            }
            [output appendBytes:start length:4];
            [output appendBytes:parameter length:length];
        }
    }
    CMBlockBufferRef block = CMSampleBufferGetDataBuffer(sample);
    size_t length = CMBlockBufferGetDataLength(block);
    if (length > 16 * 1024 * 1024) {
        dispatch_semaphore_signal(capture.encodedSignal);
        return;
    }
    NSMutableData *avcc = [NSMutableData dataWithLength:length];
    if (CMBlockBufferCopyDataBytes(block, 0, length, avcc.mutableBytes) != noErr) {
        dispatch_semaphore_signal(capture.encodedSignal);
        return;
    }
    const uint8_t *bytes = avcc.bytes;
    size_t offset = 0;
    while (offset + 4 <= length) {
        uint32_t count = ((uint32_t)bytes[offset] << 24) | ((uint32_t)bytes[offset + 1] << 16) |
                         ((uint32_t)bytes[offset + 2] << 8) | bytes[offset + 3];
        offset += 4;
        if (!count || count > length - offset) {
            dispatch_semaphore_signal(capture.encodedSignal);
            return;
        }
        [output appendBytes:start length:4];
        [output appendBytes:bytes + offset length:count];
        offset += count;
    }
    if (offset == length) {
        capture.encoded = output;
        capture.encodedKeyframe = key;
    }
    dispatch_semaphore_signal(capture.encodedSignal);
}

static NSData *h264_frame(SBCapture *capture, CIImage *image, unsigned int *width,
                          unsigned int *height) {
    double scale = MIN(1.0, 1280.0 / MAX(image.extent.size.width, image.extent.size.height));
    unsigned int w = ((unsigned int)(image.extent.size.width * scale) / 2) * 2;
    unsigned int h = ((unsigned int)(image.extent.size.height * scale) / 2) * 2;
    if (w < 2 || h < 2)
        return nil;
    if (capture.encoder && (w != capture.encodedWidth || h != capture.encodedHeight)) {
        VTCompressionSessionInvalidate(capture.encoder);
        CFRelease(capture.encoder);
        capture.encoder = NULL;
    }
    if (!capture.encoder) {
        VTCompressionSessionRef encoder = NULL;
        NSDictionary *attributes = @{
            (id)kCVPixelBufferPixelFormatTypeKey : @(kCVPixelFormatType_32BGRA),
            (id)kCVPixelBufferIOSurfacePropertiesKey : @{}
        };
        OSStatus status = VTCompressionSessionCreate(
            NULL, w, h, kCMVideoCodecType_H264,
            (__bridge CFDictionaryRef)
                @{(id)kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder : @YES},
            (__bridge CFDictionaryRef)attributes, NULL, h264_callback, (__bridge void *)capture,
            &encoder);
        if (status != noErr || !encoder)
            return nil;
        capture.encoder = encoder;
        capture.encodedWidth = w;
        capture.encodedHeight = h;
        if (VTSessionSetProperty(encoder, kVTCompressionPropertyKey_RealTime, kCFBooleanTrue) !=
                noErr ||
            VTSessionSetProperty(encoder, kVTCompressionPropertyKey_AllowFrameReordering,
                                 kCFBooleanFalse) != noErr ||
            VTSessionSetProperty(encoder, kVTCompressionPropertyKey_ProfileLevel,
                                 kVTProfileLevel_H264_Baseline_AutoLevel) != noErr ||
            VTSessionSetProperty(encoder, kVTCompressionPropertyKey_AverageBitRate,
                                 (__bridge CFNumberRef) @4000000) != noErr ||
            VTSessionSetProperty(encoder, kVTCompressionPropertyKey_MaxKeyFrameInterval,
                                 (__bridge CFNumberRef) @60) != noErr ||
            VTSessionSetProperty(encoder, kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration,
                                 (__bridge CFNumberRef) @2.0) != noErr ||
            VTCompressionSessionPrepareToEncodeFrames(encoder) != noErr)
            return nil;
    }
    CVPixelBufferRef pixel = NULL;
    CVPixelBufferPoolRef pool = VTCompressionSessionGetPixelBufferPool(capture.encoder);
    if (!pool || CVPixelBufferPoolCreatePixelBuffer(NULL, pool, &pixel) != kCVReturnSuccess)
        return nil;
    CIImage *scaled = [image
        imageByApplyingTransform:CGAffineTransformMakeScale((double)w / image.extent.size.width,
                                                            (double)h / image.extent.size.height)];
    [capture.context render:scaled toCVPixelBuffer:pixel];
    capture.encoded = nil;
    capture.encodedSignal = dispatch_semaphore_create(0);
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    unsigned long long ns = (unsigned long long)now.tv_sec * NSEC_PER_SEC + now.tv_nsec;
    if (!capture.encodingStartNs)
        capture.encodingStartNs = ns;
    CMTime pts = CMTimeMake(ns - capture.encodingStartNs, NSEC_PER_SEC);
    NSDictionary *options = capture.forceKeyframe
                                ? @{(id)kVTEncodeFrameOptionKey_ForceKeyFrame : @YES}
                                : nil;
    capture.forceKeyframe = NO;
    OSStatus status =
        VTCompressionSessionEncodeFrame(capture.encoder, pixel, pts, CMTimeMake(1, 30),
                                        (__bridge CFDictionaryRef)options, NULL, NULL);
    CVPixelBufferRelease(pixel);
    if (status != noErr ||
        dispatch_semaphore_wait(capture.encodedSignal,
                                dispatch_time(DISPATCH_TIME_NOW, 2 * NSEC_PER_SEC)) != 0)
        return nil;
    *width = w;
    *height = h;
    return capture.encoded;
}

static id<SBSimulatorObject> device_for_uuid(const char *developer, const char *uuid) {
    NSString *directory = [NSString stringWithUTF8String:developer];
    NSString *core = @"/Library/Developer/PrivateFrameworks/CoreSimulator.framework/CoreSimulator";
    NSString *kit =
        [directory stringByAppendingPathComponent:
                       @"Library/PrivateFrameworks/SimulatorKit.framework/SimulatorKit"];
    if (!dlopen(core.fileSystemRepresentation, RTLD_NOW | RTLD_GLOBAL))
        return nil;
    if (!dlopen(kit.fileSystemRepresentation, RTLD_NOW | RTLD_GLOBAL))
        return nil;
    Class contextClass = NSClassFromString(@"SimServiceContext");
    SEL selector = @selector(sharedServiceContextForDeveloperDir:error:);
    if (!contextClass || ![contextClass respondsToSelector:selector])
        return nil;
    NSError *error = nil;
    id<SBSimulatorObject> context =
        [(id<SBSimulatorObject>)contextClass sharedServiceContextForDeveloperDir:directory error:&error];
    const char *selected = getenv("TESTRUN_SIMULATOR_DEVICE_SET");
    id<SBSimulatorObject> set = nil;
    if (selected) {
        if (![(id)context respondsToSelector:@selector(deviceSetWithPath:error:)])
            return nil;
        NSString *path = [[NSString stringWithUTF8String:selected] stringByResolvingSymlinksInPath];
        if (![path isAbsolutePath]) return nil;
        set = [context deviceSetWithPath:path error:&error];
    } else {
        set = [context defaultDeviceSetWithError:&error];
    }
    if (!set || ![(id)set respondsToSelector:@selector(availableDevices)])
        return nil;
    NSString *wanted = [NSString stringWithUTF8String:uuid];
    for (id<SBSimulatorObject> device in [set availableDevices]) {
        if ([[[device UDID] UUIDString] caseInsensitiveCompare:wanted] == NSOrderedSame)
            return device;
    }
    return nil;
}

// This ABI returns malloc-owned encoded bytes and a stable numeric failure code.
// It executes in a bounded Rust-owned worker, so an Apple service hang cannot
// block the parent MCP process or outlive the worker's deadline.
void *sb_simulator_open(const char *developer, const char *uuid, int *failure) {
    *failure = 1;
    @autoreleasepool {
        @try {
            id<SBSimulatorObject> device = device_for_uuid(developer, uuid);
            if (!device)
                return NULL;
            id<SBSimulatorObject> io = [device io];
            for (id<SBSimulatorObject> port in [io ioPorts]) {
                id<SBSimulatorObject> descriptor = [port descriptor];
                if (![(id)descriptor respondsToSelector:@selector(state)])
                    continue;
                id<SBSimulatorObject> state = [descriptor state];
                if (![(id)state respondsToSelector:@selector(displayClass)] ||
                    [state displayClass] != 0)
                    continue;
                SBCapture *capture = [SBCapture new];
                capture.device = device;
                capture.peer = peer_for_device(device);
                if (!capture.peer) { *failure = 8; return NULL; }
                capture.descriptor = descriptor;
                capture.context = [CIContext contextWithOptions:@{
                    kCIContextUseSoftwareRenderer : @NO
                }];
                capture.damageCounter = 1;
                if ([(id)descriptor respondsToSelector:@selector(registerCallbackWithUUID:
                                                                 damageRectanglesCallback:)] &&
                    [(id)descriptor respondsToSelector:@selector(registerCallbackWithUUID:
                                                                 ioSurfacesChangeCallback:)]) {
                    capture.callbackToken = NSUUID.UUID;
                    __weak SBCapture *weak = capture;
                    [descriptor registerCallbackWithUUID:capture.callbackToken
                                damageRectanglesCallback:^(NSArray *rectangles) {
                                    if (!rectangles.count)
                                        return;
                                    SBCapture *value = weak;
                                    if (value) {
                                        @synchronized(value) {
                                            value.damageCounter++;
                                        }
                                    }
                                }];
                    [descriptor registerCallbackWithUUID:capture.callbackToken
                                ioSurfacesChangeCallback:^(id surface) {
                                    (void)surface;
                                    SBCapture *value = weak;
                                    if (value) {
                                        @synchronized(value) {
                                            value.damageCounter++;
                                        }
                                    }
                                }];
                    capture.callbacksSupported = YES;
                }
                *failure = 0;
                return (__bridge_retained void *)capture;
            }
            *failure = 2;
            return NULL;
        } @catch (NSException *exception) {
            (void)exception;
            *failure = 6;
            return NULL;
        }
    }
}
void sb_simulator_close(void *handle) {
    if (handle) {
        id value = (__bridge_transfer id)handle;
        (void)value;
    }
}

const char *sb_simulator_device_path(void *handle) {
    @try {
        id<SBBoundSimulator> binding = (__bridge id<SBBoundSimulator>)handle;
        if (![(id)binding.device respondsToSelector:@selector(devicePath)])
            return NULL;
        return [[binding.device devicePath] fileSystemRepresentation];
    } @catch (NSException *exception) {
        (void)exception;
        return NULL;
    }
}

int sb_simulator_peer(void *handle, int *pid, uint64_t *seconds, uint64_t *micros) {
    @try {
        SBServicePeer *peer = [(__bridge id<SBBoundSimulator>)handle peer];
        if (!peer || ![peer valid])
            return 1;
        *pid = peer.pid;
        *seconds = peer.seconds;
        *micros = peer.micros;
        return 0;
    } @catch (NSException *exception) {
        (void)exception;
        return 1;
    }
}

int sb_simulator_geometry(void *handle, unsigned int *width, unsigned int *height) {
    @autoreleasepool {
        @try {
            SBCapture *capture = (__bridge SBCapture *)handle;
            if (![capture.peer valid])
                return 1;
            id<SBSimulatorObject> descriptor = capture.descriptor;
            IOSurfaceRef surface = NULL;
            if ([(id)descriptor respondsToSelector:@selector(framebufferSurface)])
                surface = [descriptor framebufferSurface];
            if (!surface && [(id)descriptor respondsToSelector:@selector(ioSurface)])
                surface = [descriptor ioSurface];
            if (!surface)
                return 1;
            CFRetain(surface);
            *width = (unsigned int)IOSurfaceGetWidth(surface);
            *height = (unsigned int)IOSurfaceGetHeight(surface);
            CFRelease(surface);
            return *width && *height ? 0 : 1;
        } @catch (NSException *exception) {
            (void)exception;
            return 6;
        }
    }
}

unsigned char *sb_simulator_frame(void *handle, int jpeg, size_t *size, unsigned int *width,
                                  unsigned int *height, int *failure) {
    *size = 0;
    *failure = 1;
    @autoreleasepool {
        @try {
            SBCapture *capture = (__bridge SBCapture *)handle;
            if (![capture.peer valid]) {
                *failure = 7;
                return NULL;
            }
            id<SBSimulatorObject> descriptor = capture.descriptor;
            IOSurfaceRef surface = NULL;
            if ([(id)descriptor respondsToSelector:@selector(framebufferSurface)])
                surface = [descriptor framebufferSurface];
            if (!surface && [(id)descriptor respondsToSelector:@selector(ioSurface)])
                surface = [descriptor ioSurface];
            if (!surface) {
                *failure = 2;
                return NULL;
            }
            CFRetain(surface);
            capture.sourceWidth = (unsigned int)IOSurfaceGetWidth(surface);
            capture.sourceHeight = (unsigned int)IOSurfaceGetHeight(surface);
            if (!capture.sourceWidth || !capture.sourceHeight || capture.sourceWidth > 8192 ||
                capture.sourceHeight > 8192 ||
                (uint64_t)capture.sourceWidth * capture.sourceHeight * 4 > 128 * 1024 * 1024) {
                CFRelease(surface);
                *failure = 4;
                return NULL;
            }
            if (IOSurfaceLock(surface, kIOSurfaceLockReadOnly, NULL) != kIOReturnSuccess) {
                CFRelease(surface);
                *failure = 3;
                return NULL;
            }
            NSData *data = nil;
            CGImageRef rendered = NULL;
            @try {
                CIImage *image = [CIImage imageWithIOSurface:surface options:nil];
                CIContext *context = capture.context;
                CGRect rectangle = image.extent;
                if (jpeg == 3) {
                    unsigned int sw = capture.sourceWidth, sh = capture.sourceHeight;
                    unsigned int longest = MAX(sw, sh);
                    unsigned int w = longest > 960 ? MAX(1, (uint64_t)sw * 960 / longest) : sw;
                    unsigned int h = longest > 960 ? MAX(1, (uint64_t)sh * 960 / longest) : sh;
                    image = [image imageByApplyingTransform:CGAffineTransformMakeScale(
                                                                (double)w / sw, (double)h / sh)];
                    rectangle = CGRectMake(0, 0, w, h);
                }
                rendered = jpeg == 2 ? NULL : [context createCGImage:image fromRect:rectangle];
                if (jpeg == 2)
                    data = h264_frame(capture, image, width, height);
                if (rendered) {
                    NSBitmapImageRep *bitmap = [[NSBitmapImageRep alloc] initWithCGImage:rendered];
                    *width = (unsigned int)CGImageGetWidth(rendered);
                    *height = (unsigned int)CGImageGetHeight(rendered);
                    data = [bitmap representationUsingType:jpeg ? NSBitmapImageFileTypeJPEG
                                                                : NSBitmapImageFileTypePNG
                                                properties:jpeg
                                                               ? @{NSImageCompressionFactor : @0.75}
                                                               : @{}];
                }
            } @finally {
                if (rendered)
                    CGImageRelease(rendered);
                IOSurfaceUnlock(surface, kIOSurfaceLockReadOnly, NULL);
                CFRelease(surface);
            }
            if (![capture.peer valid]) {
                *failure = 7;
                return NULL;
            }
            if (!data || data.length > 64 * 1024 * 1024) {
                *failure = 4;
                return NULL;
            }
            unsigned char *bytes = malloc(data.length);
            if (!bytes) {
                *failure = 5;
                return NULL;
            }
            memcpy(bytes, data.bytes, data.length);
            *size = data.length;
            *failure = 0;
            return bytes;
        } @catch (NSException *exception) {
            (void)exception;
            *failure = 6;
            return NULL;
        }
    }
}
void sb_simulator_free(void *bytes) { free(bytes); }
int sb_simulator_keyframe(void *handle) { return ((__bridge SBCapture *)handle).encodedKeyframe; }
void sb_simulator_source_size(void *handle, unsigned int *width, unsigned int *height) {
    SBCapture *capture = (__bridge SBCapture *)handle;
    *width = capture.sourceWidth;
    *height = capture.sourceHeight;
}
int sb_simulator_hardware_encoder(void *handle) {
    SBCapture *capture = (__bridge SBCapture *)handle;
    if (!capture.encoder)
        return -1;
    CFTypeRef value = NULL;
    if (VTSessionCopyProperty(capture.encoder,
                              kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder, NULL,
                              &value) != noErr ||
        !value)
        return -1;
    BOOL enabled = CFGetTypeID(value) == CFBooleanGetTypeID() && CFBooleanGetValue(value);
    CFRelease(value);
    return enabled;
}
unsigned long long sb_simulator_activity(void *handle) {
    return ((__bridge SBCapture *)handle).damageCounter;
}
int sb_simulator_callbacks(void *handle) {
    return ((__bridge SBCapture *)handle).callbacksSupported;
}
void sb_simulator_force_keyframe(void *handle) {
    ((__bridge SBCapture *)handle).forceKeyframe = YES;
}
int sb_simulator_recording_start(void *handle, const char *path) {
    SBCapture *capture = (__bridge SBCapture *)handle;
    if (capture.recordPath)
        return 1;
    capture.recordingDrops = 0;
    capture.recordingFailed = NO;
    capture.recordPath = [NSString stringWithUTF8String:path];
    capture.forceKeyframe = YES;
    return capture.recordPath ? 0 : 1;
}
void *sb_simulator_recording_detach(void *handle, unsigned long long *drops) {
    SBCapture *capture = (__bridge SBCapture *)handle;
    *drops = capture.recordingDrops;
    if (!capture.writer || capture.recordingFailed) {
        [capture.writer cancelWriting];
        capture.writer = nil;
        capture.writerInput = nil;
        capture.recordPath = nil;
        return NULL;
    }
    AVAssetWriter *writer = capture.writer;
    [capture.writerInput markAsFinished];
    capture.writer = nil;
    capture.writerInput = nil;
    capture.recordPath = nil;
    return (__bridge_retained void *)writer;
}
int sb_simulator_recording_complete(void *handle) {
    @autoreleasepool {
        AVAssetWriter *writer = (__bridge AVAssetWriter *)handle;
        dispatch_semaphore_t finished = dispatch_semaphore_create(0);
        [writer finishWritingWithCompletionHandler:^{
            dispatch_semaphore_signal(finished);
        }];
        int result;
        if (dispatch_semaphore_wait(finished, dispatch_time(DISPATCH_TIME_NOW, 8 * NSEC_PER_SEC)) != 0) {
            [writer cancelWriting];
            result = 2;
        } else {
            result = writer.status == AVAssetWriterStatusCompleted ? 0 : 3;
        }
        return result;
    }
}
void sb_simulator_recording_dispose(void *handle) {
    @autoreleasepool {
        AVAssetWriter *writer = (__bridge_transfer AVAssetWriter *)handle;
        if (writer.status == AVAssetWriterStatusWriting)
            [writer cancelWriting];
    }
}
static int send_message(id<SBSimulatorObject> client, void *message) {
    if (!message)
        return 3;
    dispatch_semaphore_t finished = dispatch_semaphore_create(0);
    __block BOOL failed = NO;
    dispatch_queue_t queue =
        dispatch_queue_create("com.canonic.testrun.hid", DISPATCH_QUEUE_SERIAL);
    [client sendWithMessage:message
               freeWhenDone:YES
            completionQueue:queue
                 completion:^(NSError *error) {
                     failed = error != nil;
                     dispatch_semaphore_signal(finished);
                 }];
    if (dispatch_semaphore_wait(finished, dispatch_time(DISPATCH_TIME_NOW, 2 * NSEC_PER_SEC)) != 0)
        return 4;
    return failed ? 5 : 0;
}
typedef struct {
    uint32_t usage;
    uint32_t modifiers;
} SBStroke;
static int keys_client(id<SBSimulatorObject> client, const SBStroke *strokes, size_t count, BOOL (^valid)(void)) {
    @autoreleasepool {
        @try {
            typedef void *(*KeyboardBuilder)(uint32_t, uint32_t);
            KeyboardBuilder build =
                (KeyboardBuilder)dlsym(RTLD_DEFAULT, "IndigoHIDMessageForKeyboardArbitrary");
            if (!client || !build)
                return 2;
            const uint32_t modifierUsages[] = {225, 224, 226, 227};
            for (size_t index = 0; index < count; index++) {
                if (!valid()) return 7;
                if (!strokes[index].usage || strokes[index].usage > 255 ||
                    strokes[index].modifiers > 15)
                    return 3;
                int result = 0;
                for (unsigned int bit = 0; bit < 4; bit++)
                    if (strokes[index].modifiers & (1u << bit)) {
                        if (!valid()) return 7;
                        result = send_message(client, build(modifierUsages[bit], 1));
                        if (result)
                            break;
                    }
                if (!valid()) return 7;
                if (!result)
                    result = send_message(client, build(strokes[index].usage, 1));
                [NSThread sleepForTimeInterval:0.01];
                if (!valid()) return 7;
                int release = send_message(client, build(strokes[index].usage, 2));
                for (int bit = 3; bit >= 0; bit--)
                    if (strokes[index].modifiers & (1u << bit)) {
                        if (!valid()) return 7;
                        int status = send_message(client, build(modifierUsages[bit], 2));
                        if (status && !release)
                            release = status;
                    }
                if (result || release)
                    return result ? result : release;
            }
            return 0;
        } @catch (NSException *exception) {
            (void)exception;
            return 6;
        }
    }
}


// Coordinates are normalized in the full display's top-left space. The selected
// Xcode 26 interface uses six C parameters, including the by-value CGSize.
static int gesture_client(id<SBSimulatorObject> client, double x, double y, double endX,
                          double endY, unsigned int durationMs, BOOL (^valid)(void)) {
    @autoreleasepool {
        @try {
            if (!client || ![(id)client respondsToSelector:@selector(sendWithMessage:freeWhenDone:
                                                                     completionQueue:completion:)])
                return 2;
            typedef void *(*MouseBuilder)(CGPoint *, CGPoint *, unsigned int, unsigned long, CGSize,
                                          unsigned int);
            MouseBuilder build =
                (MouseBuilder)dlsym(RTLD_DEFAULT, "IndigoHIDMessageForMouseNSEvent");
            if (!build)
                return 3;
            CGPoint point = CGPointMake(x, y), delta = CGPointZero;
            if (!valid()) return 7;
            int result = send_message(
                client, build(&point, &delta, 0x32, NSEventTypeLeftMouseDown, CGSizeMake(1, 1), 0));
            if (result) {
                if (!valid()) return 7;
                (void)send_message(client, build(&point, &delta, 0x32, NSEventTypeLeftMouseUp,
                                                 CGSizeMake(1, 1), 0));
                return result;
            }
            unsigned int steps = MAX(1, durationMs / 16);
            for (unsigned int step = 1; step <= steps; step++) {
                [NSThread sleepForTimeInterval:(double)durationMs / steps / 1000.0];
                if (!valid())
                    return 7;
                point = CGPointMake(x + (endX - x) * step / steps, y + (endY - y) * step / steps);
                if (step < steps && (x != endX || y != endY)) {
                    result =
                        send_message(client, build(&point, &delta, 0x32, NSEventTypeLeftMouseDown,
                                                   CGSizeMake(1, 1), 0));
                    if (result)
                        break;
                }
            }
            if (!valid())
                return 7;
            int release = send_message(
                client, build(&point, &delta, 0x32, NSEventTypeLeftMouseUp, CGSizeMake(1, 1), 0));
            return result ? result : release;
        } @catch (NSException *exception) {
            (void)exception;
            return 6;
        }
    }
}

@interface SBInput : NSObject <SBBoundSimulator>
@property(nonatomic, strong) id<SBSimulatorObject> device;
@property(nonatomic, strong) id<SBSimulatorObject> client;
@property(nonatomic, strong) id<SBSimulatorObject> descriptor;
@property(nonatomic, strong) SBServicePeer *peer;
@property(nonatomic, strong) NSDictionary *incarnation;
@property(nonatomic) ino_t directoryNode;
@property(nonatomic) ino_t dataNode;
@property(nonatomic) dev_t directoryDevice;
@property(nonatomic) dev_t dataDevice;
@end
@implementation SBInput
@end

static NSDictionary *input_incarnation(id<SBSimulatorObject> device) {
    if (![(id)device respondsToSelector:@selector(devicePath)])
        return nil;
    NSString *file = [[device devicePath] stringByAppendingPathComponent:@"device.plist"];
    NSDictionary *record = [NSDictionary dictionaryWithContentsOfFile:file];
    if (![record[@"state"] isEqual:@3] || ![record[@"lastBootedAt"] isKindOfClass:NSDate.class] ||
        ![record[@"runtime"] isKindOfClass:NSString.class] ||
        ![record[@"UDID"] isKindOfClass:NSString.class] || [record[@"isDeleted"] boolValue])
        return nil;
    return @{
        @"boot" : record[@"lastBootedAt"],
        @"runtime" : record[@"runtime"],
        @"uuid" : record[@"UDID"]
    };
}
static BOOL input_valid(SBInput *input) {
    @try {
        NSString *path = [input.device devicePath];
        struct stat directory, data;
        return [input.peer valid] && lstat(path.fileSystemRepresentation, &directory) == 0 &&
               S_ISDIR(directory.st_mode) &&
               lstat([[path stringByAppendingPathComponent:@"data"] fileSystemRepresentation],
                     &data) == 0 &&
               S_ISDIR(data.st_mode) && directory.st_ino == input.directoryNode &&
               directory.st_dev == input.directoryDevice && data.st_ino == input.dataNode &&
               data.st_dev == input.dataDevice &&
               [input.incarnation isEqual:input_incarnation(input.device)];
    } @catch (NSException *exception) {
        (void)exception;
        return NO;
    }
}

void *sb_simulator_input_open(const char *developer, const char *uuid) {
    @autoreleasepool {
        @try {
            id<SBSimulatorObject> device = device_for_uuid(developer, uuid);
            Class cls = NSClassFromString(@"SimulatorKit.SimDeviceLegacyHIDClient");
            if (!device || !cls)
                return NULL;
            NSError *error = nil;
            id<SBSimulatorObject> client = [(id<SBSimulatorObject>)[cls alloc] initWithDevice:device error:&error];
            if (!client || ![(id)client respondsToSelector:@selector(sendWithMessage:freeWhenDone:
                                                                     completionQueue:completion:)])
                return NULL;
            for (id<SBSimulatorObject> port in [[device io] ioPorts]) {
                id<SBSimulatorObject> descriptor = [port descriptor];
                if (![(id)descriptor respondsToSelector:@selector(state)])
                    continue;
                id<SBSimulatorObject> state = [descriptor state];
                if (![(id)state respondsToSelector:@selector(displayClass)] ||
                    [state displayClass] != 0)
                    continue;
                SBInput *input = [SBInput new];
                input.device = device;
                input.client = client;
                input.descriptor = descriptor;
                input.peer = peer_for_device(device);
                input.incarnation = input_incarnation(device);
                NSString *path = [device devicePath];
                struct stat directory, data;
                if (!input.incarnation || lstat(path.fileSystemRepresentation, &directory) != 0 ||
                    lstat([[path stringByAppendingPathComponent:@"data"] fileSystemRepresentation],
                          &data) != 0)
                    return NULL;
                input.directoryNode = directory.st_ino;
                input.directoryDevice = directory.st_dev;
                input.dataNode = data.st_ino;
                input.dataDevice = data.st_dev;
                if (!input_valid(input))
                    return NULL;
                return (__bridge_retained void *)input;
            }
            return NULL;
        } @catch (NSException *exception) {
            (void)exception;
            return NULL;
        }
    }
}
int sb_simulator_input_gesture(void *handle, double x, double y, double endX, double endY,
                               unsigned int duration) {
    SBInput *input = (__bridge SBInput *)handle;
    return gesture_client(input.client, x, y, endX, endY, duration, ^BOOL {
        return input_valid(input);
    });
}
int sb_simulator_input_keys(void *handle, const SBStroke *strokes, size_t count) {
    SBInput *input = (__bridge SBInput *)handle;
    return keys_client(input.client, strokes, count, ^BOOL {
        return input_valid(input);
    });
}
int sb_simulator_input_release(void *handle, int touch, const uint32_t *keys, size_t count) {
    @try {
        if (!input_valid((__bridge SBInput *)handle))
            return 7;
        id<SBSimulatorObject> client = [(__bridge SBInput *)handle client];
        int result = 0;
        typedef void *(*MouseBuilder)(CGPoint *, CGPoint *, unsigned int, unsigned long, CGSize,
                                      unsigned int);
        MouseBuilder mouse = (MouseBuilder)dlsym(RTLD_DEFAULT, "IndigoHIDMessageForMouseNSEvent");
        if (touch) {
            if (!mouse)
                return 3;
            CGPoint point = CGPointZero, delta = CGPointZero;
            result = send_message(
                client, mouse(&point, &delta, 0x32, NSEventTypeLeftMouseUp, CGSizeMake(1, 1), 0));
        }
        typedef void *(*KeyboardBuilder)(uint32_t, uint32_t);
        KeyboardBuilder keyboard =
            (KeyboardBuilder)dlsym(RTLD_DEFAULT, "IndigoHIDMessageForKeyboardArbitrary");
        if (count && !keyboard)
            return 3;
        for (size_t i = 0; i < count; i++) {
            if (!input_valid((__bridge SBInput *)handle))
                return 7;
            int status = send_message(client, keyboard(keys[i], 2));
            if (!result)
                result = status;
        }
        return result;
    } @catch (NSException *exception) {
        (void)exception;
        return 6;
    }
}
