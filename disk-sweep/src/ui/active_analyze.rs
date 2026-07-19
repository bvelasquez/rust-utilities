use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use anyhow::Result;

use crate::analyze::{AnalyzeOptions, AnalyzeReport};
use crate::scan::ScanItem;
use crate::watch_data::ScanUpdate;

use super::progress::ProgressView;

pub enum AnalyzePoll {
    Running,
    Done,
    Cancelled,
    Complete(Vec<ScanItem>),
}

pub struct ActiveAnalyze {
    rx: Receiver<ScanUpdate>,
    cancel: Arc<AtomicBool>,
    finished: bool,
}

impl ActiveAnalyze {
    pub fn start(options: AnalyzeOptions) -> Self {
        let (tx, rx) = mpsc::sync_channel::<ScanUpdate>(64);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_worker = Arc::clone(&cancel);
        std::thread::spawn(move || {
            match crate::analyze::run_analyze_with_progress(&options, &cancel_worker, &tx) {
                Ok(report) => {
                    let _ = tx.send(ScanUpdate::AnalyzeComplete(report.items));
                }
                Err(e) => {
                    if cancel_worker.load(Ordering::Relaxed) {
                        let _ = tx.send(ScanUpdate::Cancelled);
                    } else {
                        let _ = tx.send(ScanUpdate::Failed(format!("{e:#}")));
                    }
                }
            }
        });
        Self {
            rx,
            cancel,
            finished: false,
        }
    }

    pub fn abort(mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.finished = true;
    }

    pub fn poll(&mut self, progress: &mut Option<ProgressView>) -> AnalyzePoll {
        let mut cancelled = false;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                ScanUpdate::AnalyzeComplete(items) => {
                    self.finished = true;
                    return AnalyzePoll::Complete(items);
                }
                ScanUpdate::Cancelled => cancelled = true,
                ScanUpdate::Failed(e) => {
                    if let Some(p) = progress.as_mut() {
                        p.apply(&ScanUpdate::Failed(e));
                    }
                    self.finished = true;
                    return AnalyzePoll::Done;
                }
                ScanUpdate::Phase {
                    phase,
                    detail,
                    current,
                    total,
                } => {
                    if progress.is_none() {
                        *progress = Some(ProgressView::new(&detail, total));
                    }
                    if let Some(p) = progress.as_mut() {
                        p.apply(&ScanUpdate::Phase {
                            phase,
                            detail,
                            current,
                            total,
                        });
                    }
                }
                ScanUpdate::Log(line) => {
                    if progress.is_none() {
                        *progress = Some(ProgressView::new("Analyze", 3));
                    }
                    if let Some(p) = progress.as_mut() {
                        p.apply(&ScanUpdate::Log(line));
                    }
                }
                ScanUpdate::Snapshot(_) => {}
            }
        }

        if cancelled {
            self.finished = true;
            return AnalyzePoll::Cancelled;
        }

        if self.finished {
            return AnalyzePoll::Done;
        }

        AnalyzePoll::Running
    }
}

#[allow(dead_code)]
pub fn run_blocking(options: AnalyzeOptions) -> Result<AnalyzeReport> {
    crate::analyze::run_analyze(&options)
}
