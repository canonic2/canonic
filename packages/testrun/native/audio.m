// Bounded AAC-ELD mode-8 decoder. No playback, concealment, or synthetic PCM on failure.
#import <AudioToolbox/AudioToolbox.h>
#include <stdlib.h>

typedef struct {
    AudioConverterRef converter;
    const uint8_t *bytes;
    uint32_t length;
    bool pending;
    AudioStreamPacketDescription description;
} SBAudioDecoder;

static OSStatus Feed(AudioConverterRef converter, UInt32 *packets, AudioBufferList *data,
                     AudioStreamPacketDescription **description, void *opaque) {
    (void)converter;
    SBAudioDecoder *decoder = opaque;
    if (!decoder->pending) { *packets = 0; return 1; }
    *packets = 1;
    data->mNumberBuffers = 1;
    data->mBuffers[0] = (AudioBuffer){2, decoder->length, (void *)decoder->bytes};
    decoder->description = (AudioStreamPacketDescription){0, 0, decoder->length};
    if (description) *description = &decoder->description;
    decoder->pending = false;
    return noErr;
}
void *sb_audio_decoder_open(void) {
    SBAudioDecoder *decoder = calloc(1, sizeof(*decoder));
    if (!decoder) return NULL;
    AudioStreamBasicDescription src = {48000, kAudioFormatMPEG4AAC_ELD, 0, 0, 480, 0, 2, 0, 0};
    AudioStreamBasicDescription dst = {48000, kAudioFormatLinearPCM,
        kLinearPCMFormatFlagIsSignedInteger | kLinearPCMFormatFlagIsPacked, 4, 1, 4, 2, 16, 0};
    // ESDS with AAC-ELD ASC F8 E6 50 00: 48kHz, stereo, 480 samples/frame.
    const uint8_t cookie[] = {3,27,0,0,0,4,19,0x40,0x15,0,0,0,0,0,0,0,0,0,0,0,5,4,0xf8,0xe6,0x50,0,6,1,2};
    AudioStreamBasicDescription derived=src;
    UInt32 size = sizeof(derived);
    OSStatus status = AudioFormatGetProperty(kAudioFormatProperty_FormatInfo,sizeof(cookie),cookie,&size,&derived);
    if (status || derived.mSampleRate != 48000 || derived.mChannelsPerFrame != 2 || derived.mFramesPerPacket != 480) {
        free(decoder); return NULL;
    }
    if (AudioConverterNew(&src,&dst,&decoder->converter)) {free(decoder);return NULL;}
    const uint8_t asc[]={0xf8,0xe6,0x50,0};
    status=AudioConverterSetProperty(decoder->converter,kAudioConverterDecompressionMagicCookie,sizeof(asc),asc);
    if(status) status=AudioConverterSetProperty(decoder->converter,kAudioConverterDecompressionMagicCookie,sizeof(cookie),cookie);
    if(status) {
        AudioConverterDispose(decoder->converter);decoder->converter=NULL;
        status=AudioConverterNew(&derived,&dst,&decoder->converter);
        if(!status) status=AudioConverterSetProperty(decoder->converter,kAudioConverterDecompressionMagicCookie,sizeof(cookie),cookie);
    }
    if (status) {
        if (decoder->converter) AudioConverterDispose(decoder->converter);
        free(decoder); return NULL;
    }
    return decoder;
}
int32_t sb_audio_decoder_decode(void *opaque, const uint8_t *bytes, uint32_t length, int16_t *pcm) {
    SBAudioDecoder *decoder = opaque;
    if (!decoder || !bytes || !length || length > 65536 || !pcm) return -1;
    decoder->bytes=bytes; decoder->length=length; decoder->pending=true;
    AudioBufferList output = {1, {{2, 480*4, pcm}}};
    UInt32 frames=480;
    OSStatus status=AudioConverterFillComplexBuffer(decoder->converter,Feed,decoder,&frames,&output,NULL);
    if ((status != noErr && status != 1) || decoder->pending || frames != 480 || output.mBuffers[0].mDataByteSize != 480*4)
        return status != noErr ? status : -1;
    return 0;
}
void sb_audio_decoder_close(void *opaque) {
    SBAudioDecoder *decoder=opaque;
    if (decoder) { AudioConverterDispose(decoder->converter); free(decoder); }
}
