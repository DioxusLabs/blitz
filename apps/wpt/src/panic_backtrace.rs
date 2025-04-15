// use std::backtrace::Backtrace;
use std::{cell::Cell, panic::PanicHookInfo};

thread_local! {
    static STASHED_PANIC_INFO: Cell<Option<StashedPanicInfo>> = const { Cell::new(None) };
}

pub fn take_stashed_panic_info() -> Option<StashedPanicInfo> {
    STASHED_PANIC_INFO.take()
}

pub struct StashedPanicInfo {
    // backtrace: Backtrace,
    pub message: Option<String>,
    pub file: String,
    pub line: u32,
    pub column: u32,
}

pub fn stash_panic_handler(info: &PanicHookInfo) {
    // let trace = Backtrace::capture();
    let payload = info.payload();
    let location = info.location().unwrap();

    let str_msg = payload.downcast_ref::<&str>().map(|s| s.to_string());
    let string_msg = payload.downcast_ref::<String>().map(|s| s.to_owned());
    let message = str_msg.or(string_msg);

    let info = StashedPanicInfo {
        message,
        file: location.file().to_owned(),
        line: location.line(),
        column: location.column(),
    };

    STASHED_PANIC_INFO.with(move |b| b.set(Some(info)));
}
