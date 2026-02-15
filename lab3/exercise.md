# Lab3 习题

## 概念题

### 判断题

**1.** 多道程序系统中，多个用户程序在 CPU 上真正同时执行。

<details>
<summary>答案</summary>

**错误。** CPU 在任意时刻只执行一个程序。多道程序只是让多个程序同时待在内存中，CPU 在它们之间快速切换，给人一种"同时运行"的错觉。

</details>

**2.** 协作式调度中，如果一个程序从不调用 `sys_yield`，其他程序就永远无法运行。

<details>
<summary>答案</summary>

**正确。** 协作式调度完全依赖程序主动让出 CPU。如果某个程序"霸占"CPU 不让出，其他程序就会被饿死。这正是抢占式调度存在的意义——用时钟中断强制切换。

</details>

**3.** 在 lab3 的主循环中，`Event::None`（比如 write 系统调用）会导致切换到下一个任务。

<details>
<summary>答案</summary>

**错误。** `Event::None` 对应的是 `continue`，会回到 loop 顶部继续执行当前任务。只有时钟中断、yield、exit、异常等情况才会 `break` 跳出内层 loop，切换到下一个任务。

</details>

**4.** lab3 中每个用户程序的起始地址都是 `0x80400000`。

<details>
<summary>答案</summary>

**错误。** lab3 中每个程序有不同的起始地址。`build.rs` 在编译时给每个程序分配了不同的地址（比如 A 在 `0x80400000`，B 在 `0x80600000`），这样它们才能同时待在内存中互不干扰。

</details>

### 选择题

**5.** 在 lab3 的抢占式调度中，时钟中断触发后，内核做了什么？

A. 直接杀死当前任务，运行下一个

B. 关闭闹钟，跳出内层 loop，轮转到下一个任务

C. 暂停所有任务，等待用户输入

D. 重启当前任务

<details>
<summary>答案</summary>

**B。** 时钟中断只是时间片用完了，不是出错。内核关闭闹钟（`set_timer(u64::MAX)`），返回 `false`（任务没结束），`break` 跳出内层 loop，外层 while 轮转到下一个任务。下次轮到这个任务时，会从上次中断的地方继续执行。

</details>

---

## 编程挑战：实现 trace 系统调用

> 这是选做题，不影响基础测试（`bash test.sh base`）的通过。

### 题目描述

实现 `sys_trace` 系统调用（编号 410），它根据 `trace_request` 参数做不同的事：

| trace_request | 功能 | 返回值 |
|---|---|---|
| 0 | 读取用户内存 `id` 地址处的 1 个字节 | 该字节的值 |
| 1 | 将 `data` 的低 8 位写入 `id` 地址处 | 0 |
| 2 | 查询当前任务调用编号为 `id` 的系统调用的次数（**本次调用也计入**） | 调用次数 |
| 其他 | 无效 | -1 |

### 需要修改的文件

你需要修改两个文件：

**`src/task.rs`**：
1. 给 `TaskControlBlock` 加一个系统调用计数数组字段
2. 在 `handle_syscall()` 里，每次处理前给对应的计数 +1
3. 提供一个方法让 `main.rs` 能查到计数

**`src/main.rs`**：
1. 把 `Trace::trace()` 的占位实现替换成真正的逻辑

### 提示

<details>
<summary>提示 1：计数器存在哪里？</summary>

每个任务需要独立的计数器。用一个数组 `[usize; 512]`，下标是 syscall 编号，值是调用次数。放在 `TaskControlBlock` 里。

</details>

<details>
<summary>提示 2：计数器怎么从 task.rs 传到 main.rs？</summary>

调用链是 `handle_syscall() → tg_syscall::handle() → Trace::trace()`。中间隔了一个外部库。可以用全局指针做桥梁：在 `handle_syscall()` 里把当前任务的计数数组地址存到 `static mut` 全局变量，`Trace::trace()` 通过公开函数读取。

</details>

<details>
<summary>提示 3：为什么计数要在处理之前 +1？</summary>

题目要求 `trace(request=2)` 查询时"本次调用也计入"。如果在处理之后才 +1，查询 trace 次数时会少算当前这次。

</details>

<details>
<summary>提示 4：读写内存怎么实现？</summary>

把 `id` 参数当作指针，用 `unsafe` 裸指针操作：

```rust
// 读 1 字节
unsafe { *(id as *const u8) as isize }

// 写 1 字节
unsafe { *(id as *mut u8) = data as u8 }
```

</details>

<details>
<summary>提示 5：加了计数数组后程序没有输出？</summary>

`syscall_counts: [usize; 512]` 每个任务多了 4KB，32 个任务多了 128KB，超出了默认内核栈大小。需要把 `main.rs` 里的 `STACK_SIZE` 改大：
```rust
// 改前
const STACK_SIZE: usize = (APP_CAPACITY + 2) * 8192;
// 改后
const STACK_SIZE: usize = (APP_CAPACITY + 2) * 8192 * 2;
```

</details>

### 验证

```bash
bash test.sh exercise
```

7/7 全部通过就说明你的 trace 实现正确了。

参考答案见 [solutions/lab3/](../solutions/lab3/)。
