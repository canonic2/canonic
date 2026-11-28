// Windows Media Foundation H.264 encoder and full-track decoder.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <codecapi.h>
#include <mfapi.h>
#include <mferror.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <wrl/client.h>
#include <algorithm>
#include <atomic>
#include <cstdint>
#include <cstring>
#include <memory>
#include <new>
#include <utility>

using Microsoft::WRL::ComPtr;

namespace {
struct Library {
    HMODULE module;
    explicit Library(const wchar_t* name)
        : module(LoadLibraryExW(name, nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32)) {}
    ~Library() { if (module) FreeLibrary(module); }
    Library(const Library&) = delete;
    Library& operator=(const Library&) = delete;

    template <class T>
    T resolve(const char* name) const {
        FARPROC address = module ? GetProcAddress(module, name) : nullptr;
        T function = nullptr;
        static_assert(sizeof(function) == sizeof(address));
        std::memcpy(&function, &address, sizeof(function));
        return function;
    }
};

struct Functions {
    Library platform{L"mfplat.dll"};
    Library readwrite{L"mfreadwrite.dll"};
    decltype(&MFStartup) startup = platform.resolve<decltype(&MFStartup)>("MFStartup");
    decltype(&MFShutdown) shutdown = platform.resolve<decltype(&MFShutdown)>("MFShutdown");
    decltype(&MFCreateFile) file = platform.resolve<decltype(&MFCreateFile)>("MFCreateFile");
    decltype(&MFCreateAttributes) attributes =
        platform.resolve<decltype(&MFCreateAttributes)>("MFCreateAttributes");
    decltype(&MFCreateMediaType) media_type =
        platform.resolve<decltype(&MFCreateMediaType)>("MFCreateMediaType");
    decltype(&MFCreateMemoryBuffer) buffer =
        platform.resolve<decltype(&MFCreateMemoryBuffer)>("MFCreateMemoryBuffer");
    decltype(&MFCreateSample) sample = platform.resolve<decltype(&MFCreateSample)>("MFCreateSample");
    decltype(&MFCreateSinkWriterFromURL) sink =
        readwrite.resolve<decltype(&MFCreateSinkWriterFromURL)>("MFCreateSinkWriterFromURL");
    decltype(&MFCreateSourceReaderFromURL) source =
        readwrite.resolve<decltype(&MFCreateSourceReaderFromURL)>("MFCreateSourceReaderFromURL");

    bool available() const {
        return startup && shutdown && file && attributes && media_type && buffer && sample &&
            sink && source;
    }
};

struct Platform {
    Functions api;
    HRESULT com = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    HRESULT status = FAILED(com) ? com : api.available() ? api.startup(MF_VERSION, MFSTARTUP_FULL)
                                                      : HRESULT_FROM_WIN32(ERROR_MOD_NOT_FOUND);
    ~Platform() {
        if (SUCCEEDED(status)) api.shutdown();
        if (SUCCEEDED(com)) CoUninitialize();
    }
};

class Finalized : public IMFSinkWriterCallback {
    std::atomic<ULONG> refs{1};
    std::atomic<HRESULT> result{E_PENDING};
    HANDLE event = CreateEventW(nullptr, TRUE, FALSE, nullptr);

public:
    ~Finalized() { if (event) CloseHandle(event); }
    bool valid() const { return event != nullptr; }

    STDMETHODIMP QueryInterface(REFIID id, void** value) override {
        if (!value) return E_POINTER;
        *value = nullptr;
        if (id != __uuidof(IUnknown) && id != __uuidof(IMFSinkWriterCallback)) return E_NOINTERFACE;
        *value = static_cast<IMFSinkWriterCallback*>(this);
        AddRef();
        return S_OK;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return ++refs; }
    STDMETHODIMP_(ULONG) Release() override {
        ULONG remaining = --refs;
        if (remaining == 0) delete this;
        return remaining;
    }
    STDMETHODIMP OnFinalize(HRESULT status) override {
        result.store(status, std::memory_order_release);
        SetEvent(event);
        return S_OK;
    }
    STDMETHODIMP OnMarker(DWORD, LPVOID) override { return S_OK; }

    HRESULT wait() const {
        const DWORD status = WaitForSingleObject(event, 8000);
        if (status == WAIT_OBJECT_0) return result.load(std::memory_order_acquire);
        return HRESULT_FROM_WIN32(status == WAIT_TIMEOUT ? ERROR_TIMEOUT : GetLastError());
    }
};

UINT64 pair(UINT32 first, UINT32 second) {
    return (static_cast<UINT64>(first) << 32) | second;
}

HRESULT video_type(const Functions& api, const GUID& subtype, UINT32 width, UINT32 height,
                   IMFMediaType** output) {
    ComPtr<IMFMediaType> type;
    HRESULT hr = api.media_type(type.GetAddressOf());
    if (SUCCEEDED(hr)) hr = type->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    if (SUCCEEDED(hr)) hr = type->SetGUID(MF_MT_SUBTYPE, subtype);
    if (SUCCEEDED(hr)) hr = type->SetUINT64(MF_MT_FRAME_SIZE, pair(width, height));
    if (SUCCEEDED(hr)) hr = type->SetUINT64(MF_MT_FRAME_RATE, pair(30, 1));
    if (SUCCEEDED(hr)) hr = type->SetUINT64(MF_MT_PIXEL_ASPECT_RATIO, pair(1, 1));
    if (SUCCEEDED(hr)) hr = type->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
    if (SUCCEEDED(hr)) hr = type->SetUINT32(MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT601);
    if (SUCCEEDED(hr)) hr = type->SetUINT32(MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235);
    if (SUCCEEDED(hr)) *output = type.Detach();
    return hr;
}

struct Writer {
    Platform platform;
    ComPtr<Finalized> finalized;
    ComPtr<IMFSinkWriter> writer;
    ComPtr<IMFSample> pending;
    DWORD index = 0;
    DWORD frame_size = 0;
    LONGLONG time = 0;
    HRESULT write_failure = S_OK;

    HRESULT open(const wchar_t* path, UINT32 width, UINT32 height) {
        if (FAILED(platform.status)) return platform.status;
        frame_size = width * height * 3 / 2;
        const auto& api = platform.api;
        finalized.Attach(new (std::nothrow) Finalized());
        if (!finalized || !finalized->valid()) return E_OUTOFMEMORY;
        ComPtr<IMFAttributes> options;
        HRESULT hr = api.attributes(options.GetAddressOf(), 1);
        if (SUCCEEDED(hr)) hr = options->SetUnknown(MF_SINK_WRITER_ASYNC_CALLBACK, finalized.Get());
        ComPtr<IMFByteStream> file;
        if (SUCCEEDED(hr)) hr = api.file(MF_ACCESSMODE_READWRITE, MF_OPENMODE_FAIL_IF_EXIST,
                                         MF_FILEFLAGS_NONE, path, file.GetAddressOf());
        if (SUCCEEDED(hr)) hr = api.sink(path, file.Get(), options.Get(), writer.GetAddressOf());
        ComPtr<IMFMediaType> encoded;
        if (SUCCEEDED(hr)) hr = video_type(api, MFVideoFormat_H264, width, height, encoded.GetAddressOf());
        if (SUCCEEDED(hr)) hr = encoded->SetUINT32(MF_MT_AVG_BITRATE,
                                                   std::clamp(width * height * 4u, 500000u, 20000000u));
        if (SUCCEEDED(hr)) hr = encoded->SetUINT32(MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base);
        if (SUCCEEDED(hr)) hr = writer->AddStream(encoded.Get(), &index);
        ComPtr<IMFMediaType> raw;
        if (SUCCEEDED(hr)) hr = video_type(api, MFVideoFormat_NV12, width, height, raw.GetAddressOf());
        if (SUCCEEDED(hr)) hr = raw->SetUINT32(MF_MT_DEFAULT_STRIDE, width);
        if (SUCCEEDED(hr)) hr = writer->SetInputMediaType(index, raw.Get(), nullptr);
        if (SUCCEEDED(hr)) hr = writer->BeginWriting();
        return hr;
    }

    HRESULT append(const std::uint8_t* bytes, size_t length, LONGLONG time_ns) {
        if (FAILED(write_failure)) return write_failure;
        if (!bytes || length != frame_size || time_ns < 0) return E_INVALIDARG;
        const auto& api = platform.api;
        ComPtr<IMFMediaBuffer> buffer;
        HRESULT hr = api.buffer(frame_size, buffer.GetAddressOf());
        BYTE* data = nullptr;
        if (SUCCEEDED(hr)) hr = buffer->Lock(&data, nullptr, nullptr);
        if (SUCCEEDED(hr)) {
            std::memcpy(data, bytes, frame_size);
            hr = buffer->Unlock();
        }
        if (SUCCEEDED(hr)) hr = buffer->SetCurrentLength(frame_size);
        ComPtr<IMFSample> next;
        if (SUCCEEDED(hr)) hr = api.sample(next.GetAddressOf());
        if (SUCCEEDED(hr)) hr = next->AddBuffer(buffer.Get());
        const LONGLONG next_time = pending ? std::max(time + 1, time_ns / 100) : time_ns / 100;
        if (SUCCEEDED(hr)) hr = next->SetSampleTime(next_time);
        if (SUCCEEDED(hr) && pending) hr = submit(next_time);
        if (SUCCEEDED(hr)) {
            pending = std::move(next);
            time = next_time;
        }
        return hr;
    }

    HRESULT finish(LONGLONG end_ns) {
        if (FAILED(write_failure)) return write_failure;
        if (!pending || end_ns <= 0) return E_INVALIDARG;
        HRESULT hr = submit(std::max(time + 1, end_ns / 100));
        if (SUCCEEDED(hr)) hr = writer->Finalize();
        if (SUCCEEDED(hr)) hr = finalized->wait();
        return hr;
    }

private:
    HRESULT submit(LONGLONG end) {
        HRESULT hr = pending->SetSampleDuration(end - time);
        if (SUCCEEDED(hr)) {
            hr = writer->WriteSample(index, pending.Get());
            if (FAILED(hr)) write_failure = hr;
        }
        return hr;
    }
};

HRESULT decoded_layout(IMFMediaType* type, UINT32 width, UINT32 height, DWORD* bytes) {
    GUID major{}, subtype{};
    UINT64 size = 0;
    if (FAILED(type->GetGUID(MF_MT_MAJOR_TYPE, &major)) || major != MFMediaType_Video ||
        FAILED(type->GetGUID(MF_MT_SUBTYPE, &subtype)) || subtype != MFVideoFormat_NV12 ||
        FAILED(type->GetUINT64(MF_MT_FRAME_SIZE, &size)))
        return MF_E_INVALIDMEDIATYPE;
    const UINT32 coded_width = static_cast<UINT32>(size >> 32);
    const UINT32 coded_height = static_cast<UINT32>(size);
    if (size != pair(width, height)) {
        // H.264 output can include macroblock padding. Accept only its bounded
        // coded dimensions and an explicit, exact visible image aperture.
        if (coded_width < width || coded_width > ((width + 15u) & ~15u) ||
            coded_height < height || coded_height > ((height + 15u) & ~15u) ||
            (coded_width & 1) || (coded_height & 1)) return MF_E_INVALIDMEDIATYPE;
        MFVideoArea aperture{};
        UINT32 length = 0;
        HRESULT hr = type->GetBlob(MF_MT_MINIMUM_DISPLAY_APERTURE,
                                   reinterpret_cast<UINT8*>(&aperture), sizeof(aperture), &length);
        if (hr == MF_E_ATTRIBUTENOTFOUND)
            hr = type->GetBlob(MF_MT_GEOMETRIC_APERTURE,
                               reinterpret_cast<UINT8*>(&aperture), sizeof(aperture), &length);
        if (FAILED(hr) || length != sizeof(aperture) || aperture.OffsetX.value ||
            aperture.OffsetX.fract || aperture.OffsetY.value || aperture.OffsetY.fract ||
            aperture.Area.cx != static_cast<LONG>(width) ||
            aperture.Area.cy != static_cast<LONG>(height)) return MF_E_INVALIDMEDIATYPE;
    }
    *bytes = coded_width * coded_height * 3 / 2;
    return S_OK;
}

HRESULT decode(IMFSourceReader* reader, UINT32 width, UINT32 height, UINT64 limit,
               UINT64* frames, UINT64* last_pts, UINT32* phase) {
    LONGLONG previous = -1;
    UINT32 empty = 0;
    DWORD minimum_bytes = width * height * 3 / 2;
    const ULONGLONG started = GetTickCount64();
    for (;;) {
        if (GetTickCount64() - started >= 60000) return HRESULT_FROM_WIN32(ERROR_TIMEOUT);
        DWORD flags = 0;
        LONGLONG time = 0;
        ComPtr<IMFSample> sample;
        *phase = 70;
        const HRESULT hr = reader->ReadSample(0, 0, nullptr, &flags, &time, sample.GetAddressOf());
        if (FAILED(hr)) return hr;
        *phase = 71;
        if (flags & (MF_SOURCE_READERF_ERROR | MF_SOURCE_READERF_NATIVEMEDIATYPECHANGED))
            return MF_E_INVALIDMEDIATYPE;
        if (flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED) {
            *phase = 72;
            ComPtr<IMFMediaType> current;
            if (FAILED(reader->GetCurrentMediaType(0, current.GetAddressOf())) ||
                FAILED(decoded_layout(current.Get(), width, height, &minimum_bytes)))
                return MF_E_INVALIDMEDIATYPE;
        }
        if (sample) {
            *phase = 73;
            DWORD bytes = 0;
            if (FAILED(sample->GetTotalLength(&bytes)) || bytes < minimum_bytes)
                return MF_E_INVALIDMEDIATYPE;
            *phase = 74;
            if (time <= previous) return MF_E_INVALIDMEDIATYPE;
            *phase = 75;
            if (++*frames > limit) return MF_E_INVALIDMEDIATYPE;
            previous = time;
            empty = 0;
        } else if (++empty > 128) {
            *phase = 77;
            return MF_E_INVALIDMEDIATYPE;
        }
        if (flags & MF_SOURCE_READERF_ENDOFSTREAM) {
            *phase = 76;
            if (previous < 0) return MF_E_INVALIDMEDIATYPE;
            *last_pts = static_cast<UINT64>(previous) * 100;
            return S_OK;
        }
    }
}
} // namespace

extern "C" int sb_browser_mf_available() { return Functions().available(); }

extern "C" std::int32_t sb_browser_mf_open(const wchar_t* path, UINT32 width, UINT32 height,
                                            void** output) {
    if (!path || !output || !width || !height || width > 8192 || height > 8192 ||
        width * height > 8000000 || (width & 1) || (height & 1)) return E_INVALIDARG;
    std::unique_ptr<Writer> writer(new (std::nothrow) Writer());
    if (!writer) return E_OUTOFMEMORY;
    HRESULT hr = writer->open(path, width, height);
    if (SUCCEEDED(hr)) *output = writer.release();
    return hr;
}

extern "C" std::int32_t sb_browser_mf_append(void* handle, const std::uint8_t* bytes, size_t length,
                                              std::int64_t time_ns) {
    return static_cast<Writer*>(handle)->append(bytes, length, time_ns);
}

extern "C" std::int32_t sb_browser_mf_finish(void* handle, std::int64_t end_ns) {
    return static_cast<Writer*>(handle)->finish(end_ns);
}

extern "C" void sb_browser_mf_close(void* handle) { delete static_cast<Writer*>(handle); }

extern "C" std::int32_t sb_browser_mf_verify(const wchar_t* path, UINT32 width, UINT32 height,
                                              UINT64 expected, UINT64* frames, UINT64* last_pts,
                                              UINT32* phase) {
    *phase = 1;
    Platform platform;
    if (FAILED(platform.status)) return platform.status;
    ComPtr<IMFSourceReader> reader;
    *phase = 2;
    HRESULT hr = platform.api.source(path, nullptr, reader.GetAddressOf());
    if (FAILED(hr)) return hr;
    ComPtr<IMFMediaType> native;
    *phase = 3;
    hr = reader->GetNativeMediaType(0, 0, native.GetAddressOf());
    if (FAILED(hr)) return hr;
    GUID major{}, subtype{};
    *phase = 4;
    UINT64 size = 0;
    *phase = 41;
    if (FAILED(native->GetGUID(MF_MT_MAJOR_TYPE, &major)) || major != MFMediaType_Video)
        return MF_E_INVALIDMEDIATYPE;
    *phase = 42;
    if (FAILED(native->GetGUID(MF_MT_SUBTYPE, &subtype)) || subtype != MFVideoFormat_H264)
        return MF_E_INVALIDMEDIATYPE;
    *phase = 43;
    if (FAILED(native->GetUINT64(MF_MT_FRAME_SIZE, &size)) || size != pair(width, height))
        return MF_E_INVALIDMEDIATYPE;
    ComPtr<IMFMediaType> second;
    *phase = 5;
    if (reader->GetNativeMediaType(1, 0, second.GetAddressOf()) != MF_E_INVALIDSTREAMNUMBER)
        return MF_E_INVALIDMEDIATYPE;
    ComPtr<IMFMediaType> raw;
    *phase = 6;
    hr = platform.api.media_type(raw.GetAddressOf());
    if (SUCCEEDED(hr)) hr = raw->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    if (SUCCEEDED(hr)) hr = raw->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_NV12);
    if (SUCCEEDED(hr)) hr = reader->SetCurrentMediaType(0, nullptr, raw.Get());
    if (FAILED(hr)) return hr;
    *phase = 7;
    return decode(reader.Get(), width, height, expected, frames, last_pts, phase);
}
