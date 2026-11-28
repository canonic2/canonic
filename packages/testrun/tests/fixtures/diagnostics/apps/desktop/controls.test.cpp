#include "faults.hpp"
#include <cassert>
#include <iostream>
int main() {
    AcmeDesktopFaults faults;
    assert(faults.run(20) == "Arm this app first");
    bool observed = false;
    try { faults.run(10); } catch (const std::runtime_error&) { observed = true; }
    assert(observed);
    assert(faults.run(11) == "704982704");
    for (int n=0; n<3; ++n) assert(faults.run(12) == "1048576");
    assert(faults.run(13) == "Acme reset");
    faults.corrected = true;
    assert(faults.run(10) == "ready"); assert(faults.run(11) == "4950"); assert(faults.run(12) == "0");
    for (int action : {20, 21, 22}) { faults.armed = true; faults.run(action); assert(!faults.armed); }
    std::cout << "Acme desktop controls ready\n";
}
