# AI4OSE 实验一 · 学习进展

> **作者**：Dnoob
>
> **仓库**：https://github.com/Dnoob/ai4ose-lab1-dnoob
>
> **起始日期**：2026.02.10

| 日期 | 进度摘要                                                     |
| ---- | ------------------------------------------------------------ |
| Day1 | 环境搭建完成，ch1/ch2 运行通过，学习裸机程序与批处理系统核心概念 |
|      |                                                              |



## Day1（2026.02.10）

### 完成事项

**环境搭建**

- Rust 1.93.0 + `riscv64gc-unknown-none-elf` 交叉编译目标
- QEMU 8.2.2 (`qemu-system-riscv64`)
- cargo 国内镜像（USTC）、`cargo-binutils`、`cargo-clone`
- 克隆组件化实验：`rCore-Tutorial-in-single-workspace`（test 分支）



**实验运行**

- ch1：`cargo run` 成功输出`hello, world`
- ch2: `cargo run` 成功运行8个用户程序，批处理系统行为正确
  -  正常程序（app0/2/5-7）：正常输出并推出
  -  异常退出（app1 StoreFault, app3/4 IllegalInstruction ）: 内核正确捕获并杀死



### 学习笔记

**ch1 — 裸机程序**

核心问题：没有操作系统，怎么在屏幕上打印一行字？

- `#![no_std]`  + `#![no_main]` : 不依赖标准库 和 C runtime
- `_start` 裸函数：手动设置栈指针sp，跳转到`rust_main`
- 通过SBI(Supervisor Binary Interface) 逐字输出，SBI 是比 OS 更底层的固件
- 链接脚本将 `_start` 放到 `0x80200000`，这是 SBI 约定的内核入口地址

启动流程：

```
QEMU加电 -> SBI初始化(M-mode) -> 跳转0x80200000 -> _start设置sp -> rust_main -> 逐字符输出 -> 关机
```



**ch2 — 批处理系统**

核心问题: 让一台"空电脑"能依次运行多个用户程序，一个跑完自动跑完下一个。

三个关键概念：

1.  **特权级**：U-mode(用户程序，权限受限)、S-mode(内核，管理资源)、M-mode（SBI固件）。用户程序不能直接访问硬件，必须通过系统调用请求内核帮忙。
2.  **Trap**：从低级特权陷入高级特权的机制。用户程序执行 `ecall` → 硬件自动保存PC到 `sepc` 
3. **系统调用**：ch2 实现了`write`（输出文字） 和 `exit` (退出程序) 。约定 `a7` 放 syscall ID, `a0-a5` 放参数，返回值写回 `a0`

代码层面：

- **build.rs**（编译时）：下载用户程序源码 → 逐个交叉编译为 RISC-V ELF → `objcopy` 转纯二进制 → 生成 `app.asm` 用 `.incbin` 嵌入内核
- **rust_main**（运行时）：清 BSS → 初始化控制台/syscall → 遍历用户程序 → 创建 `LocalContext`（设 `sepc` 为入口、`sstatus.SPP` 为 User）→ 分配用户栈 → `ctx.execute()` 切到 U-mode → Trap 回来后读 `scause` 判断原因 → syscall 则处理并继续，异常则杀掉程序
- **handle_syscall**：从上下文读 `a7`（ID）和 `a0-a5`（参数），分发处理，结果写回 `a0`，`sepc += 4` 跳过 `ecall` 指令



### 遇到的问题

| 问题                               | 原因         | 解决                                     |
| ---------------------------------- | ------------ | ---------------------------------------- |
| cargo-clone 编译失败找不到 OpenSSL | 缺少系统依赖 | `sudo apt install pkg-config libssl-dev` |
|                                    |              |                                          |

