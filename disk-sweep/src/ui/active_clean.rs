use std::sync::mpsc::{self, Receiver};
use std::thread;

use anyhow::Result;

use crate::clean::{clean_items_with_progress, CleanReport, CleanUpdate};
use crate::scan::ScanItem;

use super::progress::CleanProgressView;

pub enum CleanPoll {
    Running,
    Done,
}

pub struct ActiveClean {
    rx: Receiver<CleanUpdate>,
    handle: thread::JoinHandle<Result<CleanReport>>,
}

impl ActiveClean {
    pub fn start(items: Vec<ScanItem>) -> Self {
        let (tx, rx) = mpsc::sync_channel::<CleanUpdate>(64);
        let handle = thread::spawn(move || clean_items_with_progress(&items, false, &tx));
        Self { rx, handle }
    }

    pub fn poll(&mut self, progress: &mut CleanProgressView) -> CleanPoll {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                CleanUpdate::Progress {
                    current,
                    total,
                    path,
                } => {
                    progress.current = current;
                    progress.total = total;
                    progress.path = path;
                }
                CleanUpdate::Log(line) => {
                    progress.log.push(line);
                    if progress.log.len() > 6 {
                        progress.log.remove(0);
                    }
                }
            }
        }
        if self.handle.is_finished() {
            CleanPoll::Done
        } else {
            CleanPoll::Running
        }
    }

    pub fn finish(self) -> Result<CleanReport> {
        self.handle
            .join()
            .map_err(|_| anyhow::anyhow!("clean thread panicked"))?
    }
}
