//! Run control shared between the copy workers and a front end: safe
//! cancel, pause/resume, and a live byte counter (SPEC §11: progress with
//! ETA, pause, resume and safe cancel).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// Cheap to clone; all clones control the same run.
#[derive(Debug, Clone, Default)]
pub struct Flow {
    /// Set to stop: workers finish nothing new, partial files are removed,
    /// and the job can be resumed.
    pub cancel: Arc<AtomicBool>,
    /// While set, workers wait between reads (also inside a large file).
    pub pause: Arc<AtomicBool>,
    /// Bytes read from the source so far in this run (main streams,
    /// including files still being copied and retried attempts).
    pub bytes: Arc<AtomicU64>,
}

const PAUSE_POLL: Duration = Duration::from_millis(100);

impl Flow {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        // A paused worker must wake up to notice the cancel.
        self.pause.store(false, Ordering::Relaxed);
    }

    pub fn paused(&self) -> bool {
        self.pause.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, on: bool) {
        self.pause.store(on, Ordering::Relaxed);
    }

    /// Blocks while paused. Returns `false` if the run was cancelled.
    pub fn proceed(&self) -> bool {
        while self.paused() && !self.cancelled() {
            std::thread::sleep(PAUSE_POLL);
        }
        !self.cancelled()
    }

    pub fn add_bytes(&self, n: u64) {
        self.bytes.fetch_add(n, Ordering::Relaxed);
    }

    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_wakes_a_paused_worker() {
        let f = Flow::default();
        f.set_paused(true);
        let g = f.clone();
        let t = std::thread::spawn(move || g.proceed());
        std::thread::sleep(Duration::from_millis(150));
        f.stop();
        assert!(!t.join().unwrap());
    }

    #[test]
    fn resume_lets_the_worker_go_on() {
        let f = Flow::default();
        f.set_paused(true);
        let g = f.clone();
        let t = std::thread::spawn(move || g.proceed());
        std::thread::sleep(Duration::from_millis(150));
        f.set_paused(false);
        assert!(t.join().unwrap());
        f.add_bytes(5);
        assert_eq!(f.bytes(), 5);
    }
}
