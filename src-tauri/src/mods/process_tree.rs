//! A child process and every process it starts, killed together and at once.
//! Ours.
//!
//! On Windows the child goes into a job object as it starts, and its own
//! children join that job by themselves, so one call ends them all. The old way,
//! `taskkill /T`, is a process of its own: about half a second on an idle
//! machine and seconds on a busy one, and a run being killed holds its place in
//! mods/lut_provider.rs's queue all that time. It stays only for a child the
//! job could not take. Closing the job kills nothing, so a child that ends on
//! its own leaves what it started running, as before.
//!
//! Elsewhere the child alone is killed, as before.

use std::process::{Child, Command};

/// A started child and what it starts.
pub struct Tree {
    pub child: Child,
    #[cfg(windows)]
    job: Option<windows_job::Job>,
}

impl Tree {
    /// Starts `command`. On Windows its job is made at once, before the child
    /// has had time to start anything.
    pub fn spawn(command: &mut Command) -> std::io::Result<Self> {
        let child = command.spawn()?;
        #[cfg(windows)]
        let job = windows_job::Job::holding(&child);
        Ok(Self {
            child,
            #[cfg(windows)]
            job,
        })
    }

    /// The child and all it started, now; returns once the child is gone.
    pub fn kill(&mut self) {
        #[cfg(windows)]
        if !self.job.as_ref().is_some_and(windows_job::Job::terminate) {
            windows_job::taskkill(self.child.id());
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(windows)]
mod windows_job {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject,
    };
    use windows::core::PCWSTR;

    /// `CREATE_NO_WINDOW`.
    pub const NO_WINDOW: u32 = 0x0800_0000;

    /// A job holding one child and whatever it starts. Its handle closes with it.
    pub struct Job(HANDLE);

    impl Job {
        /// None when Windows will not make the job or put the child in it.
        pub fn holding(child: &Child) -> Option<Self> {
            // SAFETY: a new unnamed job with default security, owned by `Job`
            // from here; the child's handle is borrowed for the call only.
            unsafe {
                let job = Self(CreateJobObjectW(None, PCWSTR::null()).ok()?);
                let process = HANDLE(child.as_raw_handle());
                AssignProcessToJobObject(job.0, process).ok()?;
                Some(job)
            }
        }

        /// Whether every process in the job was ended.
        pub fn terminate(&self) -> bool {
            // SAFETY: the job handle is open for as long as `self` lives.
            unsafe { TerminateJobObject(self.0, 1).is_ok() }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle is ours and closed once, here.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// The old way, for a child with no job.
    pub fn taskkill(pid: u32) {
        let system = std::env::var_os("SystemRoot").map_or_else(PathBuf::new, PathBuf::from);
        let _ = Command::new(system.join("System32").join("taskkill.exe"))
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[cfg(windows)]
pub use windows_job::NO_WINDOW;

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    /// A command that runs for half a minute unless killed.
    fn long() -> Command {
        if cfg!(windows) {
            let mut ping = Command::new("ping");
            ping.args(["-n", "30", "127.0.0.1"]);
            ping
        } else {
            let mut sleep = Command::new("sleep");
            sleep.arg("30");
            sleep
        }
    }

    /// How long a kill of a running child takes, once it is gone.
    fn one_kill() -> Duration {
        let mut tree = Tree::spawn(long().stdout(Stdio::null())).expect("started");
        let from = Instant::now();
        tree.kill();
        let took = from.elapsed();
        assert!(tree.child.try_wait().expect("a status").is_some());
        took
    }

    /// `taskkill` took 0.5 s or more, idle or not; a starved test machine can
    /// slow any one try, so the best of three counts.
    #[test]
    fn a_kill_is_over_at_once() {
        let quick = |took: &Duration| *took < Duration::from_millis(300);
        let mut tries = Vec::new();
        while tries.len() < 3 && !tries.iter().any(quick) {
            tries.push(one_kill());
        }
        assert!(tries.iter().any(quick), "the kills took {tries:?}");
    }

    /// A child's child holds the pipe it was given: once the tree is killed,
    /// the pipe ends, so that one went too.
    #[cfg(windows)]
    #[test]
    fn what_the_child_started_goes_with_it() {
        use std::io::Read;

        let mut command = Command::new("cmd");
        command
            .args(["/c", "ping -n 30 127.0.0.1"])
            .stdout(Stdio::piped());
        let mut tree = Tree::spawn(&mut command).expect("started");
        let mut stdout = tree.child.stdout.take().expect("a pipe");
        // Long enough for cmd to start ping.
        std::thread::sleep(Duration::from_millis(500));
        tree.kill();
        let (tell, told) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = stdout.read_to_end(&mut Vec::new());
            let _ = tell.send(());
        });
        assert!(
            told.recv_timeout(Duration::from_secs(5)).is_ok(),
            "ping outlived the kill"
        );
    }
}
