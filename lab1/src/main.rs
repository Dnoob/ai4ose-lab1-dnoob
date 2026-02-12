//! # Lab1：裸机程序
//!
//! 完成两个 TODO，让程序打印 "Hello, world!" 并关机。
//!
//! 可用的工具函数：
//! - `console_putchar(c: u8)` —— 输出一个字符到屏幕
//! - `shutdown(fail: bool)` —— 关机（false=正常, true=异常）

#![no_std]
#![no_main]
#![cfg_attr(target_arch = "riscv64", deny(warnings, missing_docs))]
#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]

use tg_sbi::{console_putchar, shutdown};

// ============================================================
// TODO 1: 实现 panic 处理函数
//
// 在裸机上，程序出错了只能关机。
// 调用 shutdown(true) 表示异常关机。
// 写完后删掉 loop {}
// ============================================================

/// panic 处理函数。
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // 你的代码写在这里（1行）：

    loop {}
}

// ============================================================
// TODO 2: 实现主函数
//
// 目标：打印 "Hello, world!\n" 然后正常关机
//
// 提示：b"Hello, world!\n" 是字节字符串，可以用 for 循环遍历，
//       每次拿到一个字节的引用，用 *c 取出值传给 console_putchar
//
// 写完后删掉 loop {}
// ============================================================

/// S 态主函数。
extern "C" fn rust_main() -> ! {
    // 你的代码写在这里（3-4行）：

    loop {}
}

// ============================================================
// 以下代码已实现，不需要修改
// ============================================================

/// S 态程序入口点。设置栈指针后跳转到 rust_main。
#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
unsafe extern "C" fn _start() -> ! {
    const STACK_SIZE: usize = 4096;

    #[unsafe(link_section = ".bss.uninit")]
    static mut STACK: [u8; STACK_SIZE] = [0u8; STACK_SIZE];

    core::arch::naked_asm!(
        "la sp, {stack} + {stack_size}",
        "j  {main}",
        stack_size = const STACK_SIZE,
        stack      =   sym STACK,
        main       =   sym rust_main,
    )
}

#[cfg(not(target_arch = "riscv64"))]
mod stub {
    #[unsafe(no_mangle)]
    pub extern "C" fn main() -> i32 { 0 }
    #[unsafe(no_mangle)]
    pub extern "C" fn __libc_start_main() -> i32 { 0 }
    #[unsafe(no_mangle)]
    pub extern "C" fn rust_eh_personality() {}
}
