//! 第四章：地址空间 —— 参考答案
//!
//! 本章实现了基于 RISC-V Sv39 的虚拟内存管理，为每个进程提供独立的地址空间。
//! 标有 "TODO X 参考答案" 的位置对应骨架代码中的填空处。
#![no_std]
#![no_main]
#![cfg_attr(target_arch = "riscv64", deny(warnings, missing_docs))]
#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code, unused_imports))]

mod process;

#[macro_use]
extern crate tg_console;

extern crate alloc;

use crate::{
    impls::{Sv39Manager, SyscallContext},
    process::Process,
};
use alloc::{alloc::alloc, vec::Vec};
use core::{alloc::Layout, cell::UnsafeCell};
use impls::Console;
use riscv::register::*;
#[cfg(not(target_arch = "riscv64"))]
use stub::Sv39;
use tg_console::log;
use tg_kernel_context::{foreign::MultislotPortal, LocalContext};
#[cfg(target_arch = "riscv64")]
use tg_kernel_vm::page_table::Sv39;
use tg_kernel_vm::{
    page_table::{MmuMeta, VAddr, VmFlags, VmMeta, PPN, VPN},
    AddressSpace,
};
use tg_sbi;
use tg_syscall::Caller;
use xmas_elf::ElfFile;

/// 构建 VmFlags。
#[cfg(target_arch = "riscv64")]
const fn build_flags(s: &str) -> VmFlags<Sv39> {
    VmFlags::build_from_str(s)
}

/// 解析 VmFlags。
#[cfg(target_arch = "riscv64")]
fn parse_flags(s: &str) -> Result<VmFlags<Sv39>, ()> {
    s.parse()
}

#[cfg(not(target_arch = "riscv64"))]
use stub::{build_flags, parse_flags};

// 应用程序内联进来。
#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(include_str!(env!("APP_ASM")));
// 定义内核入口。
#[cfg(target_arch = "riscv64")]
tg_linker::boot0!(rust_main; stack = 6 * 4096);
// 物理内存容量 = 24 MiB。
const MEMORY: usize = 24 << 20;
// 传送门所在虚页。
const PROTAL_TRANSIT: VPN<Sv39> = VPN::MAX;
// 进程列表。
struct ProcessList(UnsafeCell<Vec<Process>>);

unsafe impl Sync for ProcessList {}

impl ProcessList {
    const fn new() -> Self {
        Self(UnsafeCell::new(Vec::new()))
    }

    unsafe fn get_mut(&self) -> &mut Vec<Process> {
        &mut *self.0.get()
    }
}

static PROCESSES: ProcessList = ProcessList::new();

extern "C" fn rust_main() -> ! {
    let layout = tg_linker::KernelLayout::locate();
    unsafe { layout.zero_bss() };
    tg_console::init_console(&Console);
    tg_console::set_log_level(option_env!("LOG"));
    tg_console::test_log();
    tg_kernel_alloc::init(layout.start() as _);
    unsafe {
        tg_kernel_alloc::transfer(core::slice::from_raw_parts_mut(
            layout.end() as _,
            MEMORY - layout.len(),
        ))
    };
    let portal_size = MultislotPortal::calculate_size(1);
    let portal_layout = Layout::from_size_align(portal_size, 1 << Sv39::PAGE_BITS).unwrap();
    let portal_ptr = unsafe { alloc(portal_layout) };
    assert!(portal_layout.size() < 1 << Sv39::PAGE_BITS);
    let mut ks = kernel_space(layout, MEMORY, portal_ptr as _);
    let portal_idx = PROTAL_TRANSIT.index_in(Sv39::MAX_LEVEL);
    for (i, elf) in tg_linker::AppMeta::locate().iter().enumerate() {
        let base = elf.as_ptr() as usize;
        log::info!("detect app[{i}]: {base:#x}..{:#x}", base + elf.len());
        if let Some(process) = Process::new(ElfFile::new(elf).unwrap()) {
            process.address_space.root()[portal_idx] = ks.root()[portal_idx];
            unsafe { PROCESSES.get_mut().push(process) };
        }
    }

    const PAGE: Layout =
        unsafe { Layout::from_size_align_unchecked(2 << Sv39::PAGE_BITS, 1 << Sv39::PAGE_BITS) };
    let pages = 2;
    let stack = unsafe { alloc(PAGE) };
    ks.map_extern(
        VPN::new((1 << 26) - pages)..VPN::new(1 << 26),
        PPN::new(stack as usize >> Sv39::PAGE_BITS),
        build_flags("_WRV"),
    );
    let mut scheduling = LocalContext::thread(schedule as *const () as _, false);
    *scheduling.sp_mut() = 1 << 38;
    unsafe { scheduling.execute() };
    log::error!("stval = {:#x}", stval::read());
    panic!("trap from scheduling thread: {:?}", scause::read().cause());
}

extern "C" fn schedule() -> ! {
    let portal = unsafe { MultislotPortal::init_transit(PROTAL_TRANSIT.base().val(), 1) };
    tg_syscall::init_io(&SyscallContext);
    tg_syscall::init_process(&SyscallContext);
    tg_syscall::init_scheduling(&SyscallContext);
    tg_syscall::init_clock(&SyscallContext);
    tg_syscall::init_trace(&SyscallContext);
    tg_syscall::init_memory(&SyscallContext);
    while !unsafe { PROCESSES.get_mut().is_empty() } {
        let ctx = unsafe { &mut PROCESSES.get_mut()[0].context };
        unsafe { ctx.execute(portal, ()) };
        match scause::read().cause() {
            scause::Trap::Exception(scause::Exception::UserEnvCall) => {
                use tg_syscall::{SyscallId as Id, SyscallResult as Ret};

                let ctx = &mut ctx.context;
                let id: Id = ctx.a(7).into();
                let args = [ctx.a(0), ctx.a(1), ctx.a(2), ctx.a(3), ctx.a(4), ctx.a(5)];

                // ======== TODO 1 参考答案：记录系统调用次数 ========
                let id_num: usize = ctx.a(7);
                if id_num < 512 {
                    unsafe { PROCESSES.get_mut()[0].syscall_counts[id_num] += 1 };
                }
                // ======== END TODO 1 ========

                match tg_syscall::handle(Caller { entity: 0, flow: 0 }, id, args) {
                    Ret::Done(ret) => match id {
                        Id::EXIT => unsafe {
                            PROCESSES.get_mut().remove(0);
                        },
                        _ => {
                            *ctx.a_mut(0) = ret as _;
                            ctx.move_next();
                        }
                    },
                    Ret::Unsupported(_) => {
                        log::info!("id = {id:?}");
                        unsafe { PROCESSES.get_mut().remove(0) };
                    }
                }
            }
            e => {
                log::error!(
                    "unsupported trap: {e:?}, stval = {:#x}, sepc = {:#x}",
                    stval::read(),
                    ctx.context.pc()
                );
                unsafe { PROCESSES.get_mut().remove(0) };
            }
        }
    }
    tg_sbi::shutdown(false)
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log::error!("{info}");
    tg_sbi::shutdown(true)
}

fn kernel_space(
    layout: tg_linker::KernelLayout,
    memory: usize,
    portal: usize,
) -> AddressSpace<Sv39, Sv39Manager> {
    let mut space = AddressSpace::<Sv39, Sv39Manager>::new();
    for region in layout.iter() {
        log::info!("{region}");
        use tg_linker::KernelRegionTitle::*;
        let flags = match region.title {
            Text => "X_RV",
            Rodata => "__RV",
            Data | Boot => "_WRV",
        };
        let s = VAddr::<Sv39>::new(region.range.start);
        let e = VAddr::<Sv39>::new(region.range.end);
        space.map_extern(
            s.floor()..e.ceil(),
            PPN::new(s.floor().val()),
            build_flags(flags),
        )
    }
    log::info!(
        "(heap) ---> {:#10x}..{:#10x}",
        layout.end(),
        layout.start() + memory
    );
    let s = VAddr::<Sv39>::new(layout.end());
    let e = VAddr::<Sv39>::new(layout.start() + memory);
    space.map_extern(
        s.floor()..e.ceil(),
        PPN::new(s.floor().val()),
        build_flags("_WRV"),
    );
    space.map_extern(
        PROTAL_TRANSIT..PROTAL_TRANSIT + 1,
        PPN::new(portal >> Sv39::PAGE_BITS),
        build_flags("__G_XWRV"),
    );
    println!();
    unsafe { satp::set(satp::Mode::Sv39, 0, space.root_ppn().val()) };
    space
}

/// 各种接口库的实现。
mod impls {
    use crate::{build_flags, parse_flags, Sv39, PROCESSES};
    use alloc::alloc::alloc_zeroed;
    use core::{alloc::Layout, ptr::NonNull};
    use tg_console::log;
    use tg_kernel_vm::{
        page_table::{MmuMeta, Pte, VAddr, VmFlags, PPN, VPN},
        PageManager,
    };
    use tg_syscall::*;

    #[repr(transparent)]
    pub struct Sv39Manager(NonNull<Pte<Sv39>>);

    impl Sv39Manager {
        const OWNED: VmFlags<Sv39> = unsafe { VmFlags::from_raw(1 << 8) };

        #[inline]
        fn page_alloc<T>(count: usize) -> *mut T {
            unsafe {
                alloc_zeroed(Layout::from_size_align_unchecked(
                    count << Sv39::PAGE_BITS,
                    1 << Sv39::PAGE_BITS,
                ))
            }
            .cast()
        }
    }

    impl PageManager<Sv39> for Sv39Manager {
        #[inline]
        fn new_root() -> Self {
            Self(NonNull::new(Self::page_alloc(1)).unwrap())
        }

        #[inline]
        fn root_ppn(&self) -> PPN<Sv39> {
            PPN::new(self.0.as_ptr() as usize >> Sv39::PAGE_BITS)
        }

        #[inline]
        fn root_ptr(&self) -> NonNull<Pte<Sv39>> {
            self.0
        }

        #[inline]
        fn p_to_v<T>(&self, ppn: PPN<Sv39>) -> NonNull<T> {
            unsafe { NonNull::new_unchecked(VPN::<Sv39>::new(ppn.val()).base().as_mut_ptr()) }
        }

        #[inline]
        fn v_to_p<T>(&self, ptr: NonNull<T>) -> PPN<Sv39> {
            PPN::new(VAddr::<Sv39>::new(ptr.as_ptr() as _).floor().val())
        }

        #[inline]
        fn check_owned(&self, pte: Pte<Sv39>) -> bool {
            pte.flags().contains(Self::OWNED)
        }

        #[inline]
        fn allocate(&mut self, len: usize, flags: &mut VmFlags<Sv39>) -> NonNull<u8> {
            *flags |= Self::OWNED;
            NonNull::new(Self::page_alloc(len)).unwrap()
        }

        fn deallocate(&mut self, _pte: Pte<Sv39>, _len: usize) -> usize {
            todo!()
        }

        fn drop_root(&mut self) {
            todo!()
        }
    }

    pub struct Console;

    impl tg_console::Console for Console {
        #[inline]
        fn put_char(&self, c: u8) {
            tg_sbi::console_putchar(c);
        }
    }

    pub struct SyscallContext;

    impl IO for SyscallContext {
        fn write(&self, caller: Caller, fd: usize, buf: usize, count: usize) -> isize {
            match fd {
                STDOUT | STDDEBUG => {
                    const READABLE: VmFlags<Sv39> = build_flags("RV");
                    if let Some(ptr) = unsafe { PROCESSES.get_mut() }
                        .get_mut(caller.entity)
                        .unwrap()
                        .address_space
                        .translate::<u8>(VAddr::new(buf), READABLE)
                    {
                        print!("{}", unsafe {
                            core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                                ptr.as_ptr(),
                                count,
                            ))
                        });
                        count as _
                    } else {
                        log::error!("ptr not readable");
                        -1
                    }
                }
                _ => {
                    log::error!("unsupported fd: {fd}");
                    -1
                }
            }
        }
    }

    impl Process for SyscallContext {
        #[inline]
        fn exit(&self, _caller: Caller, _status: usize) -> isize {
            0
        }

        fn sbrk(&self, caller: Caller, size: i32) -> isize {
            if let Some(process) = unsafe { PROCESSES.get_mut() }.get_mut(caller.entity) {
                if let Some(old_brk) = process.change_program_brk(size as isize) {
                    old_brk as isize
                } else {
                    -1
                }
            } else {
                -1
            }
        }
    }

    impl Scheduling for SyscallContext {
        #[inline]
        fn sched_yield(&self, _caller: Caller) -> isize {
            0
        }
    }

    impl Clock for SyscallContext {
        #[inline]
        fn clock_gettime(&self, caller: Caller, clock_id: ClockId, tp: usize) -> isize {
            const WRITABLE: VmFlags<Sv39> = build_flags("W_V");
            match clock_id {
                ClockId::CLOCK_MONOTONIC => {
                    if let Some(mut ptr) = unsafe { PROCESSES.get_mut() }
                        .get_mut(caller.entity)
                        .unwrap()
                        .address_space
                        .translate::<TimeSpec>(VAddr::new(tp), WRITABLE)
                    {
                        let time = riscv::register::time::read() * 10000 / 125;
                        *unsafe { ptr.as_mut() } = TimeSpec {
                            tv_sec: time / 1_000_000_000,
                            tv_nsec: time % 1_000_000_000,
                        };
                        0
                    } else {
                        log::error!("ptr not readable");
                        -1
                    }
                }
                _ => -1,
            }
        }
    }

    impl Trace for SyscallContext {
        // ======== TODO 2 参考答案 ========
        #[inline]
        fn trace(&self, caller: Caller, trace_request: usize, id: usize, data: usize) -> isize {
            const READABLE: VmFlags<Sv39> = build_flags("U__RV");
            const WRITABLE: VmFlags<Sv39> = build_flags("U_W_V");

            let process = unsafe { PROCESSES.get_mut() }
                .get_mut(caller.entity)
                .unwrap();

            match trace_request {
                0 => {
                    if let Some(ptr) = process
                        .address_space
                        .translate::<u8>(VAddr::new(id), READABLE)
                    {
                        unsafe { ({ *ptr.as_ptr() }) as isize }
                    } else {
                        -1
                    }
                }
                1 => {
                    if let Some(mut ptr) = process
                        .address_space
                        .translate::<u8>(VAddr::new(id), WRITABLE)
                    {
                        unsafe { *ptr.as_mut() = data as u8 };
                        0
                    } else {
                        -1
                    }
                }
                2 => {
                    if id < 512 {
                        process.syscall_counts[id] as isize
                    } else {
                        -1
                    }
                }
                _ => -1,
            }
        }
        // ======== END TODO 2 ========
    }

    impl Memory for SyscallContext {
        // ======== TODO 3 参考答案 ========
        fn mmap(
            &self,
            caller: Caller,
            addr: usize,
            len: usize,
            prot: i32,
            _flags: i32,
            _fd: i32,
            _offset: usize,
        ) -> isize {
            if addr % (1 << Sv39::PAGE_BITS) != 0 { return -1; }
            if prot & !0x7 != 0 { return -1; }
            if prot & 0x7 == 0 { return -1; }
            if len == 0 { return 0; }

            let mut flags: [u8; 5] = *b"U___V";
            if prot & 0x1 != 0 { flags[3] = b'R'; }
            if prot & 0x2 != 0 { flags[2] = b'W'; }
            if prot & 0x4 != 0 { flags[1] = b'X'; }

            let process = unsafe { PROCESSES.get_mut() }
                .get_mut(caller.entity)
                .unwrap();

            let start = VAddr::<Sv39>::new(addr);
            let end = VAddr::<Sv39>::new(addr + len);
            const VALID: VmFlags<Sv39> = build_flags("___V");
            let mut vpn = start.floor();
            while vpn < end.ceil() {
                if process.address_space
                    .translate::<u8>(vpn.base(), VALID)
                    .is_some()
                {
                    return -1;
                }
                vpn = vpn + 1;
            }

            process.address_space.map(
                start.floor()..end.ceil(),
                &[],
                0,
                parse_flags(unsafe { core::str::from_utf8_unchecked(&flags) }).unwrap(),
            );
            0
        }
        // ======== END TODO 3 ========

        // ======== TODO 4 参考答案 ========
        fn munmap(&self, caller: Caller, addr: usize, len: usize) -> isize {
            if addr % (1 << Sv39::PAGE_BITS) != 0 { return -1; }
            if len == 0 { return 0; }

            let process = unsafe { PROCESSES.get_mut() }
                .get_mut(caller.entity)
                .unwrap();

            let start = VAddr::<Sv39>::new(addr);
            let end = VAddr::<Sv39>::new(addr + len);
            const VALID: VmFlags<Sv39> = build_flags("___V");
            let mut vpn = start.floor();
            while vpn < end.ceil() {
                if process.address_space
                    .translate::<u8>(vpn.base(), VALID)
                    .is_none()
                {
                    return -1;
                }
                vpn = vpn + 1;
            }

            process.address_space.unmap(start.floor()..end.ceil());
            0
        }
        // ======== END TODO 4 ========
    }
}

/// 非 RISC-V64 架构的占位实现
#[cfg(not(target_arch = "riscv64"))]
mod stub {
    use tg_kernel_vm::page_table::{MmuMeta, VmFlags};

    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
    pub struct Sv39;

    impl MmuMeta for Sv39 {
        const P_ADDR_BITS: usize = 56;
        const PAGE_BITS: usize = 12;
        const LEVEL_BITS: &'static [usize] = &[9, 9, 9];
        const PPN_POS: usize = 10;

        #[inline]
        fn is_leaf(value: usize) -> bool {
            value & 0b1110 != 0
        }
    }

    pub const fn build_flags(_s: &str) -> VmFlags<Sv39> {
        unsafe { VmFlags::from_raw(0) }
    }

    pub fn parse_flags(_s: &str) -> Result<VmFlags<Sv39>, ()> {
        Ok(unsafe { VmFlags::from_raw(0) })
    }

    #[no_mangle]
    pub extern "C" fn main() -> i32 {
        0
    }

    #[no_mangle]
    pub extern "C" fn __libc_start_main() -> i32 {
        0
    }

    #[no_mangle]
    pub extern "C" fn rust_eh_personality() {}
}
