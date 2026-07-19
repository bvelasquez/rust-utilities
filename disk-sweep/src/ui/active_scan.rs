use std::sync::mpsc::{self, Receiver};
use std::thread;

use anyhow::Result;

use crate::scan::{scan_targets_with_progress, ScanProgress, ScanReport};
use crate::targets::default_targets;

use super::progress::ProgressView;

pub enum ScanPoll {
    Running,
    Done(Result<ScanReport>),
}

pub struct ActiveScan {
    rx: Receiver<ScanProgress>,
    handle: Option<thread::JoinHandle<Result<ScanReport>>>,
}

impl ActiveScan {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::sync_channel::<ScanProgress>(64);
        let handle = thread::spawn(move || scan_targets_with_progress(&default_targets(), &tx));
        Self {
            rx,
            handle: Some(handle),
        }
    }

    pub fn poll(&mut self, progress: &mut Option<ProgressView>) -> ScanPoll {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                ScanProgress::Phase {
                    detail,
                    current,
                    total,
                } => {
                    if progress.is_none() {
                        *progress = Some(ProgressView::new(&detail, total));
                    }
                    if let Some(p) = progress.as_mut() {
                        p.phase = "Scanning".into();
                        p.detail = detail;
                        p.current = current;
                        p.total = total;
                    }
                }
                ScanProgress::Log(line) => {
                    if progress.is_none() {
                        *progress = Some(ProgressView::new("Scanning cleanup targets…", 1));
                    }
                    if let Some(p) = progress.as_mut() {
                        p.log.push(line);
                        if p.log.len() > 8 {
                            p.log.remove(0);
                        }
                    }
                }
            }
        }

        let Some(handle) = self.handle.as_ref() else {
            return ScanPoll::Done(Err(anyhow::anyhow!("scan already finished")));
        };

        if handle.is_finished() {
            let handle = self.handle.take().unwrap();
            ScanPoll::Done(
                handle
                    .join()
                    .map_err(|_| anyhow::anyhow!("scan thread panicked"))
                    .and_then(|r| r),
            )
        } else {
            ScanPoll::Running
        }
    }
}
