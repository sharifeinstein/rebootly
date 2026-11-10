//! Slow steps (scans, planning, copying, restoring) run on a worker thread
//! so the window stays responsive; the page polls the result each frame.

use std::sync::mpsc;

use eframe::egui;

pub struct Task<T> {
    rx: Option<mpsc::Receiver<T>>,
    result: Option<T>,
}

impl<T: Send + 'static> Task<T> {
    pub fn spawn(ctx: &egui::Context, f: impl FnOnce() -> T + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
            ctx.request_repaint();
        });
        Task {
            rx: Some(rx),
            result: None,
        }
    }

    /// A finished task (for results that are ready at once).
    pub fn ready(value: T) -> Self {
        Task {
            rx: None,
            result: Some(value),
        }
    }

    /// The result once the worker is done.
    pub fn get(&mut self) -> Option<&mut T> {
        if self.result.is_none()
            && let Some(rx) = &self.rx
        {
            match rx.try_recv() {
                Ok(v) => self.result = Some(v),
                Err(mpsc::TryRecvError::Empty) => return None,
                // The worker panicked: nothing will come.
                Err(mpsc::TryRecvError::Disconnected) => self.rx = None,
            }
        }
        self.result.as_mut()
    }

    pub fn pending(&mut self) -> bool {
        self.get().is_none() && self.rx.is_some()
    }

    pub fn take(&mut self) -> Option<T> {
        self.get();
        self.result.take()
    }
}
