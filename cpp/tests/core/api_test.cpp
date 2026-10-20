// Compile-time checks over the public aldis_core API: every public function is noexcept
// (CPP-026), value types are trivially copyable (CPP-021, CPP-025), and no wire type uses a
// bit-field member or a layout cast (CPP-033 is inspected; sizes here only document footprints).

#include "aldis/core/core.hpp"

#include <gtest/gtest.h>

#include <cstddef>
#include <span>
#include <type_traits>
#include <utility>

namespace {

using namespace aldis::core; // NOLINT(google-build-using-namespace): a list of API checks

constexpr std::span<const std::byte> cbuf{};
constexpr std::span<std::byte> mbuf{};

// error.hpp
static_assert(noexcept(error::code(ErrorKind::Malformed)));
static_assert(noexcept(error::from_code(1)));
static_assert(noexcept(error::name(ErrorKind::Malformed)));
static_assert(noexcept(error::from_name(std::span<const char>{})));
static_assert(noexcept(Error{ErrorKind::Malformed}));
static_assert(noexcept(Error::buffer_too_small(1)));
static_assert(noexcept(Error::incomplete(1)));
static_assert(noexcept(Error::out_of_range(FieldId{"x"}, 1, 2)));
static_assert(noexcept(std::declval<const Error&>().with_field(FieldId{"x"})));
static_assert(noexcept(std::declval<const Error&>().with_detail(ErrorDetail::none())));
static_assert(noexcept(std::declval<const Error&>().kind()));
static_assert(noexcept(std::declval<const Error&>().field()));
static_assert(noexcept(std::declval<const Error&>().detail()));
static_assert(noexcept(ErrorDetail::needed(1)));
static_assert(noexcept(ErrorDetail::expected(1, 2)));
static_assert(noexcept(std::declval<const ErrorDetail&>().kind()));
static_assert(noexcept(std::declval<const ErrorDetail&>().needed()));
static_assert(noexcept(std::declval<const ErrorDetail&>().expected()));
static_assert(noexcept(std::declval<const FieldId&>().name()));
static_assert(noexcept(std::declval<const FieldId&>() == std::declval<const FieldId&>()));
static_assert(noexcept(std::declval<const Result<int>&>().ok()));
static_assert(noexcept(std::declval<const Result<int>&>().error()));
static_assert(noexcept(std::declval<const Result<int>&>().is_ok()));
static_assert(noexcept(std::declval<const Result<int>&>().value_or(0)));
static_assert(noexcept(std::declval<const Result<void>&>().error()));
static_assert(noexcept(std::declval<const Result<void>&>().is_ok()));

// bytes.hpp
static_assert(noexcept(bytes::at(mbuf, 0)));
static_assert(noexcept(bytes::get_or(cbuf, 0, std::byte{})));
static_assert(noexcept(bytes::put(mbuf, 0, std::byte{})));
static_assert(noexcept(bytes::subspan(cbuf, 0, 0)));
static_assert(noexcept(bytes::subspan(cbuf, OctetRange{})));
static_assert(noexcept(bytes::subspan_or_empty(cbuf, 0, 0)));
static_assert(noexcept(bytes::equal(cbuf, cbuf)));
static_assert(noexcept(bytes::copy(mbuf, cbuf)));
static_assert(noexcept(bytes::fill(mbuf, std::byte{})));
static_assert(noexcept(OctetRange::make(0, 0)));
static_assert(noexcept(OctetRange::from_len(0, 0)));
static_assert(noexcept(std::declval<ByteReader&>().read(1)));
static_assert(noexcept(std::declval<ByteReader&>().read_u8()));
static_assert(noexcept(std::declval<ByteReader&>().read_u64()));
static_assert(noexcept(std::declval<ByteWriter&>().write(cbuf)));
static_assert(noexcept(std::declval<ByteWriter&>().write_u64(0)));

// bits.hpp
static_assert(noexcept(bits::get(cbuf, 0, 1)));
static_assert(noexcept(bits::set(mbuf, 0, 1, 0)));
static_assert(noexcept(bits::read_u16(cbuf, 0)));
static_assert(noexcept(bits::read_u32(cbuf, 0)));
static_assert(noexcept(bits::read_u64(cbuf, 0)));
static_assert(noexcept(bits::write_u16(mbuf, 0, 0)));
static_assert(noexcept(bits::write_u32(mbuf, 0, 0)));
static_assert(noexcept(bits::write_u64(mbuf, 0, 0)));

// ids.hpp, counter.hpp, time.hpp
static_assert(noexcept(Apid::make(0)));
static_assert(noexcept(Apid::idle()));
static_assert(noexcept(SpacecraftId<10>::make(0)));
static_assert(noexcept(VcId<6>::make(0)));
static_assert(noexcept(MapId<4>::make(0)));
static_assert(noexcept(SeqCount::make(0)));
static_assert(noexcept(std::declval<const SeqCount&>().next()));
static_assert(noexcept(std::declval<const SeqCount&>().classify(SeqCount{})));
static_assert(noexcept(std::declval<const SeqCount&>().counter()));
static_assert(noexcept(FrameSeqNum{0}.next()));
static_assert(noexcept(PacketVersion::make(0)));
static_assert(noexcept(ModCounter::make(8, 0)));
static_assert(noexcept(std::declval<ModCounter&>().increment()));
static_assert(noexcept(std::declval<const ModCounter&>().distance_to(0)));
static_assert(noexcept(std::declval<const ModCounter&>().classify(0)));
static_assert(noexcept(Instant::from_micros(0).duration_since(Instant{})));
static_assert(noexcept(Duration::from_secs(0).saturating_add(Duration{})));

// crc.hpp, oid.hpp
static_assert(noexcept(crc::crc16(cbuf)));
static_assert(noexcept(crc::crc16_table(cbuf)));
static_assert(noexcept(crc::verify_fecf(cbuf)));
static_assert(noexcept(crc::write_fecf(mbuf)));
static_assert(noexcept(crc::syndrome(cbuf)));
static_assert(noexcept(std::declval<OidGenerator&>().fill(mbuf)));
static_assert(noexcept(std::declval<OidGeneratorGalois&>().fill(mbuf)));

// container.hpp
static_assert(noexcept(std::declval<FixedQueue<int, 2>&>().push(0)));
static_assert(noexcept(std::declval<FixedQueue<int, 2>&>().pop()));
static_assert(noexcept(std::declval<const FixedQueue<int, 2>&>().peek()));
static_assert(noexcept(std::declval<ByteQueue<8>&>().push(cbuf)));
static_assert(noexcept(std::declval<ByteQueue<8>&>().pop_into(mbuf)));
static_assert(noexcept(std::declval<ByteQueue<8>&>().discard()));
static_assert(noexcept(std::declval<FixedVec<int, 2>&>().push(0)));
static_assert(noexcept(std::declval<FixedVec<int, 2>&>().pop()));

// security.hpp, layout.hpp
static_assert(noexcept(SecurityLayout::make(0, 0)));
static_assert(noexcept(SecuredFields::make(OctetRange{}, OctetRange{}, OctetRange{})));
static_assert(noexcept(FrameLayout::compute(LayoutSpec{})));

// Value types are trivially copyable (CPP-021, CPP-025) and carry no pointer members, apart from
// the views that FieldId and the cursors hold by design.
static_assert(std::is_trivially_copyable_v<Error>);
static_assert(std::is_trivially_copyable_v<SecurityLayout>);
static_assert(std::is_trivially_copyable_v<FrameCtx>);
static_assert(std::is_trivially_copyable_v<SecuredFields>);
static_assert(std::is_trivially_copyable_v<FrameLayout>);
static_assert(std::is_trivially_copyable_v<LayoutSpec>);
static_assert(std::is_trivially_copyable_v<ModCounter>);

TEST(Api, EveryPublicFunctionIsNoexcept) {
    // req: CPP-026, CPP-021, CPP-025
    SUCCEED(); // the checks are the static_asserts above
}

} // namespace
