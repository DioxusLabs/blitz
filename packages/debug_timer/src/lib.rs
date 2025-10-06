#[cfg(feature = "enable")]
mod real_debug_timer {
    use std::io::{stdout, Write};
    use std::time::{Duration, Instant};

    const SECOND: Duration = Duration::from_secs(1);
    const MILLISECOND: Duration = Duration::from_millis(1);
    const MICROSECOND: Duration = Duration::from_micros(1);

    pub struct DebugTimer {
        initial_time: Instant,
        recorded_times: Vec<(&'static str, Instant)>,
    }

    fn value_and_units(duration: Duration) -> (f32, &'static str) {
        if duration < MICROSECOND {
            (duration.subsec_nanos() as f32, "ns")
        } else if duration < MILLISECOND {
            (duration.subsec_nanos() as f32 / 1000.0, "us")
        } else if duration < SECOND {
            (duration.subsec_micros() as f32 / 1000.0, "ms")
        } else {
            (duration.as_millis() as f32 / 1000.0, "s")
        }
    }

    impl DebugTimer {
        pub fn init() -> Self {
            let time = Instant::now();
            Self {
                initial_time: time,
                recorded_times: Vec::new(),
            }
        }

        pub fn record_time(&mut self, message: &'static str) {
            self.recorded_times.push((message, Instant::now()));
        }

        pub fn print_times(&self, message: &str) {
            let now = Instant::now();
            let (overall_val, overall_unit) = value_and_units(now - self.initial_time);

            let mut out = stdout().lock();
            if overall_val < 10.0 {
                write!(out, "{message}{overall_val:.1}{overall_unit} (").unwrap();
            } else {
                write!(out, "{message}{overall_val:.0}{overall_unit} (").unwrap();
            }

            for (idx, times) in self.recorded_times.windows(2).enumerate() {
                let last = times[0];
                let current = times[1];

                if idx != 0 {
                    write!(out, ", ").unwrap();
                }

                let duration = current.1.duration_since(last.1);

                let (val, unit) = value_and_units(duration);
                if val < 10.0 {
                    write!(out, "{}: {val:.1}{unit}", current.0).unwrap();
                } else {
                    write!(out, "{}: {val:.0}{unit}", current.0).unwrap();
                }
            }
            writeln!(out, ")").unwrap();
        }
    }
}

mod dummy_debug_timer {
    pub struct DebugTimer;
    impl DebugTimer {
        #[inline(always)]
        pub fn init() -> Self {
            Self
        }
        #[inline(always)]
        pub fn record_time(&mut self, _message: &'static str) {}
        #[inline(always)]
        pub fn print_times(&self, _message: &str) {}
    }
}

#[cfg(feature = "enable")]
#[macro_export]
macro_rules! debug_timer {
    ($id:ident, $($cond:tt)*) => {
        let mut $id =  {
            #[cfg($($cond)*)]
            let timer = $crate::RealDebugTimer::init();
            #[cfg(not($($cond)*))]
            let timer = $crate::DummyDebugTimer::init();
            timer
        };
    };
}

#[cfg(feature = "enable")]
#[macro_export]
macro_rules! debug_timer_type {
    ($id:ident, $($cond:tt)*) => {
        #[cfg($($cond)*)]
        pub type $id = $crate::RealDebugTimer;
        #[cfg(not($($cond)*))]
        pub type $id = $crate::DummyDebugTimer;
    };
}

#[cfg(not(feature = "enable"))]
#[macro_export]
macro_rules! debug_timer {
    ($id:ident, $($cond:tt)*) => {
        let mut $id = $crate::DummyDebugTimer::init();
    };
}

#[cfg(not(feature = "enable"))]
#[macro_export]
macro_rules! debug_timer_type {
    ($id:ident, $($cond:tt)*) => {
        pub type $id = $crate::DummyDebugTimer;
    };
}

pub use dummy_debug_timer::DebugTimer as DummyDebugTimer;
#[cfg(feature = "enable")]
pub use real_debug_timer::DebugTimer as RealDebugTimer;
