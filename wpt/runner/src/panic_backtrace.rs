use std::backtrace::{Backtrace, BacktraceStatus};
use std::{cell::Cell, panic::PanicHookInfo};

thread_local! {
    static STASHED_PANIC_INFO: Cell<Option<StashedPanicInfo>> = const { Cell::new(None) };
}

pub fn take_stashed_panic_info() -> Option<StashedPanicInfo> {
    STASHED_PANIC_INFO.take()
}

pub struct StashedPanicInfo {
    pub message: Option<String>,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub backtrace: Backtrace,
}

pub fn stash_panic_handler(info: &PanicHookInfo) {
    let backtrace = Backtrace::force_capture();
    let payload = info.payload();
    let location = info.location().unwrap();

    let str_msg = payload.downcast_ref::<&str>().map(|s| s.to_string());
    let string_msg = payload.downcast_ref::<String>().map(|s| s.to_owned());
    let message = str_msg.or(string_msg);

    let info = StashedPanicInfo {
        message,
        backtrace,
        file: location.file().to_owned(),
        line: location.line(),
        column: location.column(),
    };

    STASHED_PANIC_INFO.with(move |b| b.set(Some(info)));
}

#[inline(never)]
pub fn backtrace_cutoff<R, T: FnOnce() -> R>(cb: T) -> R {
    cb()
}

pub fn trim_backtrace(backtrace: &Backtrace) -> Option<String> {
    if backtrace.status() != BacktraceStatus::Captured {
        return None;
    }

    let string_backtrace = backtrace.to_string();
    let mut filtered = String::with_capacity(string_backtrace.len());
    let mut started = false;

    for line in string_backtrace.lines() {
        if line.contains("wpt::panic_backtrace::backtrace_cutoff") {
            break;
        }

        if started {
            filtered.push_str(line);
            filtered.push('\n');
        }

        if line.contains("core::panicking::panic") {
            started = true;
        }
    }

    Some(filtered)
}
