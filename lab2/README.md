# Lab2：批处理系统 —— 让一台电脑依次运行多个程序

## 你会学到什么

完成本实验后，你将理解：
- 什么是批处理系统，它解决了 lab1 的什么问题
- CPU 的特权级（U-mode / S-mode）为什么存在，怎么切换
- Trap 是什么：用户程序出了事，CPU 怎么通知内核
- 系统调用是什么：用户程序怎么请求内核帮忙

> 遇到不熟悉的术语？查看[术语表](../glossary.md)

## 前置要求

- 已完成 Lab1（理解裸机启动、SBI、`_start → rust_main` 流程）
- 能在终端执行 `cargo run` 并看到输出

---

## 第一节：为什么需要批处理系统？

回顾 lab1：你写了一个裸机程序，打印 "Hello, world!" 然后关机。整台电脑从开机到关机，只运行了一个程序。

这有什么问题？想象你有 8 个程序要跑——难道每次都要手动启动 QEMU、跑完一个、退出、再启动、跑下一个？太蠢了。

**批处理系统**就是来解决这个问题的：把多个程序打包在一起，让电脑自动依次运行，一个跑完自动接下一个，全部跑完才关机。

```
Lab1 的世界：
  开机 → 运行程序 A → 关机
  开机 → 运行程序 B → 关机
  开机 → 运行程序 C → 关机
  （每次都要手动操作）

Lab2 的世界：
  开机 → 运行程序 A → 运行程序 B → 运行程序 C → 关机
  （全自动，一口气跑完）
```

但这引出一个新问题：**如果程序 B 有 bug，会不会把整台电脑搞崩，导致程序 C 也跑不了？**

在 lab1 里，你的代码直接运行在 S-mode（高权限），可以执行任何指令、访问任何内存。如果用户程序也有这么大的权限，一个 bug 就可能覆盖内核的代码或数据，整个系统就完了。

解决思路：**把内核和用户程序放在不同的权限等级。** 内核在 S-mode（高权限），用户程序在 U-mode（低权限）。用户程序想干什么"出格"的事，硬件会自动拦截，把控制权交回内核。内核可以选择帮它完成请求，或者直接杀掉它。

这就是 **特权级机制** —— 本章最核心的概念。

---

## 第二节：特权级切换与 Trap

### U-mode 和 S-mode

lab1 里提过 RISC-V 有三个特权级：M-mode、S-mode、U-mode。本章重点关注 S-mode 和 U-mode 这两个：

| 特权级 | 谁在这里运行 | 能做什么 |
|--------|-------------|---------|
| S-mode（内核态） | 操作系统内核 | 几乎什么都能做：访问所有内存、操作硬件寄存器 |
| U-mode（用户态） | 用户程序 | 只能做"普通"的事：计算、访问自己的内存 |

类比一栋大楼：S-mode 是物业管理员，拿着所有房间的钥匙；U-mode 是住户，只能进自己的房间。住户想用电梯、开空调，得找物业帮忙。

**用户程序在 U-mode 下不能做的事包括：**
- 执行特权指令（比如 `sret`，这是内核才能用的指令）
- 访问内核的内存区域
- 直接操作硬件

如果用户程序尝试做这些事，CPU 硬件会立刻拦截，强制切换到 S-mode，让内核来处理。这个"拦截并切换"的过程，就叫 **Trap**。

### Trap：从用户态到内核态

Trap 发生在两种情况下：

**情况一：用户程序主动请求帮助**

用户程序想输出文字到屏幕，但它在 U-mode 没有权限直接操作硬件。怎么办？执行一条特殊指令 `ecall`（environment call），主动请求内核帮忙。这就是 **系统调用**，下一节详细讲。

**情况二：用户程序出了错**

用户程序执行了非法指令、访问了不该访问的内存，CPU 硬件自动检测到违规，强制切换到内核。

不管是哪种情况，Trap 发生时，CPU 硬件会**自动**完成以下动作（注意，是硬件自动做的，不是你写代码做的）：

```
用户程序正在 U-mode 运行
        │
        │ 触发 Trap（ecall 或异常）
        ▼
┌─────────────────────────────────────────┐
│  CPU 硬件自动做的事：                      │
│                                          │
│  1. scause  ← 记录原因（为什么陷入？）      │
│     比如：UserEnvCall（用户调了 ecall）     │
│     比如：IllegalInstruction（非法指令）    │
│                                          │
│  2. sepc    ← 记录地址（陷入时执行到哪了？） │
│     保存用户程序当前的 PC（程序计数器）       │
│                                          │
│  3. sstatus ← 记录状态（陷入前在什么模式？） │
│     SPP 位记录之前是 U-mode 还是 S-mode    │
│                                          │
│  4. PC      ← stvec（跳到内核的处理入口）   │
│     CPU 开始执行内核预先设好的 Trap 处理代码  │
└─────────────────────────────────────────┘
        │
        ▼
  内核的 Trap 处理代码开始运行（S-mode）
```

硬件做完这 4 件事之后，接下来就全是**软件**（你写的内核代码）的工作了：

1. 保存用户程序的所有寄存器（不然内核运行时会覆盖掉）
2. 读 `scause`，判断 Trap 的原因
3. 根据原因做处理（系统调用就帮忙执行，异常就杀掉程序）
4. 恢复用户程序的寄存器
5. 执行 `sret` 指令切回 U-mode，用户程序继续运行

### 上下文保存与恢复

"保存/恢复用户程序的寄存器"这件事非常关键。

想象你在看书，看到第 42 页第 3 行。突然手机响了，你接了个电话。接完电话想继续看——但你忘了刚才看到哪了。

CPU 也一样。用户程序运行时，32 个通用寄存器里存着它正在用的各种数据。Trap 发生后，内核要用这些寄存器来做自己的事（比如判断 Trap 原因、执行系统调用），这会覆盖掉用户程序的数据。

所以在内核开始干活之前，必须把用户程序当前所有寄存器的值**存到内存里**。等内核处理完了，再把这些值**从内存恢复回去**，让用户程序感觉"什么都没发生过"，继续执行。

这些被保存的寄存器数据，就叫 **上下文（Context）**。

在本章的代码中，`tg-kernel-context` 这个库提供了 `LocalContext` 结构体来管理上下文。它帮你封装好了保存/恢复寄存器的底层汇编代码，你只需要调用它的接口就行。

### 完整的一次 Trap 往返

```
用户程序在 U-mode 执行 ecall
  ↓ 硬件自动：scause/sepc/sstatus 写入，PC 跳到内核入口
内核保存用户寄存器到 LocalContext
  ↓
内核读 scause，发现是 UserEnvCall
  ↓
内核处理系统调用（比如把文字输出到屏幕）
  ↓
内核把返回值写回 LocalContext 的 a0 寄存器
  ↓
sepc += 4（跳过 ecall 指令，不然回去又会执行 ecall，死循环）
  ↓
内核恢复用户寄存器，执行 sret
  ↓ 硬件自动：切回 U-mode，PC 恢复为 sepc 的值
用户程序从 ecall 的下一条指令继续执行
```

**为什么 sepc 要加 4？** 硬件把 `sepc` 设为 `ecall` 指令自己的地址。如果不加 4，`sret` 回去后 PC 还是指向 `ecall`，会再次触发 Trap，无限循环。加 4 是因为 RISC-V 中 `ecall` 指令占 4 个字节，加 4 后 PC 就指向下一条指令了。

---

## 第三节：系统调用

上一节说到，用户程序想让内核帮忙时，会执行 `ecall` 触发 Trap。但内核怎么知道用户程序想干什么？"帮我输出文字"和"我要退出"是两个完全不同的请求，内核得能区分。

答案是：**用户程序在执行 `ecall` 之前，把"我要干什么"和"相关参数"提前放到约定好的寄存器里。** 内核收到 Trap 后去这些寄存器里取值，就知道该做什么了。

### 寄存器约定

RISC-V 规定了系统调用的寄存器使用方式：

| 寄存器 | 用途 | 类比 |
|--------|------|------|
| `a7` | 系统调用编号：告诉内核"我要哪项服务" | 餐厅点菜的菜品编号 |
| `a0` ~ `a5` | 参数：告诉内核"具体怎么做" | 菜品的备注（不要辣、多加醋） |
| `a0` | 返回值：内核把结果放回这里 | 服务员端上来的菜 |

注意 `a0` 既是第一个参数，又是返回值——调用前放参数，调用后被内核覆盖成返回值。

### 本章实现的两个系统调用

| 编号 | 名称 | 功能 | 参数 |
|------|------|------|------|
| 64 | `write` | 输出文字 | a0=文件描述符(1=屏幕), a1=文字的内存地址, a2=文字长度 |
| 93 | `exit` | 退出程序 | a0=退出码(0=正常) |

### 一次 write 系统调用的完整过程

用户程序想打印 "Hello"，从用户程序的角度到内核处理完毕，完整流程是这样的：

```
用户程序调用 println!("Hello")
  ↓
用户库把它转成系统调用：
  a7 = 64        （编号 64 = write）
  a0 = 1         （文件描述符 1 = 标准输出，也就是屏幕）
  a1 = buf地址    （"Hello" 字符串在内存中的位置）
  a2 = 5         （字符串长度）
  执行 ecall
  ↓ Trap！
内核接管，读取寄存器：
  a7 = 64 → 这是 write 系统调用
  a0 = 1  → 输出到屏幕
  a1, a2  → 知道了字符串在哪、多长
  ↓
内核把 "Hello" 逐字符通过 SBI 输出到屏幕
  ↓
内核把返回值（成功写入的字节数 5）写入 a0
  ↓
sepc += 4，执行 sret，切回 U-mode
  ↓
用户程序继续执行 ecall 后面的指令
```

### 文件描述符是什么？

你可能注意到 write 的第一个参数是"文件描述符"，值为 1 表示屏幕。这是一个操作系统的传统概念：在操作系统眼里，**一切皆文件**。屏幕是一个"文件"（编号 1），键盘也是一个"文件"（编号 0）。程序不需要知道具体的硬件怎么操作，只需要告诉内核"往 1 号文件写东西"，内核自己去处理硬件细节。

在本章中，我们只支持 1 号（标准输出，屏幕）。往其他编号写，内核会返回错误。

### trait 接口：内核怎么组织系统调用的代码

本章使用 `tg-syscall` 库来管理系统调用。这个库定义了两个接口（trait）：

- **`IO` trait**：处理输入输出相关的系统调用（write）
- **`Process` trait**：处理进程管理相关的系统调用（exit）

你需要做的是**实现这两个 trait**，告诉 `tg-syscall` 库："当用户调了 write，具体该怎么做"和"当用户调了 exit，具体该怎么做"。

如果你不熟悉 trait，可以这样理解：trait 就是一份"合同"，上面列着"你必须提供哪些功能"。库定义了合同内容，你签合同（实现 trait），提供具体的实现代码。

---

## 第四节：理解项目结构

### 和 lab1 的对比

lab1 只有一个依赖（`tg-sbi`），代码也只有一个 `main.rs`。lab2 复杂了不少：

```
lab2/
├── .cargo/
│   └── config.toml        # 和 lab1 一样：交叉编译 + QEMU 运行
├── build.rs                # 🆕 编译脚本：下载编译用户程序，打包进内核
├── Cargo.toml              # 项目配置（依赖比 lab1 多了 5 个）
├── rust-toolchain.toml     # 和 lab1 一样：Rust 工具链版本
├── src/
│   └── main.rs             # 👈 你需要修改的文件
├── README.md               # 本文档
└── exercises.md            # 课后习题
```

和 lab1 相比，最大的区别是 **build.rs 变复杂了**。lab1 的 build.rs 只生成链接脚本（告诉编译器代码放在内存的什么位置）。lab2 的 build.rs 除了生成链接脚本，还要做一件大事：**把用户程序编译好并打包进内核镜像**。

### build.rs 做了什么

批处理系统要运行用户程序，但用户程序从哪里来？答案是：在编译内核的时候，build.rs 把用户程序一起打包进去。

```
cargo build 时发生的事：

1. build.rs 先运行
   ├── 生成链接脚本（和 lab1 一样）
   ├── 下载用户程序源码（tg-user 这个 crate）
   ├── 逐个编译用户程序为 RISC-V 二进制文件
   └── 生成 app.asm：把所有用户程序的二进制数据嵌入内核的 .data 段

2. 然后编译 main.rs
   └── main.rs 里有一行 global_asm! 把 app.asm 包含进来

3. 最终产物：一个内核 ELF 文件，里面包含了所有用户程序
```

你不需要修改 build.rs，但需要知道它的存在，才能理解"用户程序是怎么进到内核里的"。

### 新增的依赖

lab1 只依赖 `tg-sbi`（SBI 调用）。lab2 新增了 5 个依赖，每个都有明确的职责：

| 依赖 | 做什么 | 为什么需要 |
|------|--------|-----------|
| `tg-sbi` | SBI 调用（输出字符、关机） | 和 lab1 一样 |
| `tg-linker` | 链接脚本 + 定位用户程序在内存中的位置 | 内核要知道用户程序在哪 |
| `tg-console` | 提供 `print!` / `println!` 宏和日志 | lab1 是手动逐字符输出，现在升级了 |
| `tg-kernel-context` | 管理上下文（保存/恢复寄存器、特权级切换） | Trap 处理的核心 |
| `tg-syscall` | 系统调用的编号定义和分发框架 | 管理 write/exit 等系统调用 |
| `riscv` | 读写 RISC-V 控制寄存器（如 scause） | 判断 Trap 原因 |

你可以把它们想象成一个团队分工：`tg-kernel-context` 负责"切换场景"，`tg-syscall` 负责"接待用户请求"，`tg-console` 负责"往屏幕上写字"，`riscv` 负责"读硬件状态"，`tg-linker` 负责"找到用户程序在哪"。

### 启动流程（和 lab1 的区别）

```
cargo run
  ↓
build.rs: 编译用户程序，打包进内核
  ↓
编译 main.rs，生成带有用户程序的内核镜像
  ↓
QEMU 启动，加载内核镜像
  ↓
_start → 设置栈指针 → 跳转到 rust_main
  ↓
rust_main:
  ├── 初始化控制台（让 println! 可用）
  ├── 初始化系统调用（注册 write/exit 的处理函数）
  └── 批处理循环：
        ├── 加载第 1 个用户程序 → 切到 U-mode 运行
        │     ├── 用户程序 ecall → Trap → 内核处理 → 返回
        │     └── 用户程序 exit 或出错 → 进入下一个
        ├── 加载第 2 个用户程序 → ...
        ├── ...
        └── 所有程序跑完 → 关机
```

和 lab1 最大的区别：lab1 的 `rust_main` 干完活就关机了；lab2 的 `rust_main` 里有一个**循环**，依次运行每个用户程序，中间不断在 U-mode（运行用户程序）和 S-mode（处理 Trap）之间切换。

---

## 第五节：动手实验

本实验有 4 个 TODO 需要你完成。它们之间有依赖关系（上层调下层），所以建议按顺序填写，**全部填完后再 `cargo run`**。每填完一个可以用 `cargo build` 确认编译通过。

4 个 TODO 的关系：

```
rust_main 的批处理循环
  └── TODO 4: Trap 分支处理（读 scause，区分系统调用和异常）
        └── TODO 3: handle_syscall（提取参数，分发系统调用）
              └── TODO 1: IO::write（把文字输出到屏幕）
              └── TODO 2: Process::exit（处理程序退出）
```

我们从最底层开始填，逐步往上搭建。

### 步骤 0：确认环境

```bash
cd lab2
cargo build
```

第一次编译会比较慢，因为 build.rs 要下载并编译用户程序。如果报错找不到 `cargo-clone`，先安装：

```bash
cargo install cargo-clone
cargo install cargo-binutils
rustup component add llvm-tools
```

编译成功后，试试运行：

```bash
cargo run
```

程序会**卡住或 panic** —— 因为 TODO 还没填。按 `Ctrl+C` 退出，开始填代码。

### 步骤 1：实现 write 系统调用（TODO 1）

打开 `src/main.rs`，找到 `TODO 1`，在 `impls` 模块中。

**目标**：当用户程序调用 write 往屏幕输出文字时，内核把文字打印出来。

**背景**：用户程序通过系统调用传过来三个参数：
- `fd`：文件描述符，1 表示屏幕（标准输出）
- `buf`：文字在内存中的起始地址
- `count`：文字的字节数

你要做的是：判断 `fd` 是不是标准输出（1）或调试输出（3），如果是，就把 `buf` 指向的 `count` 个字节打印到屏幕；如果不是，返回 -1 表示错误。

**需要用到的东西**：
- `STDOUT`（值为 1）和 `STDDEBUG`（值为 3）：已经从 `tg_syscall` 导入
- `print!` 宏：和普通 Rust 的 `print!` 用法一样
- `core::slice::from_raw_parts(buf as *const u8, count)`：从内存地址和长度构造一个字节切片
- `core::str::from_utf8_unchecked(字节切片)`：把字节切片转成字符串

<details>
<summary>还是没思路？点这里看提示</summary>

```rust
match fd {
    STDOUT | STDDEBUG => {
        // 1. 用 from_raw_parts 从 buf 和 count 构造字节切片
        // 2. 用 from_utf8_unchecked 转成字符串
        // 3. 用 print! 打印出来
        // 4. 返回 count as isize（成功写入的字节数）
        // 注意：这两个函数都是 unsafe 的，需要包在 unsafe {} 块里
    }
    _ => {
        // 不支持的文件描述符，返回 -1
    }
}
```

</details>

填完后验证编译：

```bash
cargo build
```

### 步骤 2：实现 exit 系统调用（TODO 2）

找到 `TODO 2`，紧挨着 TODO 1 下面。

**目标**：当用户程序调用 exit 退出时，内核做相应处理。

**背景**：exit 是最简单的系统调用。在批处理系统中，用户程序退出后内核只需要知道"这个程序结束了"，然后去跑下一个。具体的"跑下一个"逻辑在上层处理，这里只需要返回 0 表示处理完成。

这个 TODO 只需要 1 行代码。

<details>
<summary>提示</summary>

直接 `return 0;` 就行。

</details>

填完后验证：`cargo build`

### 步骤 3：实现系统调用分发（TODO 3）

找到 `TODO 3`，在 `handle_syscall` 函数中。

**目标**：从用户上下文中取出系统调用编号和参数，交给 `tg_syscall` 库分发处理，然后把返回值写回去。

**背景**：当 Trap 分支判定"这是一次系统调用"后，会调用 `handle_syscall` 函数。这个函数要做 4 件事：

1. 从上下文的 `a7` 寄存器读取系统调用编号
2. 从 `a0` ~ `a5` 寄存器读取参数
3. 调用 `tg_syscall::handle()` 分发到具体的处理函数（就是你在 TODO 1/2 写的）
4. 根据返回结果：
   - 如果是 EXIT：返回退出码
   - 如果是其他调用：把返回值写入 `a0`，然后 `move_next()`（sepc += 4）
   - 如果不支持：返回错误

**需要用到的接口**：
- `ctx.a(n)`：读取第 n 个参数寄存器（`ctx.a(7)` 就是 syscall ID）
- `ctx.a_mut(n)`：获取第 n 个参数寄存器的可变引用，用于写返回值
- `ctx.move_next()`：sepc += 4，跳过 ecall 指令
- `tg_syscall::handle(caller, id, args)`：分发系统调用
- `SyscallId::EXIT`：exit 的系统调用编号常量

<details>
<summary>还是没思路？点这里看提示</summary>

```rust
// 1. 取 syscall ID
let id = ctx.a(7).into();
// 2. 取 6 个参数
let args = [ctx.a(0), ctx.a(1), ctx.a(2), ctx.a(3), ctx.a(4), ctx.a(5)];
// 3. 分发处理
match tg_syscall::handle(Caller { entity: 0, flow: 0 }, id, args) {
    Ret::Done(ret) => match id {
        Id::EXIT => SyscallResult::Exit(ctx.a(0)),
        _ => {
            // 写返回值到 a0，然后 move_next
            ...
            SyscallResult::Done
        }
    },
    Ret::Unsupported(id) => SyscallResult::Error(id),
}
```

</details>

填完后验证：`cargo build`

### 步骤 4：实现 Trap 分支处理（TODO 4）

找到 `TODO 4`，在 `rust_main` 的批处理循环中。

**目标**：读取 `scause` 寄存器判断 Trap 原因，区分"用户系统调用"和"其他异常"。

**背景**：`ctx.execute()` 切换到 U-mode 运行用户程序，当用户程序触发 Trap 后会回到这里。此时需要读取 `scause` 寄存器，判断发生了什么：

- 如果是 `UserEnvCall`（用户态 ecall）：调用 `handle_syscall` 处理
  - 处理结果是 `Done`：继续运行用户程序（`continue`）
  - 处理结果是 `Exit`：打印退出日志
  - 处理结果是 `Error`：打印错误日志
- 如果是其他异常（非法指令、访存错误等）：打印错误日志，杀掉程序

**需要用到的接口**：
- `scause::read().cause()`：读取 Trap 原因
- `Trap::Exception(Exception::UserEnvCall)`：匹配用户态 ecall
- `handle_syscall(&mut ctx)`：你在 TODO 3 写的函数

<details>
<summary>还是没思路？点这里看提示</summary>

```rust
use scause::{Exception, Trap};
match scause::read().cause() {
    Trap::Exception(Exception::UserEnvCall) => {
        use SyscallResult::*;
        match handle_syscall(&mut ctx) {
            Done => continue,
            Exit(code) => log::info!("app{i} exit with code {code}"),
            Error(id) => log::error!("app{i} call an unsupported syscall {}", id.0),
        }
    }
    trap => log::error!("app{i} was killed because of {trap:?}"),
}
```

</details>

### 步骤 5：运行！

4 个 TODO 都填完了，运行看效果：

```bash
cargo run
```

**预期输出**（大致如下）：

```
[lab2-batch-system 0.1.0] Hello, world!
[ INFO] load app0 to 0x802xxxxx
Hello world from user mode program!
[ INFO] app0 exit with code 0

[ INFO] load app1 to 0x802xxxxx
[ERROR] app1 was killed because of Exception(StoreFault)

[ INFO] load app2 to 0x802xxxxx
...（更多用户程序输出）...

[ INFO] load app7 to 0x802xxxxx
7^160000 = 667897727(MOD 998244353)
Test power_7 OK!
[ INFO] app7 exit with code 0
```

8 个用户程序依次运行：
- 正常程序（app0, app2, app5-7）：输出计算结果，正常退出
- 违规程序（app1, app3, app4）：触发非法操作，被内核杀掉
- 杀掉违规程序后，后面的程序不受影响，继续运行

这就是特权级保护的威力：**一个程序的 bug 不会影响其他程序。**

---

## 第六节：代码解读

你已经填完了所有 TODO，程序也跑起来了。现在我们从头到尾走一遍完整的代码，理解每一部分在做什么。

### 文件头部：属性和依赖

```rust
#![no_std]
#![no_main]
```

和 lab1 一样，裸机环境没有标准库，也没有默认的 main 入口。

```rust
#[macro_use]
extern crate tg_console;
```

引入 `tg_console` 库，并把它的宏（`print!`、`println!`）导入到当前作用域。`#[macro_use]` 是 Rust 的一种写法，意思是"我要用这个库里定义的宏"。

```rust
use riscv::register::*;
use tg_kernel_context::LocalContext;
use tg_syscall::{Caller, SyscallId};
```

导入需要用到的类型：`riscv::register` 提供读写控制寄存器（如 scause）的接口；`LocalContext` 管理用户上下文；`Caller` 和 `SyscallId` 是系统调用分发需要的类型。

### 用户程序嵌入

```rust
core::arch::global_asm!(include_str!(env!("APP_ASM")));
```

这行代码把 build.rs 生成的 `app.asm` 文件内容嵌入进来。`app.asm` 里面是所有用户程序的二进制数据。编译后，这些数据会出现在内核镜像的 `.data` 段中。

拆开来读：
- `env!("APP_ASM")`：读取环境变量 APP_ASM 的值（build.rs 设置的，指向 app.asm 文件路径）
- `include_str!(...)`：把文件内容作为字符串包含进来
- `global_asm!(...)`：把字符串当成汇编代码，编译进程序

### _start 入口

```rust
unsafe extern "C" fn _start() -> ! {
    const STACK_SIZE: usize = 8 * 4096;  // 32 KiB 内核栈
    static mut STACK: [u8; STACK_SIZE] = [0u8; STACK_SIZE];

    core::arch::naked_asm!(
        "la sp, {stack} + {stack_size}",
        "j  {main}",
        ...
    )
}
```

和 lab1 的 `_start` 几乎一样，唯一的区别是栈大了：lab1 是 4 KiB，lab2 是 32 KiB（8 页）。因为 lab2 要做更多事情（Trap 处理、系统调用），需要更大的栈空间。

### rust_main：批处理主循环

这是整个程序的核心。分为两个阶段：

**阶段一：初始化**

```rust
// 清零 BSS 段
unsafe { tg_linker::KernelLayout::locate().zero_bss() };
// 初始化控制台（让 print!/println! 可用）
tg_console::init_console(&Console);
// 初始化系统调用（注册 write 和 exit 的处理函数）
tg_syscall::init_io(&SyscallContext);
tg_syscall::init_process(&SyscallContext);
```

BSS 段是存放未初始化全局变量的内存区域，启动时必须清零，否则这些变量的值是随机的。初始化控制台和系统调用，就是把你在 `impls` 模块中实现的 `Console`、`SyscallContext` 注册进去，让后续代码能用。

**阶段二：批处理循环**

```rust
for (i, app) in tg_linker::AppMeta::locate().iter().enumerate() {
```

`tg_linker::AppMeta::locate()` 从内核镜像中找到 build.rs 打包的用户程序列表。`iter()` 逐个遍历，`enumerate()` 同时给出编号 `i`。

```rust
    let mut ctx = LocalContext::user(app_base);
```

为用户程序创建一个上下文。`LocalContext::user(entry)` 做了两件事：把 `sepc` 设为用户程序的入口地址，把 `sstatus.SPP` 设为 User，这样 `sret` 后 CPU 就会进入 U-mode。

```rust
    let mut user_stack: core::mem::MaybeUninit<[usize; 512]> =
        core::mem::MaybeUninit::uninit();
    *ctx.sp_mut() = unsafe { user_stack_ptr.add(512) } as usize;
```

在内核栈上分配 4 KiB 作为用户栈，把栈顶地址写入上下文的 `sp` 寄存器。用户程序运行时会用这个栈。`MaybeUninit` 的意思是"这块内存不需要初始化"，因为用户程序会自己往里写数据。

```rust
    loop {
        unsafe { ctx.execute() };
        // Trap 返回后到这里，读 scause 判断原因...
    }
```

`ctx.execute()` 是真正的"切换"：它恢复用户寄存器、执行 `sret` 跳到 U-mode。用户程序开始运行。当 Trap 发生时，控制权回到 `execute()` 的下一行。

循环里就是你在 TODO 4 填的 Trap 分支处理。

```rust
    unsafe { core::arch::asm!("fence.i") };
```

`fence.i` 是 RISC-V 的指令缓存屏障。CPU 内部有指令缓存（i-cache），当你把新的用户程序加载到同一块内存时，缓存里可能还残留着上一个程序的指令。`fence.i` 强制清空指令缓存，确保 CPU 执行的是新程序的代码。

### handle_syscall：系统调用分发

```rust
fn handle_syscall(ctx: &mut LocalContext) -> SyscallResult {
    let id = ctx.a(7).into();     // 从 a7 取 syscall ID
    let args = [ctx.a(0), ctx.a(1), ctx.a(2), ctx.a(3), ctx.a(4), ctx.a(5)];

    match tg_syscall::handle(Caller { entity: 0, flow: 0 }, id, args) {
        ...
    }
}
```

这个函数是 Trap 处理和具体系统调用实现之间的桥梁。它从上下文中提取寄存器值，交给 `tg_syscall::handle` 分发。`Caller { entity: 0, flow: 0 }` 是调用者信息，在批处理系统中只有一个程序在跑，所以固定为 0。

`tg_syscall::handle` 内部会根据 syscall ID 调用你实现的 `IO::write` 或 `Process::exit`。

### impls 模块：接口实现

```rust
mod impls {
    pub struct Console;
    impl tg_console::Console for Console { ... }

    pub struct SyscallContext;
    impl tg_syscall::IO for SyscallContext { ... }
    impl tg_syscall::Process for SyscallContext { ... }
}
```

这个模块定义了两个结构体，分别实现了库要求的 trait：

- `Console` 实现了 `tg_console::Console` trait：通过 SBI 逐字符输出，让 `println!` 能工作
- `SyscallContext` 实现了 `IO` 和 `Process` trait：这就是你在 TODO 1 和 TODO 2 填的内容

### stub 模块

```rust
#[cfg(not(target_arch = "riscv64"))]
mod stub { ... }
```

和 lab1 一样，这是为了在 x86 电脑上编译检查（`cargo publish --dry-run`）能通过而提供的占位代码。在 RISC-V 目标上编译时，这个模块完全不存在。

---

## 第七节：总结

回顾一下，lab2 在 lab1 的基础上实现了四个关键进步：

| | Lab1 | Lab2 |
|---|------|------|
| 运行几个程序 | 1 个 | 多个，依次自动运行 |
| 特权级 | 只有 S-mode | S-mode（内核）+ U-mode（用户程序） |
| 保护机制 | 无 | 用户程序出错，内核杀掉它，不影响后续程序 |
| 与硬件交互 | 直接调 SBI | 用户程序通过系统调用请求内核帮忙 |

你实现的 4 个 TODO 分别对应 4 个核心概念：

| TODO | 你做了什么 | 对应的概念 |
|------|-----------|-----------|
| TODO 1: write | 把用户程序的文字输出到屏幕 | 系统调用的具体实现 |
| TODO 2: exit | 处理用户程序退出 | 系统调用的具体实现 |
| TODO 3: handle_syscall | 从寄存器取参数，分发到处理函数 | 系统调用的分发机制 |
| TODO 4: Trap 分支 | 区分系统调用和异常 | Trap 处理与特权级切换 |

完整的数据流：

```
用户程序执行 ecall
  → CPU 硬件自动切到 S-mode，跳到内核
  → 内核读 scause（TODO 4）
  → 发现是 UserEnvCall，调用 handle_syscall（TODO 3）
  → handle_syscall 提取 a7 和参数，分发到 write（TODO 1）或 exit（TODO 2）
  → 处理完毕，返回值写回 a0，sepc += 4
  → sret 切回 U-mode，用户程序继续执行
```

但 lab2 仍然有一个重要的局限：**同一时间只有一个程序在运行。** 一个程序跑完（或被杀），才能跑下一个。如果一个程序在等待（比如等用户输入），CPU 就干等着，浪费算力。

lab3 将解决这个问题——让多个程序同时驻留在内存中，CPU 在它们之间来回切换，实现**分时共享**。

---

完成实验后，试试[习题](exercises.md)检验理解。如果实验中遇到困难，可以参考[参考答案](../solutions/lab2/src/main.rs)。
