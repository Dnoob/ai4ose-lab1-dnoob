//! 第四章：地址空间
//!
//! 本章实现了基于 RISC-V Sv39 的虚拟内存管理，为每个进程提供独立的地址空间。
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
    // bss 段清零
    unsafe { layout.zero_bss() };
    // 初始化 `console`
    tg_console::init_console(&Console);
    tg_console::set_log_level(option_env!("LOG"));
    tg_console::test_log();
    // 初始化内核堆
    tg_kernel_alloc::init(layout.start() as _);
    unsafe {
        tg_kernel_alloc::transfer(core::slice::from_raw_parts_mut(
            layout.end() as _,
            MEMORY - layout.len(),
        ))
    };
    // 建立异界传送门
    let portal_size = MultislotPortal::calculate_size(1);
    let portal_layout = Layout::from_size_align(portal_size, 1 << Sv39::PAGE_BITS).unwrap();
    let portal_ptr = unsafe { alloc(portal_layout) };
    assert!(portal_layout.size() < 1 << Sv39::PAGE_BITS);
    // 建立内核地址空间
    let mut ks = kernel_space(layout, MEMORY, portal_ptr as _);
    let portal_idx = PROTAL_TRANSIT.index_in(Sv39::MAX_LEVEL);
    // 加载应用程序
    for (i, elf) in tg_linker::AppMeta::locate().iter().enumerate() {
        let base = elf.as_ptr() as usize;
        log::info!("detect app[{i}]: {base:#x}..{:#x}", base + elf.len());
        if let Some(process) = Process::new(ElfFile::new(elf).unwrap()) {
            // 映射异界传送门
            process.address_space.root()[portal_idx] = ks.root()[portal_idx];
            unsafe { PROCESSES.get_mut().push(process) };
        }
    }

    // 建立调度栈
    const PAGE: Layout =
        unsafe { Layout::from_size_align_unchecked(2 << Sv39::PAGE_BITS, 1 << Sv39::PAGE_BITS) };
    let pages = 2;
    let stack = unsafe { alloc(PAGE) };
    ks.map_extern(
        VPN::new((1 << 26) - pages)..VPN::new(1 << 26),
        PPN::new(stack as usize >> Sv39::PAGE_BITS),
        build_flags("_WRV"),
    );
    // 建立调度线程，目的是划分异常域。调度线程上发生内核异常时会回到这个控制流处理
    let mut scheduling = LocalContext::thread(schedule as *const () as _, false);
    *scheduling.sp_mut() = 1 << 38;
    unsafe { scheduling.execute() };
    log::error!("stval = {:#x}", stval::read());
    panic!("trap from scheduling thread: {:?}", scause::read().cause());
}

extern "C" fn schedule() -> ! {
    // 初始化异界传送门
    let portal = unsafe { MultislotPortal::init_transit(PROTAL_TRANSIT.base().val(), 1) };
    // 初始化 syscall
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

                // ===========================================================
                // TODO 1：记录系统调用次数
                // ===========================================================
                // 需求：
                //   - 从 ctx.a(7) 取出系统调用编号（usize 类型）
                //   - 如果编号 < 512，给当前进程的 syscall_counts[编号] 加 1
                //
                // 提示：
                //   - 当前进程是 PROCESSES[0]
                //   - 访问 PROCESSES 需要 unsafe { PROCESSES.get_mut() }
                //   - 这段代码约 3 行
                // ===========================================================
                // >>> 在此处填写代码 <<<

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

/// Rust 异常处理函数，以异常方式关机。
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
    #[allow(unused_imports)]
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
        #[inline]
        fn trace(&self, _caller: Caller, _trace_request: usize, _id: usize, _data: usize) -> isize {
            // =============================================================
            // TODO 2：实现 trace 系统调用
            // =============================================================
            // 功能：通过地址翻译，在内核中读写用户地址空间的数据
            //
            // 参数说明：
            //   caller        - 调用者信息，caller.entity 是进程下标
            //   trace_request - 操作类型：0=读字节, 1=写字节, 2=查询syscall次数
            //   id            - 操作类型 0/1 时是虚拟地址，类型 2 时是 syscall 编号
            //   data          - 操作类型 1 时要写入的字节值
            //
            // 返回值：
            //   操作 0：成功返回读到的字节值(as isize)，失败返回 -1
            //   操作 1：成功返回 0，失败返回 -1
            //   操作 2：返回 syscall_counts[id]，id >= 512 时返回 -1
            //   其他：  返回 -1
            //
            // 提示：
            //   1. 先去掉参数前的 _ 下划线（表示参数现在要使用了）
            //   2. 用 build_flags 定义读/写权限常量（参考 clock_gettime）
            //      注意：用户态页面必须带 U 标志！对比 clock_gettime 的 "W_V"
            //   3. 用 process.address_space.translate::<u8>(...) 做地址翻译
            //   4. 操作 2 直接从 process.syscall_counts 数组读取
            //
            // 参考：上方 clock_gettime 的 translate 用法
            // =============================================================
            todo!("TODO 2")
        }
    }

    impl Memory for SyscallContext {
        fn mmap(
            &self,
            _caller: Caller,
            _addr: usize,
            _len: usize,
            _prot: i32,
            _flags: i32,
            _fd: i32,
            _offset: usize,
        ) -> isize {
            // =============================================================
            // TODO 3：实现 mmap 系统调用
            // =============================================================
            // 功能：申请物理内存，映射到用户指定的虚拟地址范围
            //
            // 参数说明：
            //   addr - 起始虚拟地址（必须页对齐，即 4096 的整数倍）
            //   len  - 映射长度（字节），为 0 时直接返回 0
            //   prot - 权限位：bit0=R, bit1=W, bit2=X（不能全 0，高位必须为 0）
            //
            // 返回值：成功返回 0，失败返回 -1
            //
            // 实现步骤：
            //   1. 参数校验（4 个检查，任一失败返回 -1）：
            //      - addr 页对齐？  addr % (1 << Sv39::PAGE_BITS) != 0
            //      - prot 高位为 0？ prot & !0x7 != 0
            //      - prot 非全零？   prot & 0x7 == 0
            //      - len 为 0？      直接返回 0（不是错误）
            //
            //   2. 把 prot 转成页表权限标志：
            //      提示：参考 process.rs 中 ELF 段映射的 flags 构建方式
            //      注意用户页面必须有 U 标志
            //
            //   3. 逐页检查目标范围内没有已映射的页（防止重复映射 panic）：
            //      提示：用 translate + is_some() 检查
            //
            //   4. 调用 process.address_space.map(...) 完成映射
            //      提示：参考 change_program_brk 中扩展堆的 map 调用
            //
            // 参考：process.rs 的 ELF 段映射 和 change_program_brk
            // =============================================================
            todo!("TODO 3")
        }

        fn munmap(&self, _caller: Caller, _addr: usize, _len: usize) -> isize {
            // =============================================================
            // TODO 4：实现 munmap 系统调用
            // =============================================================
            // 功能：取消虚拟地址范围的映射（与 mmap 对称）
            //
            // 与 mmap 的区别：
            //   - 不需要 prot 参数
            //   - 检查方向相反：要求范围内所有页都已映射（is_none → -1）
            //   - 用 unmap 代替 map
            //
            // 参考：mmap 的实现 + change_program_brk 中收缩堆的 unmap
            // =============================================================
            todo!("TODO 4")
        }
    }
}

/// 非 RISC-V64 架构的占位实现
#[cfg(not(target_arch = "riscv64"))]
mod stub {
    use tg_kernel_vm::page_table::{MmuMeta, VmFlags};

    /// Sv39 占位类型
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

    /// 构建 VmFlags 占位。
    pub const fn build_flags(_s: &str) -> VmFlags<Sv39> {
        unsafe { VmFlags::from_raw(0) }
    }

    /// 解析 VmFlags 占位。
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
