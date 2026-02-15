//! 任务管理模块（参考答案）
//!
//! 包含基础 TODO 1 的答案和 trace 练习题的完整实现。

use tg_kernel_context::LocalContext;
use tg_syscall::{Caller, SyscallId};

// ==================== trace 练习题相关 ====================

const MAX_SYSCALL_NUM: usize = 512;

static mut CURRENT_SYSCALL_COUNTS: *mut [usize; MAX_SYSCALL_NUM] = core::ptr::null_mut();

/// 供 main.rs 中 Trace 实现调用：查询当前任务某个 syscall 的调用次数
pub fn current_syscall_count(id: usize) -> usize {
    if id >= MAX_SYSCALL_NUM {
        return 0;
    }
    unsafe {
        if CURRENT_SYSCALL_COUNTS.is_null() {
            0
        } else {
            (*CURRENT_SYSCALL_COUNTS)[id]
        }
    }
}

// ==================== 任务控制块 ====================

/// 任务控制块（Task Control Block, TCB）
pub struct TaskControlBlock {
    ctx: LocalContext,
    pub finish: bool,
    stack: [usize; 1024],
    syscall_counts: [usize; MAX_SYSCALL_NUM],
}

/// 调度事件
pub enum SchedulingEvent {
    None,
    Yield,
    Exit(usize),
    UnsupportedSyscall(SyscallId),
}

impl TaskControlBlock {
    pub const ZERO: Self = Self {
        ctx: LocalContext::empty(),
        finish: false,
        stack: [0; 1024],
        syscall_counts: [0; MAX_SYSCALL_NUM],
    };

    pub fn init(&mut self, entry: usize) {
        self.stack.fill(0);
        self.finish = false;
        self.syscall_counts = [0; MAX_SYSCALL_NUM];
        self.ctx = LocalContext::user(entry);
        // ★ TODO 1 答案：设置用户栈指针为栈顶（数组末尾地址）
        *self.ctx.sp_mut() = self.stack.as_ptr() as usize + core::mem::size_of_val(&self.stack);
    }

    #[inline]
    pub unsafe fn execute(&mut self) {
        unsafe { self.ctx.execute() };
    }

    pub fn handle_syscall(&mut self) -> SchedulingEvent {
        use tg_syscall::{SyscallId as Id, SyscallResult as Ret};
        use SchedulingEvent as Event;

        let id = self.ctx.a(7).into();
        let args = [
            self.ctx.a(0),
            self.ctx.a(1),
            self.ctx.a(2),
            self.ctx.a(3),
            self.ctx.a(4),
            self.ctx.a(5),
        ];

        // ★ trace 练习题：计数 +1（在处理之前）
        let id_num = self.ctx.a(7);
        if id_num < MAX_SYSCALL_NUM {
            self.syscall_counts[id_num] += 1;
        }

        // ★ trace 练习题：设置全局指针
        unsafe {
            CURRENT_SYSCALL_COUNTS = &mut self.syscall_counts as *mut _;
        }

        match tg_syscall::handle(Caller { entity: 0, flow: 0 }, id, args) {
            Ret::Done(ret) => match id {
                Id::EXIT => Event::Exit(self.ctx.a(0)),
                Id::SCHED_YIELD => {
                    *self.ctx.a_mut(0) = ret as _;
                    self.ctx.move_next();
                    Event::Yield
                }
                _ => {
                    *self.ctx.a_mut(0) = ret as _;
                    self.ctx.move_next();
                    Event::None
                }
            },
            Ret::Unsupported(_) => Event::UnsupportedSyscall(id),
        }
    }
}
