# Lab1 习题

## 判断题

1. `#![no_std]` 意味着程序完全不能使用任何 Rust 库。（ ）

<details>
<summary>答案</summary>

❌ 错误。`#![no_std]` 只是不使用标准库 `std`，仍然可以使用核心库 `core`（提供基本类型、Option、Result 等）。

</details>

2. 在 RISC-V 架构中，M-mode 的权限比 S-mode 高。（ ）

<details>
<summary>答案</summary>

✅ 正确。RISC-V 特权级从高到低是 M-mode > S-mode > U-mode。

</details>

3. `_start` 函数必须是裸函数，因为普通函数无法在没有栈的情况下运行。（ ）

<details>
<summary>答案</summary>

✅ 正确。普通函数入口会自动生成 push 指令往栈上保存寄存器，但此时 sp 还未初始化，会崩溃。裸函数不生成这些代码。

</details>

4. SBI 运行在 S-mode，为 U-mode 的应用程序提供服务。（ ）

<details>
<summary>答案</summary>

❌ 错误。SBI 运行在 M-mode，为 S-mode 的操作系统内核提供服务。

</details>

## 选择题

5. 以下哪个地址是本实验中 S-mode 代码的起始地址？
   - A. `0x1000`
   - B. `0x80000000`
   - C. `0x80200000`
   - D. `0x80400000`

<details>
<summary>答案</summary>

C。0x1000 是 QEMU 内置引导代码地址，0x80000000 是 M-mode 代码起始地址，0x80400000 是后续章节中用户程序的加载地址。

</details>

6. `console_putchar` 底层通过什么机制请求 M-mode 服务？
   - A. 函数调用
   - B. `ecall` 指令
   - C. 中断
   - D. 共享内存

<details>
<summary>答案</summary>

B。S-mode 执行 ecall 后 CPU 陷入 M-mode，SBI 固件处理请求后返回。

</details>
