//! # Lab1 参考答案
//!
//! 裸机程序：在"空电脑"上打印 Hello, world!

#![no_std]
#![no_main]
#![cfg_attr(target_arch = "riscv64", deny(warnings, missing_docs))]
#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]

use tg_sbi::{console_putchar, shutdown};

/// panic 处理函数：程序出错时以异常状态关机。
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    shutdown(true)
}

/// S 态主函数：打印 "Hello, world!" 并关机。
extern "C" fn rust_main() -> ! {
    for c in b"Hello, world!\n" {
        console_putchar(*c);
    }
    shutdown(false)
}

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
