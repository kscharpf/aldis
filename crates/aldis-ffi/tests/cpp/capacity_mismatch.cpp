// Must NOT compile: a handle type whose capacity differs from the library's fails to build with
// a static_assert (CABI-072, CABI-030, OI-20). The `header` check compiles this file and expects
// the failure to name the constant; with `-DALDIS_HPP_CAPACITY_OK` it compiles, which shows the
// failure is the capacity and nothing else.

#include "aldis/aldis.hpp"

#ifdef ALDIS_HPP_CAPACITY_OK
constexpr std::size_t kQueue = ALDIS_FOP1_SENT_QUEUE_FRAMES;
#else
constexpr std::size_t kQueue = ALDIS_FOP1_SENT_QUEUE_FRAMES + 1;
#endif

int main() {
    aldis::Fop1<kQueue, ALDIS_FOP1_MAX_FRAME_OCTETS> engine;
    aldis_fop1_status_t snapshot{};
    return aldis::ok(engine.query(snapshot)) ? 0 : 1;
}
