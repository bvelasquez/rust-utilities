use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;

use airouter_core::{SseUsageParser, TokenUsage};
use bytes::Bytes;
use futures::Stream;

use crate::Finish;

pub struct Observe<S> {
    inner: S,
    parser: SseUsageParser,
    usage: TokenUsage,
    start: Instant,
    ttft_ms: Option<u64>,
    saw_done: bool,
    error: Option<String>,
    on_finish: Option<Box<dyn FnOnce(Finish) + Send>>,
}

impl<S> Observe<S> {
    pub fn new(inner: S, on_finish: Box<dyn FnOnce(Finish) + Send>) -> Self {
        Self {
            inner,
            parser: SseUsageParser::default(),
            usage: TokenUsage::default(),
            start: Instant::now(),
            ttft_ms: None,
            saw_done: false,
            error: None,
            on_finish: Some(on_finish),
        }
    }

    fn finish(&mut self) {
        if let Some(cb) = self.on_finish.take() {
            let status = if self.error.is_some() {
                502
            } else if self.saw_done {
                200
            } else {
                499
            };
            cb(Finish {
                usage: self.usage.clone(),
                ttft_ms: self.ttft_ms,
                status,
                error: self.error.clone(),
                latency_ms: self.start.elapsed().as_millis() as u64,
            });
        }
    }
}

impl<S> Drop for Observe<S> {
    fn drop(&mut self) {
        self.finish();
    }
}

impl<S> Stream for Observe<S>
where
    S: Stream<Item = Result<Bytes, String>> + Unpin,
{
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                let text = String::from_utf8_lossy(&bytes);
                if text.contains("[DONE]") {
                    this.saw_done = true;
                }
                let saw = this.parser.push(&text, &mut this.usage);
                if saw && this.ttft_ms.is_none() {
                    this.ttft_ms = Some(this.start.elapsed().as_millis() as u64);
                }
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(Err(err))) => {
                this.error = Some(err);
                this.finish();
                Poll::Ready(None)
            }
            Poll::Ready(None) => {
                this.finish();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
