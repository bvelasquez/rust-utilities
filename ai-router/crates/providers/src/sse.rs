#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

/// Splits an arbitrary byte stream into SSE events separated by a blank line.
#[derive(Debug, Default)]
pub struct SseDecoder {
    buf: Vec<u8>,
}

impl SseDecoder {
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();
        loop {
            let Some(pos) = find_separator(&self.buf) else {
                break;
            };
            let (sep_at, sep_len) = pos;
            let raw: Vec<u8> = self.buf.drain(..sep_at).collect();
            self.buf.drain(..sep_len);
            if let Some(ev) = parse_event(&raw) {
                events.push(ev);
            }
        }
        events
    }

    pub fn finish(&mut self) -> Option<SseEvent> {
        if self.buf.iter().all(|b| b.is_ascii_whitespace()) {
            self.buf.clear();
            return None;
        }
        let raw = std::mem::take(&mut self.buf);
        parse_event(&raw)
    }
}

fn find_separator(buf: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i + 1 < buf.len() {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some((i, 2));
        }
        if i + 3 < buf.len()
            && buf[i] == b'\r'
            && buf[i + 1] == b'\n'
            && buf[i + 2] == b'\r'
            && buf[i + 3] == b'\n'
        {
            return Some((i, 4));
        }
        i += 1;
    }
    None
}

fn parse_event(raw: &[u8]) -> Option<SseEvent> {
    let text = String::from_utf8_lossy(raw);
    let mut event = String::new();
    let mut data = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("event:") {
            event = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.trim_start());
        }
    }
    if data.is_empty() {
        None
    } else {
        Some(SseEvent { event, data })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_across_chunks() {
        let mut dec = SseDecoder::default();
        let a = dec.push(b"event: ping\ndata: {\"a\":");
        assert!(a.is_empty());
        let b = dec.push(b"1}\n\nevent: stop\ndata: {}\n\n");
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].event, "ping");
        assert_eq!(b[0].data, "{\"a\":1}");
        assert_eq!(b[1].event, "stop");
    }
}
