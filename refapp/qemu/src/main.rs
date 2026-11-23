//! Bare-metal Cortex-M4F flight node for QEMU `mps2-an386` (APP-004, APP-102).
//!
//! Runs `refapp_flight::FlightNode` directly on bare metal with no heap and no OS, speaking the
//! §8.2 node protocol (`docs/reference-application.md` §8.2, APP-092) over CMSDK UART0
//! (`-serial stdio`), and terminating cleanly via Arm semihosting `SYS_EXIT`.
//!
//! Linking this binary in release mode with LTO proves panic-freedom of `refapp-flight` and all
//! reachable Aldis code (APP-102, VER-010): the panic handler calls the undefined symbol
//! `aldis_panic_is_unreachable`, so any reachable panic path fails the link.
#![no_std]
#![no_main]

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::panic::PanicInfo;

use refapp_flight::FlightNode;
use refapp_scenario::generated::SCENARIO_VERSION;
use refapp_scenario::{Apid, Delivered, Instant, LinkUnit, Node, Output, StepInput, Variant};

const PROTOCOL_VERSION: u16 = 1;
const ROLE_FLIGHT: u8 = 0;

const T_HELLO: u8 = 0x01;
const T_STEP: u8 = 0x02;
const T_END: u8 = 0x03;
const T_HELLO_ACK: u8 = 0x81;
const T_STEP_RESULT: u8 = 0x82;
const T_END_ACK: u8 = 0x83;

const MAX_RX_BODY: usize = 8192;
const MAX_TX_FRAME: usize = 16384;
const MAX_DELIVERED_UNITS: usize = 32;
const MAX_OPS: usize = 32;

/// Static workspace in `.bss` so `FlightNode` and the wire buffers never sit on the stack.
struct Buffers {
    rx: [u8; MAX_RX_BODY],
    tx: [u8; MAX_TX_FRAME],
    node: MaybeUninit<FlightNode>,
}

struct SingleCore<T>(UnsafeCell<T>);

#[expect(
    unsafe_code,
    reason = "single-core bare-metal binary with interrupts masked; accessed once from entry"
)]
// SAFETY: Cortex-M4F reset entry runs single-threaded with no interrupts enabled.
unsafe impl<T> Sync for SingleCore<T> {}

static BUFFERS: SingleCore<Buffers> = SingleCore(UnsafeCell::new(Buffers {
    rx: [0; MAX_RX_BODY],
    tx: [0; MAX_TX_FRAME],
    node: MaybeUninit::uninit(),
}));

/// Panic-freedom link check (APP-102, VER-010): if any panic path survives release LTO, the
/// linker reports an undefined reference to `aldis_panic_is_unreachable`.
#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    #[expect(
        unsafe_code,
        reason = "undefined extern symbol for the VER-010 / APP-102 panic-freedom link check"
    )]
    unsafe extern "C" {
        fn aldis_panic_is_unreachable() -> !;
    }
    #[expect(unsafe_code, reason = "diverging call to the link-check sentinel")]
    // SAFETY: never reached in a panic-free image; fails to link if reachable.
    unsafe {
        aldis_panic_is_unreachable()
    }
}

/// Fault handler: exits QEMU with status 1 via semihosting.
#[expect(unsafe_code, reason = "exception vector entry point")]
#[unsafe(no_mangle)]
pub extern "C" fn refapp_qemu_fault() -> ! {
    semihosting::exit(false)
}

/// Reset entry point referenced by `link-cortex-m.x` and `.vector_table.exceptions`.
#[expect(unsafe_code, reason = "reset entry point named in linker script")]
#[unsafe(no_mangle)]
pub extern "C" fn refapp_qemu_main() -> ! {
    #[cfg(feature = "inject-panic")]
    #[expect(
        clippy::panic,
        reason = "canary feature proving the APP-102 panic-freedom link check catches a panic"
    )]
    {
        panic!("inject-panic canary");
    }

    uart::init();

    #[expect(
        unsafe_code,
        reason = "sole mutable borrow of static `.bss` buffers from reset entry"
    )]
    // SAFETY: `refapp_qemu_main` executes once from reset with no interrupts enabled.
    let buffers = unsafe { &mut *BUFFERS.0.get() };

    let ok = run_session(buffers).is_ok();
    semihosting::exit(ok)
}

/// Runs one §8.2 node-protocol session: `HELLO`, zero or more `STEP`s, then `END`.
fn run_session(buffers: &mut Buffers) -> Result<(), ()> {
    let hello_len = read_frame(&mut buffers.rx)?;
    let hello_body = buffers.rx.get(..hello_len).ok_or(())?;
    let node = handle_hello(hello_body)?;
    let node = buffers.node.write(node);

    // Send HELLO_ACK: 4-byte length (3) + T_HELLO_ACK + version (u16 BE).
    let ver = PROTOCOL_VERSION.to_be_bytes();
    let ack = [0, 0, 0, 3, T_HELLO_ACK, ver[0], ver[1]];
    uart::write_all(&ack);

    // bound: terminates on `T_END` or protocol error from the host harness.
    loop {
        let len = read_frame(&mut buffers.rx)?;
        let body = buffers.rx.get(..len).ok_or(())?;
        let mut c = Cursor::new(body);
        match c.u8()? {
            T_STEP => {
                handle_step(&mut c, node, &mut buffers.tx)?;
            }
            T_END => {
                if !c.is_empty() {
                    return Err(());
                }
                let end_ack = [0, 0, 0, 1, T_END_ACK];
                uart::write_all(&end_ack);
                return Ok(());
            }
            _ => return Err(()),
        }
    }
}

/// Decodes `HELLO` and constructs the `FlightNode` for `case_id`.
fn handle_hello(body: &[u8]) -> Result<FlightNode, ()> {
    let mut c = Cursor::new(body);
    if c.u8()? != T_HELLO {
        return Err(());
    }
    let version = c.u16()?;
    let scenario_version = c.u16()?;
    let role = c.u8()?;
    let id_len = usize::from(c.u8()?);
    let case_id = c.take(id_len)?;
    if !c.is_empty()
        || version != PROTOCOL_VERSION
        || scenario_version != SCENARIO_VERSION
        || role != ROLE_FLIGHT
    {
        return Err(());
    }
    make_flight_node(case_id)
}

/// Maps a scenario case ID to its `FlightNode` configuration (`refapp/scenario/scenario.json`).
fn make_flight_node(case_id: &[u8]) -> Result<FlightNode, ()> {
    match case_id {
        b"a0-empty" | b"a1-nominal" | b"a1-immediate" | b"a1-rejects" | b"a1-downlink-faults" => {
            Ok(FlightNode::new())
        }
        b"a2-nominal" | b"a2-downlink-loss" | b"a2-downlink-corrupt" | b"a2-uplink-faults" => {
            FlightNode::new_frames(Variant::TmTc).map_err(|_| ())
        }
        b"a3-nominal"
        | b"a3-uplink-loss"
        | b"a3-uplink-disorder"
        | b"a3-uplink-lockout"
        | b"a3-uplink-limit" => FlightNode::new_cop1(Variant::TmTc).map_err(|_| ()),
        b"a4-aos-nominal" => FlightNode::new_cop1(Variant::Aos).map_err(|_| ()),
        b"a4-uslp-nominal" => FlightNode::new_cop1(Variant::Uslp).map_err(|_| ()),
        _ => Err(()),
    }
}

/// Decodes `STEP`, runs `FlightNode::step`, and writes `STEP_RESULT`.
fn handle_step(c: &mut Cursor<'_>, node: &mut FlightNode, tx: &mut [u8]) -> Result<(), ()> {
    let now = c.u64()?;
    let tx_slot = match c.u8()? {
        0 => false,
        1 => true,
        _ => return Err(()),
    };

    let unit_count = usize::from(c.u16()?);
    if unit_count > MAX_DELIVERED_UNITS {
        return Err(());
    }
    let mut units: [LinkUnit<'_>; MAX_DELIVERED_UNITS] = [&[]; MAX_DELIVERED_UNITS];
    // bound: `unit_count <= MAX_DELIVERED_UNITS` (32).
    for slot in units.get_mut(..unit_count).ok_or(())? {
        let n = usize::from(c.u16()?);
        *slot = c.take(n)?;
    }

    let ops_count = usize::from(c.u16()?);
    if ops_count > MAX_OPS {
        return Err(());
    }
    let mut ops = [0_u16; MAX_OPS];
    // bound: `ops_count <= MAX_OPS` (32).
    for slot in ops.get_mut(..ops_count).ok_or(())? {
        *slot = c.u16()?;
    }

    if !c.is_empty() {
        return Err(());
    }

    let delivered_slice = units.get(..unit_count).ok_or(())?;
    let ops_slice = ops.get(..ops_count).ok_or(())?;
    let input = StepInput {
        now: Instant::from_micros(now),
        delivered: Delivered::new(delivered_slice),
        ops: ops_slice,
        tx_slot,
    };

    let mut out = WireOutput::new(tx)?;
    let next_deadline = node.step(&input, &mut out).map(Instant::as_micros);
    let total = out.finish(next_deadline)?;
    let frame = tx.get(..total).ok_or(())?;
    uart::write_all(frame);
    Ok(())
}

/// Reads one length-prefixed §8.2 message body into `buf` and returns its length.
fn read_frame(buf: &mut [u8]) -> Result<usize, ()> {
    let mut len_bytes = [0_u8; 4];
    uart::read_exact(&mut len_bytes);
    let len = usize::try_from(u32::from_be_bytes(len_bytes)).map_err(|_| ())?;
    if len == 0 || len > buf.len() {
        return Err(());
    }
    let dst = buf.get_mut(..len).ok_or(())?;
    uart::read_exact(dst);
    Ok(len)
}

/// Strict big-endian slice cursor (no clamping or defaults).
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    const fn is_empty(&self) -> bool {
        self.pos == self.bytes.len()
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ()> {
        let end = self.pos.checked_add(n).ok_or(())?;
        let slice = self.bytes.get(self.pos..end).ok_or(())?;
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, ()> {
        let b = self.take(1)?;
        b.first().copied().ok_or(())
    }

    fn u16(&mut self) -> Result<u16, ()> {
        let b = self.take(2)?;
        let arr: [u8; 2] = b.try_into().map_err(|_| ())?;
        Ok(u16::from_be_bytes(arr))
    }

    fn u64(&mut self) -> Result<u64, ()> {
        let b = self.take(8)?;
        let arr: [u8; 8] = b.try_into().map_err(|_| ())?;
        Ok(u64::from_be_bytes(arr))
    }
}

/// Encodes `Output` calls directly into a `STEP_RESULT` wire frame in a fixed buffer.
struct WireOutput<'a> {
    buf: &'a mut [u8],
    pos: usize,
    count: u16,
    overflow: bool,
}

impl<'a> WireOutput<'a> {
    /// Reserves the 4-byte length prefix, `T_STEP_RESULT`, and 2-byte output count.
    fn new(buf: &'a mut [u8]) -> Result<Self, ()> {
        let header = buf.get_mut(..7).ok_or(())?;
        header.fill(0);
        if let Some(tag) = header.get_mut(4) {
            *tag = T_STEP_RESULT;
        }
        Ok(Self {
            buf,
            pos: 7,
            count: 0,
            overflow: false,
        })
    }

    fn push_bytes(&mut self, data: &[u8]) {
        if self.overflow {
            return;
        }
        let Some(end) = self.pos.checked_add(data.len()) else {
            self.overflow = true;
            return;
        };
        let Some(dst) = self.buf.get_mut(self.pos..end) else {
            self.overflow = true;
            return;
        };
        dst.copy_from_slice(data);
        self.pos = end;
    }

    fn push_u8(&mut self, v: u8) {
        self.push_bytes(&[v]);
    }

    fn push_octets(&mut self, octets: &[u8]) {
        let Ok(len) = u16::try_from(octets.len()) else {
            self.overflow = true;
            return;
        };
        self.push_bytes(&len.to_be_bytes());
        self.push_bytes(octets);
    }

    const fn bump_count(&mut self) {
        if let Some(next) = self.count.checked_add(1) {
            self.count = next;
        } else {
            self.overflow = true;
        }
    }

    /// Appends `next_deadline`, patches the body length and output count, and returns total bytes.
    fn finish(mut self, next_deadline: Option<u64>) -> Result<usize, ()> {
        self.push_bytes(&next_deadline.unwrap_or(u64::MAX).to_be_bytes());
        if self.overflow {
            return Err(());
        }
        let body_len = self.pos.checked_sub(4).ok_or(())?;
        let body_len_u32 = u32::try_from(body_len).map_err(|_| ())?;
        if let Some(dst) = self.buf.get_mut(0..4) {
            dst.copy_from_slice(&body_len_u32.to_be_bytes());
        } else {
            return Err(());
        }
        if let Some(dst) = self.buf.get_mut(5..7) {
            dst.copy_from_slice(&self.count.to_be_bytes());
        } else {
            return Err(());
        }
        Ok(self.pos)
    }
}

impl Output for WireOutput<'_> {
    fn unit(&mut self, octets: &[u8]) {
        self.bump_count();
        self.push_u8(1);
        self.push_octets(octets);
    }

    fn packet_out(&mut self, apid: Apid, octets: &[u8]) {
        self.bump_count();
        self.push_u8(2);
        self.push_bytes(&apid.get().to_be_bytes());
        self.push_octets(octets);
    }

    fn packet_in(&mut self, apid: Apid, octets: &[u8]) {
        self.bump_count();
        self.push_u8(3);
        self.push_bytes(&apid.get().to_be_bytes());
        self.push_octets(octets);
    }

    fn app_event(&mut self, code: u16, args: &[u64]) {
        self.bump_count();
        self.push_u8(4);
        self.push_bytes(&code.to_be_bytes());
        let Ok(n) = u8::try_from(args.len()) else {
            self.overflow = true;
            return;
        };
        self.push_u8(n);
        // bound: `args.len() <= 255`.
        for a in args {
            self.push_bytes(&a.to_be_bytes());
        }
    }
}

/// Cortex-M4 exception vector table for QEMU `mps2-an386`.
mod vector_table {
    use crate::{refapp_qemu_fault, refapp_qemu_main};

    #[expect(
        unsafe_code,
        reason = "the vector table must be placed in its linker section"
    )]
    // Why this is sound: placed at the vector table address by `link-cortex-m.x`.
    #[unsafe(link_section = ".vector_table.exceptions")]
    #[used]
    static EXCEPTIONS: [extern "C" fn() -> !; 15] = [
        refapp_qemu_main,
        refapp_qemu_fault,
        refapp_qemu_fault,
        refapp_qemu_fault,
        refapp_qemu_fault,
        refapp_qemu_fault,
        refapp_qemu_fault,
        refapp_qemu_fault,
        refapp_qemu_fault,
        refapp_qemu_fault,
        refapp_qemu_fault,
        refapp_qemu_fault,
        refapp_qemu_fault,
        refapp_qemu_fault,
        refapp_qemu_fault,
    ];
}

/// CMSDK APB UART0 on QEMU `mps2-an386` (`0x4000_4000`, connected to `-serial stdio`).
mod uart {
    const BASE: usize = 0x4000_4000;
    const DATA_ADDR: usize = BASE;
    const STATE_ADDR: usize = BASE + 0x04;
    const CTRL_ADDR: usize = BASE + 0x08;

    const STATE_TX_FULL: u32 = 1 << 0;
    const STATE_RX_FULL: u32 = 1 << 1;
    const CTRL_TX_EN: u32 = 1 << 0;
    const CTRL_RX_EN: u32 = 1 << 1;

    /// Enables TX and RX on UART0.
    pub fn init() {
        let ctrl: *mut u32 = core::ptr::with_exposed_provenance_mut(CTRL_ADDR);
        #[expect(unsafe_code, reason = "CMSDK UART0 MMIO register write")]
        // SAFETY: `0x4000_4008` is the CMSDK UART0 CTRL register on `mps2-an386`.
        unsafe {
            ctrl.write_volatile(CTRL_TX_EN | CTRL_RX_EN);
        }
    }

    fn read_byte() -> u8 {
        let state_reg: *const u32 = core::ptr::with_exposed_provenance(STATE_ADDR);
        let data_reg: *const u32 = core::ptr::with_exposed_provenance(DATA_ADDR);
        // bound: host harness sends the next byte of the lockstep protocol message.
        loop {
            #[expect(unsafe_code, reason = "CMSDK UART0 MMIO register read")]
            // SAFETY: `0x4000_4004` is the CMSDK UART0 STATE register on `mps2-an386`.
            let state = unsafe { state_reg.read_volatile() };
            if (state & STATE_RX_FULL) != 0 {
                #[expect(unsafe_code, reason = "CMSDK UART0 MMIO register read")]
                // SAFETY: `0x4000_4000` is the CMSDK UART0 DATA register on `mps2-an386`.
                let word = unsafe { data_reg.read_volatile() };
                return u8::try_from(word & 0xFF).unwrap_or_default();
            }
            core::hint::spin_loop();
        }
    }

    fn write_byte(b: u8) {
        let state_reg: *const u32 = core::ptr::with_exposed_provenance(STATE_ADDR);
        let data_reg: *mut u32 = core::ptr::with_exposed_provenance_mut(DATA_ADDR);
        // bound: QEMU drains the UART TX FIFO synchronously on each write.
        loop {
            #[expect(unsafe_code, reason = "CMSDK UART0 MMIO register read")]
            // SAFETY: `0x4000_4004` is the CMSDK UART0 STATE register on `mps2-an386`.
            let state = unsafe { state_reg.read_volatile() };
            if (state & STATE_TX_FULL) == 0 {
                #[expect(unsafe_code, reason = "CMSDK UART0 MMIO register write")]
                // SAFETY: `0x4000_4000` is the CMSDK UART0 DATA register on `mps2-an386`.
                unsafe {
                    data_reg.write_volatile(u32::from(b));
                }
                return;
            }
            core::hint::spin_loop();
        }
    }

    pub fn read_exact(buf: &mut [u8]) {
        // bound: bounded by `buf.len()`.
        for slot in buf {
            *slot = read_byte();
        }
    }

    pub fn write_all(buf: &[u8]) {
        // bound: bounded by `buf.len()`.
        for &b in buf {
            write_byte(b);
        }
    }
}

/// Arm semihosting `SYS_EXIT` (`bkpt 0xAB`) for clean QEMU termination.
mod semihosting {
    const SYS_EXIT: usize = 0x18;
    const APPLICATION_EXIT: usize = 0x2_0026;
    const RUNTIME_ERROR: usize = 0x2_0023;

    pub fn exit(success: bool) -> ! {
        let reason = if success {
            APPLICATION_EXIT
        } else {
            RUNTIME_ERROR
        };
        #[expect(unsafe_code, reason = "Arm semihosting SYS_EXIT trap")]
        // SAFETY: `SYS_EXIT` takes only the integer reason code in r1 and terminates QEMU.
        unsafe {
            core::arch::asm!(
                "bkpt 0xAB",
                in("r0") SYS_EXIT,
                in("r1") reason,
                options(nostack, preserves_flags),
            );
        }
        // bound: only reached without a semihosting host.
        loop {
            core::hint::spin_loop();
        }
    }
}
