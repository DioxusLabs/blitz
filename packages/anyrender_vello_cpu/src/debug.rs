#[allow(unused_imports)]
pub(crate) use debug_timer::*;

#[cfg(feature = "log_frame_times")]
mod debug_timer {
    use std::io::{Write, stdout};
    use std::time::Instant;

    pub(crate) struct DebugTimer {
        initial_time: Instant,
        last_time: Instant,
        recorded_times: Vec<(&'static str, u64)>,
    }

    impl DebugTimer {
        pub(crate) fn init() -> Self {
            let time = Instant::now();
            Self {
                initial_time: time,
                last_time: time,
                recorded_times: Vec::new(),
            }
        }

        pub(crate) fn record_time(&mut self, message: &'static str) {
            let now = Instant::now();
            let diff = (now - self.last_time).as_millis() as u64;
            self.recorded_times.push((message, diff));
            self.last_time = now;
        }

        pub(crate) fn print_times(&self, message: &str) {
            let now = Instant::now();
            let overall_ms = (now - self.initial_time).as_millis();

            let mut out = stdout().lock();
            write!(out, "{message}{overall_ms}ms (").unwrap();
            for (idx, time) in self.recorded_times.iter().enumerate() {
                if idx != 0 {
                    write!(out, ", ").unwrap();
                }
                write!(out, "{}: {}ms", time.0, time.1).unwrap();
            }
            writeln!(out, ")").unwrap();
        }
    }
}

#[cfg(not(feature = "log_frame_times"))]
#[allow(dead_code)]
mod debug_timer {
    pub(crate) struct DebugTimer;
    impl DebugTimer {
        pub(crate) fn init() -> Self {
            Self
        }
        pub(crate) fn record_time(&mut self, _message: &'static str) {}
        pub(crate) fn print_times(&self, _message: &str) {}
    }
}
