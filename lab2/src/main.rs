//! # Lab2：批处理系统
//!
//! 完成 4 个 TODO，让内核能依次运行多个用户程序。
//!
//! TODO 顺序建议：TODO 1 → TODO 2 → TODO 3 → TODO 4
//! 全部填完后 cargo run 看效果。

// 不使用标准库，裸机环境没有操作系统提供系统调用支持
#![no_std]
// 不使用标准入口，裸机环境没有 C runtime 进行初始化
#![no_main]
#![cfg_attr(target_arch = "riscv64", deny(missing_docs))]
#![allow(unused)]
#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]

// 引入控制台输出宏（print! / println!），由 tg_console 库提供
#[macro_use]
extern crate tg_console;

// 本地模块：Console 和 SyscallContext 的实现
use impls::{Console, SyscallContext};
// riscv 库：访问 RISC-V 控制状态寄存器（CSR），如 scause
use riscv::register::*;
// 日志模块
use tg_console::log;
// 用户上下文：保存/恢复用户态寄存器，实现特权级切换
use tg_kernel_context::LocalContext;
// SBI 调用：关机等
use tg_sbi;
// 系统调用相关：调用者信息、系统调用 ID
use tg_syscall::{Caller, SyscallId};

// ========== 启动相关 ==========

// 将用户程序的二进制数据内联到内核镜像的 .data 段中
// APP_ASM 由 build.rs 在编译时生成，包含所有用户程序的二进制数据
#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(include_str!(env!("APP_ASM")));

/// 内核入口点：分配 32 KiB 的内核栈，然后跳转到 rust_main。
#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
unsafe extern "C" fn _start() -> ! {
    const STACK_SIZE: usize = 8 * 4096;
    #[unsafe(link_section = ".boot.stack")]
    static mut STACK: [u8; STACK_SIZE] = [0u8; STACK_SIZE];

    core::arch::naked_asm!(
        "la sp, {stack} + {stack_size}",
        "j  {main}",
        stack = sym STACK,
        stack_size = const STACK_SIZE,
        main = sym rust_main,
    )
}

// ========== 内核主函数 ==========

/// 内核主函数：初始化各子系统，然后以批处理方式依次运行所有用户程序。
extern "C" fn rust_main() -> ! {
    // 第一步：清零 BSS 段（未初始化的全局变量区域）
    unsafe { tg_linker::KernelLayout::locate().zero_bss() };

    // 第二步：初始化控制台输出（使 print!/println! 可用）
    tg_console::init_console(&Console);
    tg_console::set_log_level(option_env!("LOG"));
    tg_console::test_log();

    // 第三步：初始化系统调用处理（注册 IO 和 Process 的实现）
    tg_syscall::init_io(&SyscallContext);
    tg_syscall::init_process(&SyscallContext);

    // 第四步：批处理——依次加载并运行每个用户程序
    for (i, app) in tg_linker::AppMeta::locate().iter().enumerate() {
        let app_base = app.as_ptr() as usize;
        log::info!("load app{i} to {app_base:#x}");

        // 创建用户态上下文，入口地址为 app_base
        // LocalContext::user() 会设置 sstatus.SPP = User，
        // 使得 sret 后 CPU 进入 U-mode
        let mut ctx = LocalContext::user(app_base);

        // 分配用户栈（4 KiB），使用 MaybeUninit 避免不必要的零初始化
        let mut user_stack: core::mem::MaybeUninit<[usize; 512]> =
            core::mem::MaybeUninit::uninit();
        let user_stack_ptr = user_stack.as_mut_ptr() as *mut usize;
        // 将用户栈顶地址写入上下文的 sp 寄存器
        *ctx.sp_mut() = unsafe { user_stack_ptr.add(512) } as usize;

        // 循环执行用户程序，直到退出或出错
        loop {
            // execute() 会：
            // 1. 将当前上下文的寄存器恢复到 CPU
            // 2. 执行 sret 切换到 U-mode 运行用户程序
            // 3. 用户程序触发 Trap 后回到这里
            unsafe { ctx.execute() };

            // ============================================================
            // TODO 4: Trap 分支处理
            //
            // 读取 scause 寄存器，判断 Trap 原因：
            //
            // - 如果是 UserEnvCall（用户态 ecall）：
            //     调用 handle_syscall(&mut ctx)，根据返回值：
            //     - Done  → continue（继续运行用户程序）
            //     - Exit(code) → 打印日志 log::info!("app{i} exit with code {code}")
            //     - Error(id) → 打印日志 log::error!("app{i} call an unsupported syscall {}", id.0)
            //
            // - 如果是其他异常：
            //     打印日志 log::error!("app{i} was killed because of {trap:?}")
            //
            // 需要用到：
            //   use scause::{Exception, Trap};
            //   scause::read().cause()             读取 Trap 原因
            //   Trap::Exception(Exception::UserEnvCall)  匹配用户态 ecall
            //   handle_syscall(&mut ctx)            你在 TODO 3 实现的函数
            //   use SyscallResult::*;               引入 Done/Exit/Error
            //
            // ============================================================


            // 清除指令缓存：下一个用户程序会被加载到相同的内存区域，
            // 需要确保 i-cache 中不会残留旧的指令
            unsafe { core::arch::asm!("fence.i") };
            break;
        }
        // 防止编译器优化掉 user_stack
        let _ = core::hint::black_box(&user_stack);
        println!();
    }

    // 所有用户程序执行完毕，关机
    tg_sbi::shutdown(false)
}

// ========== panic 处理 ==========

/// panic 处理函数：打印错误信息后以异常状态关机。
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("{info}");
    tg_sbi::shutdown(true)
}

// ========== 系统调用处理 ==========

/// 系统调用处理结果
enum SyscallResult {
    /// 系统调用完成，继续执行用户程序
    Done,
    /// 用户程序请求退出，附带退出码
    Exit(usize),
    /// 不支持的系统调用
    Error(SyscallId),
}

/// 处理系统调用。
///
/// 从用户上下文中提取系统调用 ID（a7 寄存器）和参数（a0-a5 寄存器），
/// 分发到对应的处理函数，并将返回值写回 a0 寄存器。
fn handle_syscall(ctx: &mut LocalContext) -> SyscallResult {
    use tg_syscall::{SyscallId as Id, SyscallResult as Ret};

    // ============================================================
    // TODO 3: 系统调用分发
    //
    // 要做 4 件事：
    //
    // 1. 从 ctx.a(7) 取出 syscall ID（用 .into() 转换类型）
    // 2. 从 ctx.a(0) ~ ctx.a(5) 取出 6 个参数，放到数组里
    // 3. 调用 tg_syscall::handle(Caller { entity: 0, flow: 0 }, id, args)
    // 4. 根据返回值处理：
    //    - Ret::Done(ret) 且 id 是 EXIT → 返回 SyscallResult::Exit(ctx.a(0))
    //    - Ret::Done(ret) 且 id 不是 EXIT →
    //        把 ret 写入 *ctx.a_mut(0)（返回值放回 a0）
    //        调用 ctx.move_next()（sepc += 4，跳过 ecall）
    //        返回 SyscallResult::Done
    //    - Ret::Unsupported(id) → 返回 SyscallResult::Error(id)
    //
    // 需要用到：
    //   ctx.a(n)        读取参数寄存器
    //   ctx.a_mut(n)    获取参数寄存器的可变引用
    //   ctx.move_next() sepc += 4
    //   Id::EXIT        exit 的系统调用编号
    //
    // ============================================================

    // 删掉下面这行，替换为你的实现
    SyscallResult::Error(SyscallId(0))
}

// ========== 接口实现 ==========

/// 各依赖库所需接口的具体实现
mod impls {
    use tg_syscall::{STDDEBUG, STDOUT};

    /// 控制台实现：通过 SBI 逐字符输出
    pub struct Console;

    impl tg_console::Console for Console {
        #[inline]
        fn put_char(&self, c: u8) {
            tg_sbi::console_putchar(c);
        }
    }

    /// 系统调用上下文实现：处理 IO 和 Process 相关的系统调用
    pub struct SyscallContext;

    /// IO 系统调用实现：处理 write 系统调用
    impl tg_syscall::IO for SyscallContext {
        fn write(
            &self,
            _caller: tg_syscall::Caller,
            fd: usize,
            buf: usize,
            count: usize,
        ) -> isize {
            // ============================================================
            // TODO 1: 实现 write 系统调用
            //
            // 根据 fd（文件描述符）判断输出目标：
            //
            // - fd 是 STDOUT（1）或 STDDEBUG（3）：
            //     把 buf 地址开始的 count 个字节作为字符串打印到屏幕
            //     返回 count as isize（成功写入的字节数）
            //
            // - fd 是其他值：
            //     打印错误日志 log::error!("unsupported fd: {fd}")
            //     返回 -1
            //
            // 需要用到（都在 unsafe 块里）：
            //   core::slice::from_raw_parts(buf as *const u8, count)
            //       从内存地址和长度构造字节切片
            //   core::str::from_utf8_unchecked(字节切片)
            //       把字节切片转成字符串（不检查 UTF-8 合法性）
            //   print!("{}", 字符串)
            //       打印到屏幕
            //
            // ============================================================

            // 删掉下面这行，替换为你的实现
            -1
        }
    }

    /// Process 系统调用实现：处理 exit 系统调用
    impl tg_syscall::Process for SyscallContext {
        #[inline]
        fn exit(&self, _caller: tg_syscall::Caller, _status: usize) -> isize {
            // ============================================================
            // TODO 2: 实现 exit 系统调用
            //
            // 在批处理系统中，exit 的处理很简单：
            // 直接返回 0 表示处理完成。
            // "跑下一个程序"的逻辑在上层（rust_main 的循环）处理。
            //
            // 只需要 1 行代码。
            //
            // ============================================================

            // 删掉下面这行，替换为你的实现
            -1
        }
    }
}

/// 非 RISC-V64 架构的占位模块。
///
/// 提供编译所需的符号，使得 `cargo publish --dry-run` 在主机平台上能通过编译。
#[cfg(not(target_arch = "riscv64"))]
mod stub {
    /// 主机平台占位入口
    #[unsafe(no_mangle)]
    pub extern "C" fn main() -> i32 {
        0
    }

    /// C 运行时占位
    #[unsafe(no_mangle)]
    pub extern "C" fn __libc_start_main() -> i32 {
        0
    }

    /// Rust 异常处理人格占位
    #[unsafe(no_mangle)]
    pub extern "C" fn rust_eh_personality() {}
}
