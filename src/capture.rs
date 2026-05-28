use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use crate::ffmpeg::{build_capture_command, CaptureConfig};

/// Trait for managing ffmpeg processes. Allows mocking in tests.
pub trait FfmpegProcess {
    fn spawn(&mut self) -> anyhow::Result<()>;
    fn kill(&mut self) -> anyhow::Result<()>;
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

    fn kill(&mut self) -> anyhow::Result<()> {
        if let Some(mut child) = self.child.take() {
            // Graceful: don't error if process already exited
            let _ = child.kill();
            let _ = child.wait();
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
}

impl CaptureSession {
    pub fn new(process: Box<dyn FfmpegProcess>, duration: Option<Duration>) -> Self {
        Self {
            state: CaptureState::Idle,
            process,
            start_time: None,
            duration,
        }
    }

    pub fn start(&mut self) -> anyhow::Result<()> {
        if self.state == CaptureState::Running {
            anyhow::bail!("Capture session already running");
        }

        self.process.spawn()?;
        self.state = CaptureState::Running;
        self.start_time = Some(Instant::now());
        Ok(())
    }

    pub fn stop(&mut self) -> anyhow::Result<()> {
        if self.state != CaptureState::Running {
            anyhow::bail!("No capture session running");
        }

        self.process.kill()?;
        self.state = CaptureState::Stopped;
        Ok(())
    }

    pub fn state(&self) -> &CaptureState {
        &self.state
    }

    pub fn is_duration_expired(&self) -> bool {
        if let (Some(start), Some(duration)) = (self.start_time, self.duration) {
            start.elapsed() >= duration
        } else {
            false
        }
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
    /// Returns an error if ffmpeg exited with a non-zero code.
    pub fn finish(&mut self) -> anyhow::Result<()> {
        let exit_code = self.process.wait_for_exit()?;
        self.state = CaptureState::Stopped;

        if let Some(code) = exit_code {
            if code != 0 {
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
        killed: Rc<RefCell<bool>>,
        running: Rc<RefCell<bool>>,
        exit_code: Option<i32>,
    }

    impl MockFfmpegProcess {
        fn new() -> (Self, Rc<RefCell<bool>>, Rc<RefCell<bool>>, Rc<RefCell<bool>>) {
            let spawned = Rc::new(RefCell::new(false));
            let killed = Rc::new(RefCell::new(false));
            let running = Rc::new(RefCell::new(false));

            let process = Self {
                spawned: spawned.clone(),
                killed: killed.clone(),
                running: running.clone(),
                exit_code: Some(0),
            };

            (process, spawned, killed, running)
        }
    }

    impl FfmpegProcess for MockFfmpegProcess {
        fn spawn(&mut self) -> anyhow::Result<()> {
            *self.spawned.borrow_mut() = true;
            *self.running.borrow_mut() = true;
            Ok(())
        }

        fn kill(&mut self) -> anyhow::Result<()> {
            *self.killed.borrow_mut() = true;
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
    fn session_stop_kills_process() {
        let (process, _, killed, _) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(Box::new(process), None);

        session.start().unwrap();
        session.stop().unwrap();

        assert_eq!(*session.state(), CaptureState::Stopped);
        assert!(*killed.borrow());
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
        let (process, _, killed, _) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(Box::new(process), Some(Duration::from_secs(10)));

        session.start().unwrap();
        let expired = session.check_and_stop_if_expired().unwrap();

        assert!(!expired);
        assert!(!*killed.borrow());
    }

    #[test]
    fn check_and_stop_if_expired_stops_when_expired() {
        let (process, _, killed, _) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(
            Box::new(process),
            Some(Duration::from_millis(1)), // Very short duration
        );

        session.start().unwrap();
        thread::sleep(Duration::from_millis(10)); // Wait for duration to expire

        let expired = session.check_and_stop_if_expired().unwrap();

        assert!(expired);
        assert_eq!(*session.state(), CaptureState::Stopped);
        // Note: process is NOT killed - ffmpeg exits naturally with -t
        assert!(!*killed.borrow());
    }

    #[test]
    fn finish_succeeds_on_zero_exit() {
        let (process, _, _, _) = MockFfmpegProcess::new();
        let mut session = CaptureSession::new(Box::new(process), None);

        session.start().unwrap();
        session.finish().unwrap();

        assert_eq!(*session.state(), CaptureState::Stopped);
    }
}
