#import <AVFoundation/AVFoundation.h>
#import <CoreImage/CoreImage.h>
#import <ImageIO/ImageIO.h>
#include <math.h>

// One Rust owner serializes this writer's calls. It consumes the browser's
// independently decodable JPEG feed and never owns a browser, viewer or input.
@interface SBBrowserWriter : NSObject
@property(nonatomic, strong) AVAssetWriter *writer;
@property(nonatomic, strong) AVAssetWriterInput *input;
@property(nonatomic, strong) AVAssetWriterInputPixelBufferAdaptor *adaptor;
@property(nonatomic, strong) CIContext *context;
@property(nonatomic) uint32_t width;
@property(nonatomic) uint32_t height;
@end
@implementation SBBrowserWriter
@end

void *sb_browser_recording_open(const char *path, uint32_t width, uint32_t height) {
    @autoreleasepool {
        if (!path || !width || !height || width > 8192 || height > 8192 ||
            (uint64_t)width * height > 8000000 || (width & 1) || (height & 1))
            return NULL;
        SBBrowserWriter *job = [SBBrowserWriter new];
        NSError *error = nil;
        job.writer = [[AVAssetWriter alloc] initWithURL:[NSURL fileURLWithPath:@(path)]
                                              fileType:AVFileTypeMPEG4 error:&error];
        if (!job.writer) return NULL;
        job.writer.movieTimeScale = 1000000000;
        job.width = width;
        job.height = height;
        NSDictionary *settings = @{
            AVVideoCodecKey: AVVideoCodecTypeH264,
            AVVideoWidthKey: @(width), AVVideoHeightKey: @(height),
            AVVideoCompressionPropertiesKey: @{
                AVVideoAverageBitRateKey: @(MAX(500000, MIN((uint64_t)width * height * 4, 20000000))),
                AVVideoAllowFrameReorderingKey: @NO,
            },
        };
        job.input = [[AVAssetWriterInput alloc] initWithMediaType:AVMediaTypeVideo outputSettings:settings];
        job.input.expectsMediaDataInRealTime = YES;
        job.input.mediaTimeScale = 1000000000;
        job.adaptor = [[AVAssetWriterInputPixelBufferAdaptor alloc]
            initWithAssetWriterInput:job.input sourcePixelBufferAttributes:@{
                (id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_32BGRA),
                (id)kCVPixelBufferWidthKey: @(width), (id)kCVPixelBufferHeightKey: @(height),
                (id)kCVPixelBufferIOSurfacePropertiesKey: @{},
            }];
        job.context = [CIContext contextWithOptions:@{kCIContextUseSoftwareRenderer: @NO}];
        if (!job.adaptor || !job.context || ![job.writer canAddInput:job.input]) return NULL;
        [job.writer addInput:job.input];
        if (![job.writer startWriting]) return NULL;
        [job.writer startSessionAtSourceTime:kCMTimeZero];
        return (__bridge_retained void *)job;
    }
}

int sb_browser_recording_append(void *handle, const uint8_t *bytes, size_t length, uint64_t pts) {
    @autoreleasepool {
        if (!handle || !bytes || !length || length > 6 * 1024 * 1024 || pts > INT64_MAX) return -1;
        SBBrowserWriter *job = (__bridge SBBrowserWriter *)handle;
        if (job.writer.status != AVAssetWriterStatusWriting) return -1;
        if (!job.input.readyForMoreMediaData) return 1;
        NSData *data = [NSData dataWithBytesNoCopy:(void *)bytes length:length freeWhenDone:NO];
        CIImage *image = [CIImage imageWithData:data];
        CGRect extent = image.extent;
        if (!image || !isfinite(extent.size.width) || !isfinite(extent.size.height) ||
            extent.size.width <= 0 || extent.size.height <= 0 ||
            extent.size.width > 8192 || extent.size.height > 8192 ||
            extent.size.width * extent.size.height > 8000000) return -1;
        CVPixelBufferRef buffer = NULL;
        if (!job.adaptor.pixelBufferPool ||
            CVPixelBufferPoolCreatePixelBuffer(kCFAllocatorDefault, job.adaptor.pixelBufferPool, &buffer) != kCVReturnSuccess)
            return -1;
        double scale = MIN(job.width / extent.size.width, job.height / extent.size.height);
        double x = (job.width - extent.size.width * scale) / 2;
        double y = (job.height - extent.size.height * scale) / 2;
        image = [image imageByApplyingTransform:CGAffineTransformMakeTranslation(-extent.origin.x, -extent.origin.y)];
        image = [image imageByApplyingTransform:CGAffineTransformMakeScale(scale, scale)];
        image = [image imageByApplyingTransform:CGAffineTransformMakeTranslation(x, y)];
        CGRect canvas = CGRectMake(0, 0, job.width, job.height);
        CIImage *background = [[CIImage imageWithColor:[CIColor colorWithRed:0 green:0 blue:0 alpha:1]] imageByCroppingToRect:canvas];
        CIImage *composed = [[image imageByCompositingOverImage:background] imageByCroppingToRect:canvas];
        CGColorSpaceRef color = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
        [job.context render:composed toCVPixelBuffer:buffer bounds:canvas colorSpace:color];
        CGColorSpaceRelease(color);
        BOOL accepted = [job.adaptor appendPixelBuffer:buffer withPresentationTime:CMTimeMake((int64_t)pts, 1000000000)];
        CVPixelBufferRelease(buffer);
        return accepted ? 0 : -1;
    }
}

int sb_browser_recording_finish(void *handle, uint64_t end) {
    @autoreleasepool {
        if (!handle) return -1;
        SBBrowserWriter *job = (__bridge_transfer SBBrowserWriter *)handle;
        if (!end || end > INT64_MAX) { [job.writer cancelWriting]; return -1; }
        [job.writer endSessionAtSourceTime:CMTimeMake((int64_t)end, 1000000000)];
        [job.input markAsFinished];
        dispatch_semaphore_t completed = dispatch_semaphore_create(0);
        [job.writer finishWritingWithCompletionHandler:^{ dispatch_semaphore_signal(completed); }];
        if (dispatch_semaphore_wait(completed, dispatch_time(DISPATCH_TIME_NOW, 8 * NSEC_PER_SEC)) != 0) {
            [job.writer cancelWriting];
            return -1;
        }
        return job.writer.status == AVAssetWriterStatusCompleted ? 0 : -1;
    }
}

void sb_browser_recording_cancel(void *handle) {
    @autoreleasepool {
        if (handle) {
            SBBrowserWriter *job = (__bridge_transfer SBBrowserWriter *)handle;
            [job.writer cancelWriting];
        }
    }
}

// Independent decoded-pixel probe for owned recording acceptance fixtures.
int sb_browser_recording_corners(const char *path, uint8_t *rgb) {
    @autoreleasepool {
        if (!path || !rgb) return -1;
        AVURLAsset *asset = [AVURLAsset URLAssetWithURL:[NSURL fileURLWithPath:@(path)] options:nil];
        dispatch_semaphore_t ready = dispatch_semaphore_create(0);
        __block AVAssetTrack *track = nil;
        [asset loadTracksWithMediaType:AVMediaTypeVideo completionHandler:^(NSArray<AVAssetTrack *> *tracks, NSError *error) {
            if (!error) track = tracks.firstObject;
            dispatch_semaphore_signal(ready);
        }];
        if (dispatch_semaphore_wait(ready, dispatch_time(DISPATCH_TIME_NOW, 2 * NSEC_PER_SEC)) != 0 || !track) return -1;
        AVAssetReader *reader = [[AVAssetReader alloc] initWithAsset:asset error:nil];
        AVAssetReaderTrackOutput *output = [[AVAssetReaderTrackOutput alloc] initWithTrack:track
            outputSettings:@{(id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_32BGRA)}];
        if (!reader || ![reader canAddOutput:output]) return -1;
        [reader addOutput:output];
        if (![reader startReading]) return -1;
        CMSampleBufferRef sample = [output copyNextSampleBuffer];
        if (!sample) return -1;
        CVPixelBufferRef image = CMSampleBufferGetImageBuffer(sample);
        if (!image || CVPixelBufferGetWidth(image)<4 || CVPixelBufferGetHeight(image)<4 ||
            CVPixelBufferLockBaseAddress(image,kCVPixelBufferLock_ReadOnly)!=kCVReturnSuccess) {
            CFRelease(sample); [reader cancelReading]; return -1;
        }
        size_t width=CVPixelBufferGetWidth(image), height=CVPixelBufferGetHeight(image), stride=CVPixelBufferGetBytesPerRow(image);
        uint8_t *base=CVPixelBufferGetBaseAddress(image);
        for (size_t i=0;i<4;i++) {
            size_t x=(i%2==0)?width/4:3*width/4, y=(i<2)?height/4:3*height/4;
            uint8_t *pixel=base+y*stride+4*x;
            rgb[3*i]=pixel[2];rgb[3*i+1]=pixel[1];rgb[3*i+2]=pixel[0];
        }
        CVPixelBufferUnlockBaseAddress(image,kCVPixelBufferLock_ReadOnly);
        CFRelease(sample); [reader cancelReading];
        return 0;
    }
}
