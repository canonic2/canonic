#include <chrono>
#include <cstdlib>
#include <thread>
#include <vector>

// Explicit calls from an owned disposable app/JNI/FFI bridge, never a loader hook.
extern "C" void acme_native_crash(bool corrected) { if (!corrected) std::abort(); }
extern "C" void acme_native_hang(bool corrected) { if (!corrected) std::this_thread::sleep_for(std::chrono::seconds(6)); }
extern "C" int acme_native_retain(bool corrected) {
    static std::vector<unsigned char> retained;
    if (corrected) std::vector<unsigned char>().swap(retained);
    else retained.assign(1048576, 1);
    return static_cast<int>(retained.size());
}
