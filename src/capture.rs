use std::io;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use crate::ffmpeg::{build_capture_command, CaptureConfig};

const SHELL_SIGINT_EXIT_CODE: i32 = 130;
const FFMPEG_INTERRUPT_EXIT_CODE: i32 = 255;

/// Trait for managing ffmpeg processes. Allows mocking in tests.
pub trait FfmpegProcess {
    fn spawn(&mut self) -> anyhow::Result<()>;
    fn request_stop(&mut self) -> anyhow::Result<()>;
    fn is_running(&mut self) -> bool;
    fn wait_for_exit(&mut self) -> anyhow::Result<Option<i32>>;
    fn take_stderr(&mut self) -> Option<Vec<u8>>;
}

/// Real ffmpeg process manager.
pub struct RealFfmpegProcess {
    config: CaptureConfig,
    child: Option<Child>,
}

impl RealFfmpegProcess {
    pub fn new(config: CaptureConfig) -> Self {
        Self {
            config,
            child: None,
        }
    }
}

impl FfmpegProcess for RealFfmpegProcess {
    fn spawn(&mut self) -> anyhow::Result<()> {
        let mut cmd = build_capture_command(&self.config);

        if !self.config.verbose {
            cmd.stderr(Stdio::null());
        }

        let child = cmd.spawn()?;
        self.child = Some(child);
        Ok(())
    }

    fn request_stop(&mut self) -> anyhow::Result<()> {
        if let Some(child) = self.child.as_mut() {
            // ffmpeg finalizes the MP4 trailer when it receives SIGINT. Using
            // Child::kill would send SIGKILL and leave the recording corrupt.
            #[cfg(unix)]
            {
                let result = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) };
                if result == -1 {
                    let error = io::Error::last_os_error();
                    // The process may have finished between is_running() and
                    // this call; in that case there is nothing left to signal.
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(error.into());
                    }
                }
            }

            #[cfg(not(unix))]
            child.kill()?;
        }
        Ok(())
    }

    fn is_running(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(_)) => false,
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            false
        }
    }

    fn wait_for_exit(&mut self) -> anyhow::Result<Option<i32>> {
        if let Some(ref mut child) = self.child {
            let status = child.wait()?;
            Ok(status.code())
        } else {
            Ok(None)
        }
    }

    fn take_stderr(&mut self) -> Option<Vec<u8>> {
        // stderr is already consumed by the process when it exits
        // We can't capture it after the fact without storing the handle
        // For now, return None - verbose mode shows stderr directly
        None
    }
}

/// Capture session state.
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureState {
    Idle,
    Running,
    Stopped,
    Error(String),
}

/// Capture session manager.
pub struct CaptureSession {
    state: CaptureState,
    process: Box<dyn FfmpegProcess>,
    start_time: Option<Instant>,
    duration: Option<Duration>,
    stop_requested: bool,
}

impl CaptureSession {
    pub fn new(process: Box<dyn FfmpegProcess>, duration: Option<Duration>) -> Self {
        Self {
            state: CaptureState::Idle,
            process,
            start_time: None,
            duration,
            stop_requested: false,
        }
    }

    pub fn start(&mut self) -> anyhow::Result<()> {
        if self.state == CaptureState::Running {
            anyhow::bail!("Capture session already running");
        }

        self.process.spawn()?;
        self.state = CaptureState::Running;
        self.start_time = Some(Instant::now());
        self.stop_requested = false;
        Ok(())
    }

    pub fn stop(&mut self) -> anyhow::Result<()> {
        if self.state != CaptureState::Running {
            anyhow::bail!("No capture session running");
        }

        self.process.request_stop()?;
        self.stop_requested = true;
        self.state = CaptureState::Stopped;
        Ok(())
    }

    pub fn state(&self) -> &CaptureState {
        &self.state
    }

    /// Return whether ffmpeg exited before the user requested a stop.
    pub fn has_exited(&mut self) -> bool {
        self.state == CaptureState::Running && !self.process.is_running()
    }

    pub fn is_duration_expired(&self) -> bool {
        if let (Some(start), Some(duration)) = (self.start_time, self.duration) {
            start.elapsed() >= duration
        } else {
            false
        }
    }

    /// Elapsed time since the session started. Returns `Duration::ZERO` before
    /// `start` is called so callers can render a status line at any time.
    pub fn elapsed(&self) -> Duration {
        match self.start_time {
            Some(start) => start.elapsed(),
            None => Duration::ZERO,
        }
    }

    /// Configured session duration (`-d`). `None` for indefinite captures.
    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    pub fn check_and_stop_if_expired(&mut self) -> anyhow::Result<bool> {
        if self.state == CaptureState::Running && self.is_duration_expired() {
            self.state = CaptureState::Stopped;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Wait for the ffmpeg process to exit and check its exit code.
    /// Returns an error if ffmpeg exited unexpectedly with a non-zero code.
    pub fn finish(&mut self) -> anyhow::Result<()> {
        let exit_code = self.process.wait_for_exit()?;
        self.state = CaptureState::Stopped;

        if let Some(code) = exit_code {
            let expected_interrupt = self.stop_requested
                && matches!(code, SHELL_SIGINT_EXIT_CODE | FFMPEG_INTERRUPT_EXIT_CODE);
            if code != 0 && !expected_interrupt {
                anyhow::bail!("ffmpeg exited with code {}", code);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::thread;

    /// Mock ffmpeg process for testing.
    struct MockFfmpegProcess {
        spawned: Rc<RefCell<bool>>,
        stop_requested: Rc<RefCell<bool>>,
        running: Rc<RefCell<bool>>,
        exit_code: Option<i32>,
    }

    impl MockFfmpegProcess {
        fn new() -> (Self, Rc<RefCell<bool>>, Rc<RefCell<bool>>, Rc<RefCell<bool>>) {
            Self::with_exit_code(Some(0))
        }

        fn with_exit_code(
            exit_code: Option<i32>,
        ) -> (Self, Rc<RefCell<bool>>, Rc<RefCell<bool>>, Rc<RefCell<bool>>) {
            let spawned = Rc::new(RefCell::new(false));
            let stop_requested = Rc::new(RefCell::new(false));
            let running = Rc::new(RefCell::new(false));

            let process = Self {
                spawned: spawned.clone(),
                stop_requested: stop_requested.clone(),
                running: running.clone(),
                exit_code,
            };

            (process, spawned, stop_requested, running)
        }
    }

    impl FfmpegProcess for MockFfmpegProcess {
        fn spawn(&mut self) -> anyhow::Result<()> {
            *self.spawned.borrow_mut() = true;
            *self.running.borrow_mut() = true;
            Ok(())
        }

        fn request_stop(&mut self) -> anyhow::Result<()> {
            *self.stop_requested.borrow_mut() = true;
            *self.running.borrow_mut() = false;
            Ok(())
        }

        fn is_running(&mut self) -> bool {
            *self.running.borrow()
        }

        fn wait_for_exit(&mut self) -> anyhow::Result<Option<i32>> {
            *self.running.borrow_mut() = false;
            Ok(self.exit_code)
        }

        fn take_stderr(&mut self) -> Option<Vec<u8>> {
            None
        }
    }

    #[test]
    fn session_starts_in_idle_state() {
        let (process, _, _, _) = MockFfmpegProcess::new();
        let session = CaptureSession::new(Box::new(process), None);
        assert_eq!(*session.state(), CaptureState::Idle);
    }

    #[test]
    fn session_start_spawns_process() {
        let (process, spawned, _, _) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(Box::new(process), None);

        session.start().unwrap();

        assert_eq!(*session.state(), CaptureState::Running);
        assert!(*spawned.borrow());
    }

    #[test]
    fn session_stop_requests_process_stop() {
        let (process, _, stop_requested, _) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(Box::new(process), None);

        session.start().unwrap();
        session.stop().unwrap();

        assert_eq!(*session.state(), CaptureState::Stopped);
        assert!(*stop_requested.borrow());
    }

    #[test]
    fn cannot_start_twice() {
        let (process, _, _, _) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(Box::new(process), None);

        session.start().unwrap();
        assert!(session.start().is_err());
    }

    #[test]
    fn cannot_stop_when_not_running() {
        let (process, _, _, _) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(Box::new(process), None);

        assert!(session.stop().is_err());
    }

    #[test]
    fn duration_not_expired_without_duration() {
        let (process, _, _, _) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(Box::new(process), None);

        session.start().unwrap();
        assert!(!session.is_duration_expired());
    }

    #[test]
    fn duration_not_expired_before_time() {
        let (process, _, _, _) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(Box::new(process), Some(Duration::from_secs(10)));

        session.start().unwrap();
        assert!(!session.is_duration_expired());
    }

    #[test]
    fn check_and_stop_if_expired_returns_false_when_not_expired() {
        let (process, _, stop_requested, _) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(Box::new(process), Some(Duration::from_secs(10)));

        session.start().unwrap();
        let expired = session.check_and_stop_if_expired().unwrap();

        assert!(!expired);
        assert!(!*stop_requested.borrow());
    }

    #[test]
    fn check_and_stop_if_expired_stops_when_expired() {
        let (process, _, stop_requested, _) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(
            Box::new(process),
            Some(Duration::from_millis(1)), // Very short duration
        );

        session.start().unwrap();
        thread::sleep(Duration::from_millis(10)); // Wait for duration to expire

        let expired = session.check_and_stop_if_expired().unwrap();

        assert!(expired);
        assert_eq!(*session.state(), CaptureState::Stopped);
        // Note: process is NOT asked to stop - ffmpeg exits naturally with -t
        assert!(!*stop_requested.borrow());
    }

    #[test]
    fn capture_loop_can_detect_an_early_process_exit() {
        let (process, _, _, running) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(Box::new(process), None);

        session.start().unwrap();
        *running.borrow_mut() = false;

        assert!(session.has_exited());
    }

    #[test]
    fn finish_succeeds_on_zero_exit() {
        let (process, _, _, _) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(Box::new(process), None);

        session.start().unwrap();
        session.finish().unwrap();

        assert_eq!(*session.state(), CaptureState::Stopped);
    }

    #[test]
    fn finish_accepts_the_exit_code_from_an_interrupted_capture() {
        let (process, _, _, _) = MockFfmpegProcess::with_exit_code(Some(130));
        let mut session = CaptureSession::new(Box::new(process), None);

        session.start().unwrap();
        session.stop().unwrap();

        session
            .finish()
            .expect("a user-requested interrupt should finish cleanly");
        assert_eq!(*session.state(), CaptureState::Stopped);
    }

    #[test]
    fn elapsed_is_zero_before_start() {
        // A fresh session has no recorded start time; elapsed() must report
        // zero rather than panicking so the status renderer can run before the
        // session is running.
        let (process, _, _, _) = MockFfmpegProcess::new();
        let session = CaptureSession::new(Box::new(process), None);
        assert_eq!(session.elapsed(), Duration::ZERO);
    }

    #[test]
    fn elapsed_grows_after_start() {
        let (process, _, _, _) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(Box::new(process), None);
        session.start().unwrap();
        // After starting, elapsed must be at least zero and monotonic.
        let first = session.elapsed();
        thread::sleep(Duration::from_millis(5));
        let second = session.elapsed();
        assert!(second >= first, "elapsed should not go backwards");
    }

    #[test]
    fn duration_returns_configured_duration() {
        let (process, _, _, _) = MockFfmpegProcess::new();
        let session = CaptureSession::new(Box::new(process), Some(Duration::from_secs(30)));
        assert_eq!(session.duration(), Some(Duration::from_secs(30)));
    }

    #[test]
    fn duration_returns_none_when_unset() {
        let (process, _, _, _) = MockFfmpegProcess::new();
        let session = CaptureSession::new(Box::new(process), None);
        assert_eq!(session.duration(), None);
    }
}
