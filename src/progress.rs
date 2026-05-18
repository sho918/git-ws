use std::fmt;
use std::io::{self, IsTerminal};
#[cfg(test)]
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(test)]
static STEP_LABELS: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[derive(Debug, Clone, Copy)]
pub struct Progress {
    enabled: bool,
}

impl Progress {
    pub fn stderr_for_terminal_output(stdout_is_terminal: bool) -> Self {
        Self {
            enabled: stdout_is_terminal && io::stderr().is_terminal(),
        }
    }

    pub fn disabled() -> Self {
        Self { enabled: false }
    }

    #[cfg(test)]
    pub(crate) fn enabled_for_test() -> Self {
        STEP_LABELS.lock().expect("step labels lock").clear();
        Self { enabled: true }
    }

    #[cfg(test)]
    pub(crate) fn take_step_labels_for_test() -> Vec<String> {
        std::mem::take(&mut *STEP_LABELS.lock().expect("step labels lock"))
    }

    pub fn is_enabled(self) -> bool {
        self.enabled
    }

    pub fn log(self, message: impl fmt::Display) {
        if self.enabled {
            eprintln!("git-ws: {message}");
        }
    }

    pub fn step(self, label: impl Into<String>) -> ProgressStep {
        let label = label.into();
        if self.enabled {
            #[cfg(test)]
            STEP_LABELS
                .lock()
                .expect("step labels lock")
                .push(label.clone());
            eprintln!("git-ws: {label}...");
        }
        ProgressStep {
            enabled: self.enabled,
            label,
            started_at: Instant::now(),
        }
    }

    pub fn run_result<T, E, F>(self, label: impl Into<String>, f: F) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        let step = self.step(label);
        let result = f();
        if result.is_ok() {
            step.done();
        } else {
            step.failed();
        }
        result
    }
}

#[derive(Debug)]
pub struct ProgressStep {
    enabled: bool,
    label: String,
    started_at: Instant,
}

impl ProgressStep {
    pub fn done(self) {
        self.finish("done");
    }

    pub fn failed(self) {
        self.finish("failed");
    }

    fn finish(self, status: &str) {
        if self.enabled {
            eprintln!(
                "git-ws: {} {status} {}",
                self.label,
                format_elapsed(self.started_at.elapsed())
            );
        }
    }
}

fn format_elapsed(elapsed: Duration) -> String {
    let millis = elapsed.as_millis();
    if millis < 1_000 {
        format!("{millis}ms")
    } else {
        format!("{:.1}s", millis as f64 / 1_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_progress_is_not_enabled() {
        assert!(!Progress::disabled().is_enabled());
    }
}
