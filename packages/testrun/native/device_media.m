// Typed HEVC configuration, live JPEG observations and compressed MP4 muxing.
#import <AVFoundation/AVFoundation.h>
#import <CoreImage/CoreImage.h>
#import <CoreMedia/CoreMedia.h>
#import <Foundation/Foundation.h>
#import <ImageIO/ImageIO.h>
#import <VideoToolbox/VideoToolbox.h>

@interface SBHevcDecoder : NSObject
@property(nonatomic) VTDecompressionSessionRef session;
@property(nonatomic) CVPixelBufferRef image;
@property(nonatomic) uint64_t pts;
@property(nonatomic) OSStatus decodeError;
@property(nonatomic, strong) CIContext *context;
@property(nonatomic) CGRect aperture;
@property(nonatomic) NSUInteger pending;
@end
@implementation SBHevcDecoder
- (void)dealloc {
    if (_session) {
        VTDecompressionSessionInvalidate(_session);
        CFRelease(_session);
    }
    if (_image)
        CVPixelBufferRelease(_image);
}
@end
// The decoder below consumes any validated CoreMedia video format, including AVC.
void *sb_android_h264_format(const uint8_t *sps, size_t sn, const uint8_t *pps, size_t pn,
                            uint32_t width, uint32_t height) {
    if (!sn || !pn || sn > 65535 || pn > 65535) return NULL;
    const uint8_t *sets[] = {sps, pps};
    size_t sizes[] = {sn, pn};
    CMVideoFormatDescriptionRef format = NULL;
    if (CMVideoFormatDescriptionCreateFromH264ParameterSets(kCFAllocatorDefault, 2, sets,
                                                           sizes, 4, &format) != noErr) return NULL;
    CMVideoDimensions size = CMVideoFormatDescriptionGetDimensions(format);
    if (size.width <= 0 || size.height <= 0 || size.width != (int32_t)width ||
        size.height != (int32_t)height || size.width > 8192 || size.height > 8192 ||
        (uint64_t)size.width * size.height > 16777216) {
        CFRelease(format); return NULL;
    }
    return (void *)format;
}
static void Decoded(void *reference, void *source, OSStatus status, VTDecodeInfoFlags flags,
                    CVImageBufferRef image, CMTime pts, CMTime duration) {
    (void)source;
    (void)flags;
    (void)duration;
    SBHevcDecoder *decoder = (__bridge SBHevcDecoder *)reference;
    @synchronized(decoder) {
        if (decoder.pending)
            decoder.pending--;
        if (status != noErr) decoder.decodeError = status;
        if (status != noErr || !image || !CMTIME_IS_NUMERIC(pts))
            return;
        if (decoder.image)
            CVPixelBufferRelease(decoder.image);
        decoder.image = CVPixelBufferRetain(image);
        decoder.pts =
            (uint64_t)CMTimeConvertScale(pts, 1000000000, kCMTimeRoundingMethod_Default).value;
    }
}
void *sb_hevc_decoder_open(void *format, uint32_t width, uint32_t height) {
    @autoreleasepool {
        SBHevcDecoder *decoder = [SBHevcDecoder new];
        CMVideoDimensions coded = CMVideoFormatDescriptionGetDimensions(format);
        if (coded.width <= 0 || coded.height <= 0 || width == 0 || height == 0 ||
            width > (uint32_t)coded.width || height > (uint32_t)coded.height)
            return NULL;
        decoder.aperture = CGRectMake(0, coded.height - height, width, height);
        VTDecompressionOutputCallbackRecord callback = {Decoded, (__bridge void *)decoder};
        NSDictionary *attributes =
            @{(id)kCVPixelBufferPixelFormatTypeKey : @(kCVPixelFormatType_32BGRA)};
        VTDecompressionSessionRef session = NULL;
        if (VTDecompressionSessionCreate(kCFAllocatorDefault, format, NULL,
                                         (__bridge CFDictionaryRef)attributes, &callback,
                                         &session) != noErr)
            return NULL;
        decoder.session = session;
        VTSessionSetProperty(session, kVTDecompressionPropertyKey_RealTime, kCFBooleanTrue);
        return (__bridge_retained void *)decoder;
    }
}
void sb_hevc_decoder_close(void *handle) {
    @autoreleasepool {
        SBHevcDecoder *decoder = (__bridge_transfer SBHevcDecoder *)handle;
        if (decoder.session) {
            VTDecompressionSessionWaitForAsynchronousFrames(decoder.session);
            VTDecompressionSessionInvalidate(decoder.session);
        }
    }
}
int sb_hevc_decoder_append(void *handle, void *format, const uint8_t *bytes, size_t length,
                           uint64_t pts) {
    @autoreleasepool {
        SBHevcDecoder *decoder = (__bridge SBHevcDecoder *)handle;
        CMBlockBufferRef block = NULL;
        CMSampleBufferRef sample = NULL;
        // Queue pressure is not stream corruption. Drain outstanding callbacks
        // without holding their lock, preserving predictive reference frames.
        BOOL drain = NO;
        @synchronized(decoder) {
            drain = decoder.pending >= 4;
        }
        if (drain) {
            OSStatus drained = VTDecompressionSessionWaitForAsynchronousFrames(decoder.session);
            if (drained != noErr)
                return (int)drained;
        }
        if (CMBlockBufferCreateWithMemoryBlock(kCFAllocatorDefault, NULL, length,
                                               kCFAllocatorDefault, NULL, 0, length, 0,
                                               &block) != noErr)
            return -1;
        OSStatus status = CMBlockBufferReplaceDataBytes(bytes, block, 0, length);
        CMSampleTimingInfo timing = {kCMTimeInvalid, CMTimeMake(pts, 1000000000), kCMTimeInvalid};
        if (status == noErr)
            status = CMSampleBufferCreateReady(kCFAllocatorDefault, block, format, 1, 1, &timing, 1,
                                               &length, &sample);
        if (status == noErr) {
            @synchronized(decoder) {
                decoder.pending++;
            }
            status = VTDecompressionSessionDecodeFrame(
                decoder.session, sample, kVTDecodeFrame_EnableAsynchronousDecompression, NULL,
                NULL);
            if (status != noErr) {
                @synchronized(decoder) {
                    if (decoder.pending)
                        decoder.pending--;
                }
            }
        }
        if (sample)
            CFRelease(sample);
        CFRelease(block);
        return (int)status;
    }
}
int sb_hevc_decoder_snapshot(void *handle, uint8_t **bytes, size_t *length, uint64_t *pts,
                             uint32_t *width, uint32_t *height) {
    @autoreleasepool {
        SBHevcDecoder *decoder = (__bridge SBHevcDecoder *)handle;
        CVPixelBufferRef buffer = NULL;
        @synchronized(decoder) {
            if (decoder.image) {
                buffer = CVPixelBufferRetain(decoder.image);
                *pts = decoder.pts;
            }
        }
        if (!buffer)
            return 1;
        CIImage *image = [CIImage imageWithCVPixelBuffer:buffer];
        CGRect clean = decoder.aperture;
        if (CGRectIsEmpty(clean) || !CGRectContainsRect(image.extent, clean)) {
            CVPixelBufferRelease(buffer);
            return -1;
        }
        image = [image imageByCroppingToRect:clean];
        *width = (uint32_t)clean.size.width;
        *height = (uint32_t)clean.size.height;
        double factor = MIN(1, 960.0 / MAX(clean.size.width, clean.size.height));
        image = [image imageByApplyingTransform:CGAffineTransformMakeTranslation(-clean.origin.x,
                                                                                 -clean.origin.y)];
        image = [image imageByApplyingTransform:CGAffineTransformMakeScale(factor, factor)];
        CGColorSpaceRef color = CGColorSpaceCreateDeviceRGB();
        // GPU-backed JPEG setup is independent of decoded-frame readiness.
        // Construct it only for an actual snapshot, after geometry validation.
        if (!decoder.context)
            decoder.context = [CIContext contextWithOptions:@{kCIContextUseSoftwareRenderer : @NO}];
        NSData *data = [decoder.context
            JPEGRepresentationOfImage:image
                           colorSpace:color
                              options:@{
                                  (id)kCGImageDestinationLossyCompressionQuality : @0.65
                              }];
        CGColorSpaceRelease(color);
        CVPixelBufferRelease(buffer);
        if (!data || data.length > 2 * 1024 * 1024)
            return -1;
        *bytes = malloc(data.length);
        if (!*bytes)
            return -1;
        memcpy(*bytes, data.bytes, data.length);
        *length = data.length;
        return 0;
    }
}
int sb_hevc_decoder_state(void *handle, uint64_t *pts) {
    SBHevcDecoder *decoder = (__bridge SBHevcDecoder *)handle;
    @synchronized(decoder) {
        if (!decoder.image)
            return 1;
        *pts = decoder.pts;
        return 0;
    }
}
int sb_android_decoder_state(void *handle, uint64_t *pts) {
    SBHevcDecoder *decoder = (__bridge SBHevcDecoder *)handle;
    @synchronized(decoder) {
        if (decoder.decodeError) return -1;
        if (!decoder.image) return 1;
        *pts = decoder.pts;
        return 0;
    }
}

void *sb_hevc_format(const uint8_t *vps, size_t vn, const uint8_t *sps, size_t sn,
                     const uint8_t *pps, size_t pn, uint8_t **config, size_t *length,
                     uint32_t *width, uint32_t *height) {
    const uint8_t *parameters[] = {vps, sps, pps};
    const size_t sizes[] = {vn, sn, pn};
    CMVideoFormatDescriptionRef format = NULL;
    if (CMVideoFormatDescriptionCreateFromHEVCParameterSets(kCFAllocatorDefault, 3, parameters,
                                                            sizes, 4, NULL, &format) != noErr)
        return NULL;
    NSDictionary *atoms = (__bridge NSDictionary *)CMFormatDescriptionGetExtension(
        format, kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms);
    NSData *hvcc = atoms[@"hvcC"];
    CMVideoDimensions dims = CMVideoFormatDescriptionGetDimensions(format);
    if (!hvcc || hvcc.length > 65536 || dims.width <= 0 || dims.height <= 0) {
        CFRelease(format);
        return NULL;
    }
    *config = malloc(hvcc.length);
    if (!*config) {
        CFRelease(format);
        return NULL;
    }
    memcpy(*config, hvcc.bytes, hvcc.length);
    *length = hvcc.length;
    *width = (uint32_t)dims.width;
    *height = (uint32_t)dims.height;
    return (void *)format;
}
void sb_hevc_format_release(void *format) {
    if (format)
        CFRelease(format);
}
void sb_hevc_bytes_release(void *bytes) { free(bytes); }

@interface SBHevcWriter : NSObject
@property(nonatomic, strong) AVAssetWriter *writer;
@property(nonatomic, strong) AVAssetWriterInput *input;
@end
@implementation SBHevcWriter
@end

void *sb_hevc_recording_open(const char *path, void *format) {
    @autoreleasepool {
        NSError *error = nil;
        SBHevcWriter *job = [SBHevcWriter new];
        job.writer = [[AVAssetWriter alloc] initWithURL:[NSURL fileURLWithPath:@(path)]
                                               fileType:AVFileTypeMPEG4
                                                  error:&error];
        job.input = [[AVAssetWriterInput alloc] initWithMediaType:AVMediaTypeVideo
                                                   outputSettings:nil
                                                 sourceFormatHint:format];
        job.input.expectsMediaDataInRealTime = YES;
        if (!job.writer || ![job.writer canAddInput:job.input])
            return NULL;
        [job.writer addInput:job.input];
        if (![job.writer startWriting])
            return NULL;
        [job.writer startSessionAtSourceTime:kCMTimeZero];
        return (__bridge_retained void *)job;
    }
}
int sb_hevc_recording_append(void *handle, void *format, const uint8_t *bytes, size_t length,
                             uint64_t pts, int key) {
    @autoreleasepool {
        SBHevcWriter *job = (__bridge SBHevcWriter *)handle;
        if (!job.input.readyForMoreMediaData)
            return 1;
        CMBlockBufferRef block = NULL;
        CMSampleBufferRef sample = NULL;
        OSStatus status = CMBlockBufferCreateWithMemoryBlock(
            kCFAllocatorDefault, NULL, length, kCFAllocatorDefault, NULL, 0, length, 0, &block);
        if (status != noErr)
            return -1;
        status = CMBlockBufferReplaceDataBytes(bytes, block, 0, length);
        CMSampleTimingInfo timing = {kCMTimeInvalid, CMTimeMake((int64_t)pts, 1000000000),
                                     kCMTimeInvalid};
        if (status == noErr)
            status = CMSampleBufferCreateReady(kCFAllocatorDefault, block, format, 1, 1, &timing, 1,
                                               &length, &sample);
        if (status == noErr) {
            CFArrayRef array = CMSampleBufferGetSampleAttachmentsArray(sample, true);
            CFMutableDictionaryRef attachment =
                (CFMutableDictionaryRef)CFArrayGetValueAtIndex(array, 0);
            CFDictionarySetValue(attachment, kCMSampleAttachmentKey_NotSync,
                                 key ? kCFBooleanFalse : kCFBooleanTrue);
            status = [job.input appendSampleBuffer:sample] ? noErr : -1;
        }
        if (sample)
            CFRelease(sample);
        CFRelease(block);
        return status == noErr ? 0 : -1;
    }
}
int sb_hevc_recording_finish(void *handle) {
    @autoreleasepool {
        SBHevcWriter *job = (__bridge_transfer SBHevcWriter *)handle;
        [job.input markAsFinished];
        dispatch_semaphore_t completed = dispatch_semaphore_create(0);
        [job.writer finishWritingWithCompletionHandler:^{
            dispatch_semaphore_signal(completed);
        }];
        if (dispatch_semaphore_wait(completed,
                                    dispatch_time(DISPATCH_TIME_NOW, 8 * NSEC_PER_SEC)) != 0) {
            [job.writer cancelWriting];
            return -1;
        }
        return job.writer.status == AVAssetWriterStatusCompleted ? 0 : -1;
    }
}
void sb_hevc_recording_cancel(void *handle) {
    @autoreleasepool {
        SBHevcWriter *job = (__bridge_transfer SBHevcWriter *)handle;
        [job.writer cancelWriting];
    }
}

int sb_video_track_verify(const char *path, uint64_t *frames, uint32_t *width, uint32_t *height,
                          uint64_t *duration, double budgetSeconds) {
    @autoreleasepool {
        if (!(budgetSeconds > 0.0 && budgetSeconds <= 60.0)) return -1;
        AVURLAsset *asset = [AVURLAsset URLAssetWithURL:[NSURL fileURLWithPath:@(path)]
                                                options:nil];
        dispatch_semaphore_t ready = dispatch_semaphore_create(0);
        __block AVAssetTrack *track = nil;
        [asset loadTracksWithMediaType:AVMediaTypeVideo
                     completionHandler:^(NSArray<AVAssetTrack *> *tracks, NSError *error) {
                         if (!error)
                             track = tracks.firstObject;
                         dispatch_semaphore_signal(ready);
                     }];
        if (dispatch_semaphore_wait(ready, dispatch_time(DISPATCH_TIME_NOW, 2 * NSEC_PER_SEC)) !=
                0 ||
            !track)
            return -1;
        NSError *error = nil;
        AVAssetReader *reader = [[AVAssetReader alloc] initWithAsset:asset error:&error];
        AVAssetReaderTrackOutput *output = [[AVAssetReaderTrackOutput alloc]
             initWithTrack:track
            outputSettings:@{
                (id)kCVPixelBufferPixelFormatTypeKey :
                    @(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange)
            }];
        output.alwaysCopiesSampleData = NO;
        if (!reader || ![reader canAddOutput:output])
            return -1;
        [reader addOutput:output];
        if (![reader startReading])
            return -1;
        CMTime previous = kCMTimeInvalid;
        *frames = 0;
        double deadline = NSProcessInfo.processInfo.systemUptime + budgetSeconds;
        while (true) {
            if (NSProcessInfo.processInfo.systemUptime > deadline) {
                [reader cancelReading];
                return -2;
            }
            CMSampleBufferRef sample = [output copyNextSampleBuffer];
            if (!sample)
                break;
            CVImageBufferRef image = CMSampleBufferGetImageBuffer(sample);
            CMTime pts = CMSampleBufferGetPresentationTimeStamp(sample);
            if (!image || !CMTIME_IS_NUMERIC(pts) ||
                (CMTIME_IS_VALID(previous) && CMTimeCompare(pts, previous) <= 0)) {
                CFRelease(sample);
                [reader cancelReading];
                return -1;
            }
            uint32_t w = (uint32_t)CVPixelBufferGetWidth(image),
                     h = (uint32_t)CVPixelBufferGetHeight(image);
            if (*frames == 0) {
                *width = w;
                *height = h;
            } else if (w != *width || h != *height) {
                CFRelease(sample);
                [reader cancelReading];
                return -1;
            }
            previous = pts;
            *frames += 1;
            CFRelease(sample);
        }
        if (reader.status != AVAssetReaderStatusCompleted || *frames == 0)
            return -1;
        *duration = (uint64_t)(CMTimeGetSeconds(previous) * 1000000000.0);
        return 0;
    }
}

int sb_hevc_recording_verify(const char *path, uint64_t *frames, uint32_t *width, uint32_t *height,
                             uint64_t *duration) {
    return sb_video_track_verify(path, frames, width, height, duration, 8.0);
}
