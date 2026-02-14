# Lab2 习题

## 判断题

1. Trap 发生时，用户程序的所有 32 个通用寄存器是由 CPU 硬件自动保存的。（ ）

<details>
<summary>答案</summary>

❌ 错误。硬件只自动保存 `scause`、`sepc`、`sstatus` 等少数控制寄存器。32 个通用寄存器的保存是**软件**（内核代码）负责的。如果内核不保存就直接开始处理，用户程序的寄存器值会被覆盖，回去后就乱了。

</details>

2. 在批处理系统中，所有用户程序被加载到同一个内存地址，是因为同一时间只有一个程序在运行。（ ）

<details>
<summary>答案</summary>

✅ 正确。批处理是"跑完一个再跑下一个"，同一时间只有一个用户程序在内存中执行，所以可以复用同一块地址空间。下一个程序加载时直接覆盖上一个的内存。

</details>

3. 用户程序在 U-mode 下执行 `sret` 指令，CPU 会正常返回到 S-mode。（ ）

<details>
<summary>答案</summary>

❌ 错误。`sret` 是 S-mode 特权指令，U-mode 没有权限执行。CPU 会检测到违规，触发 IllegalInstruction 异常（一种 Trap），控制权交给内核。内核发现是非法指令，会杀掉这个用户程序。

</details>

4. 系统调用处理完后，如果不执行 `sepc += 4`，用户程序会从 `ecall` 指令继续执行，导致无限循环。（ ）

<details>
<summary>答案</summary>

✅ 正确。硬件把 `sepc` 设为 `ecall` 指令本身的地址。`sret` 回去后 PC = sepc，又执行 `ecall`，再次 Trap，如此往复。加 4 后 sepc 指向下一条指令，就能正常继续了。

</details>

5. `fence.i` 指令的作用是清空数据缓存（d-cache）。（ ）

<details>
<summary>答案</summary>

❌ 错误。`fence.i` 清空的是**指令缓存**（i-cache），不是数据缓存。在批处理系统中，下一个用户程序的二进制数据被写入内存（经过 d-cache），但 CPU 取指令时走的是 i-cache。如果不清空 i-cache，CPU 可能还在执行上一个程序的旧指令。

</details>

## 选择题

6. RISC-V 系统调用约定中，系统调用编号放在哪个寄存器？
   - A. `a0`
   - B. `a5`
   - C. `a7`
   - D. `sp`

<details>
<summary>答案</summary>

C。`a7` 存放系统调用编号，`a0` ~ `a5` 存放参数，`a0` 同时也是返回值。

</details>

7. 当用户程序触发 Trap 时，以下哪个**不是** CPU 硬件自动完成的？
   - A. 将 Trap 原因写入 `scause`
   - B. 将当前 PC 写入 `sepc`
   - C. 保存所有 32 个通用寄存器
   - D. 跳转到 `stvec` 指向的地址

<details>
<summary>答案</summary>

C。硬件自动完成 A（scause ← 原因）、B（sepc ← PC）、D（PC ← stvec）。保存 32 个通用寄存器是软件的职责，由内核的上下文保存代码完成。

</details>

8. 用户程序调用 `write` 系统调用时，以下哪个寄存器存放的是"输出到哪里"（文件描述符）？
   - A. `a7`
   - B. `a0`
   - C. `a1`
   - D. `a2`

<details>
<summary>答案</summary>

B。`a7` 是系统调用编号（64 = write），`a0` 是第一个参数（文件描述符，1 = 屏幕），`a1` 是第二个参数（缓冲区地址），`a2` 是第三个参数（字节数）。

</details>

9. 在 lab2 的批处理系统中，如果 app2 执行了非法指令被内核杀掉，会发生什么？
   - A. 整个系统崩溃，后续程序都无法运行
   - B. 内核跳过 app2，继续运行 app3
   - C. 内核重新运行 app2
   - D. 内核等待用户手动干预

<details>
<summary>答案</summary>

B。这就是特权级保护的意义：用户程序的错误被限制在 U-mode，内核在 S-mode 不受影响。内核检测到异常后杀掉 app2，然后循环继续，加载下一个程序。

</details>

## 代码题

10. **新增 stderr 支持**：当前的 `write` 实现只支持 fd=1（标准输出）和 fd=3（调试输出）。请修改代码，让 fd=2（标准错误）也能输出到屏幕。

提示：只需要改 `impls` 模块中 `IO::write` 的一行代码。

<details>
<summary>答案</summary>

在 match 的分支中加上 fd=2：

```rust
match fd {
    STDOUT | STDDEBUG | 2 => {
        // 原有的打印逻辑不变
        print!("{}", unsafe {
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                buf as *const u8,
                count,
            ))
        });
        count as _
    }
    _ => {
        tg_console::log::error!("unsupported fd: {fd}");
        -1
    }
}
```

</details>
