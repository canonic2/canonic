#pragma once
#include <stdexcept>
#include <string>
#include <thread>
extern "C" void acme_native_crash(bool);
extern "C" void acme_native_hang(bool);
extern "C" int acme_native_retain(bool);

// One UI-thread owner per app. Only its own process is affected by fatal actions.
struct AcmeDesktopFaults {
    bool corrected = false;
    bool armed = false;
    std::string run(int action) {
        if (action >= 20) {
            if (!armed) return "Arm this app first";
            armed = false;
        }
        switch (action) {
            case 10: if (!corrected) throw std::runtime_error("Acme C++ fixture exception"); return "ready";
            case 11: { unsigned int checksum = 0; for (unsigned int n = 0; n < (corrected ? 100u : 100000u); ++n) checksum += n; return std::to_string(checksum); }
            case 12: return std::to_string(acme_native_retain(corrected));
            case 13: acme_native_retain(true); armed = false; return "Acme reset";
            case 20: acme_native_crash(corrected); return "crash trigger returned";
            case 21: acme_native_hang(corrected); return "hang trigger returned";
            case 22: if (!corrected) std::thread([] { acme_native_crash(false); }).detach(); return "background trigger returned";
            default: throw std::invalid_argument("Unknown Acme action");
        }
    }
};
