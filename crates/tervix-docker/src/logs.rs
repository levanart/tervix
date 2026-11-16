use crate::engine::{Chunk, Engine, text};
use arc_swap::ArcSwap;
use futures_util::{FutureExt, StreamExt};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tervix_application::docker::*;
use tokio_util::sync::CancellationToken;

const MAX_ENTRIES: usize = 1_000;
const MAX_BYTES: usize = 512 * 1024;
const MAX_ENTRY: usize = 8 * 1024;

pub(crate) struct Source {
    frame: Arc<ArcSwap<LogSnapshot>>,
    readers: Arc<AtomicUsize>,
    pub task: tokio::task::JoinHandle<()>,
}

impl Source {
    pub fn start(engine: Arc<dyn Engine>, id: ContainerId, cancel: CancellationToken) -> Self {
        let initial = Arc::new(LogSnapshot {
            revision: 0,
            state: LogState::Following,
            entries: Arc::from([]),
            dropped_entries: 0,
            truncated_chunks: 0,
        });
        let frame = Arc::new(ArcSwap::from(initial));
        let output = frame.clone();
        let readers = Arc::new(AtomicUsize::new(0));
        let demand = readers.clone();
        let task = tokio::spawn(async move {
            let mut stream = engine.logs(&id);
            let mut ring = Ring::default();
            let mut tick = tokio::time::interval(Duration::from_millis(50));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            // Allow the actor to install the first reader before checking demand.
            tick.tick().await;
            let state = loop {
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => break LogState::Cancelled,
                    _ = tick.tick() => {
                        if demand.load(Ordering::Acquire) == 0 { break LogState::Cancelled; }
                        if ring.dirty { ring.publish(&output, LogState::Following); }
                    }
                    chunk = std::panic::AssertUnwindSafe(stream.next()).catch_unwind() => match chunk {
                        Ok(Some(Ok(chunk))) => {
                            ring.push(chunk);
                            // A buffered high-volume stream must not starve control/list tasks.
                            tokio::task::yield_now().await;
                        },
                        Ok(Some(Err(error))) => break LogState::Failed(error),
                        Ok(None) => break LogState::Ended,
                        Err(_) => break LogState::Failed(DockerError::Protocol),
                    }
                }
            };
            ring.publish(&output, state);
        });
        Self {
            frame,
            readers,
            task,
        }
    }

    pub fn reader(&self) -> Box<dyn LogReader> {
        Box::new(Reader::new(self.frame.clone(), self.readers.clone()))
    }
}

struct Reader {
    frame: Arc<ArcSwap<LogSnapshot>>,
    readers: Arc<AtomicUsize>,
    paused: bool,
    current: Arc<LogSnapshot>,
}

impl Reader {
    fn new(frame: Arc<ArcSwap<LogSnapshot>>, readers: Arc<AtomicUsize>) -> Self {
        readers.fetch_add(1, Ordering::AcqRel);
        let current = frame.load_full();
        Self {
            frame,
            readers,
            paused: false,
            current,
        }
    }
}

impl LogReader for Reader {
    fn latest(&mut self) -> Arc<LogSnapshot> {
        if !self.paused {
            self.current = self.frame.load_full();
        }
        self.current.clone()
    }

    fn set_paused(&mut self, paused: bool) {
        if paused && !self.paused {
            self.latest();
        }
        self.paused = paused;
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.readers.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Default)]
struct Ring {
    entries: VecDeque<LogEntry>,
    bytes: usize,
    sequence: u64,
    revision: u64,
    dropped: u64,
    truncated: u64,
    dirty: bool,
}

impl Ring {
    fn push(&mut self, chunk: Chunk) {
        // Bound work as well as retained memory for a single malicious Engine frame.
        // Upstream transport has already decoded the frame; see documented limitation.
        let budget = chunk.bytes.len().min(64 * 1024);
        let mut chunk_truncated = budget < chunk.bytes.len();
        let limited = &chunk.bytes[..budget];
        for (index, line) in limited.split(|b| *b == b'\n').enumerate() {
            if line.is_empty() {
                continue;
            }
            if index >= 256 {
                chunk_truncated = true;
                break;
            }
            let truncated = line.len() > MAX_ENTRY || budget < chunk.bytes.len();
            chunk_truncated |= truncated;
            let raw = String::from_utf8_lossy(&line[..line.len().min(MAX_ENTRY)]);
            let (timestamp, message) = match raw.split_once(' ') {
                Some((stamp, message))
                    if stamp.len() >= 20
                        && stamp.len() <= 64
                        && stamp.is_ascii()
                        && stamp.contains('T')
                        && stamp.ends_with('Z') =>
                {
                    (Some(stamp.to_owned()), message)
                }
                _ => (None, raw.as_ref()),
            };
            let text = text(message, MAX_ENTRY);
            self.sequence = self.sequence.saturating_add(1);
            self.bytes += text.len();
            self.entries.push_back(LogEntry {
                sequence: self.sequence,
                channel: chunk.channel,
                timestamp,
                text,
                truncated,
            });
            while self.entries.len() > MAX_ENTRIES || self.bytes > MAX_BYTES {
                if let Some(old) = self.entries.pop_front() {
                    self.bytes -= old.text.len();
                    self.dropped = self.dropped.saturating_add(1);
                }
            }
        }
        if chunk_truncated {
            self.truncated = self.truncated.saturating_add(1);
        }
        self.dirty = true;
    }

    fn publish(&mut self, output: &ArcSwap<LogSnapshot>, state: LogState) {
        self.revision = self.revision.saturating_add(1);
        output.store(Arc::new(LogSnapshot {
            revision: self.revision,
            state,
            entries: self.entries.iter().cloned().collect(),
            dropped_entries: self.dropped,
            truncated_chunks: self.truncated,
        }));
        self.dirty = false;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Assertion on nonempty test frame.
mod tests {
    use super::*;

    #[test]
    fn ring_limits_channels_timestamps_truncation_and_pause() {
        let mut ring = Ring::default();
        for _ in 0..1200 {
            ring.push(Chunk {
                channel: LogChannel::Stderr,
                bytes: b"2026-11-09T00:00:00.000000000Z hello\x1b\x07\n".to_vec(),
            });
        }
        assert_eq!(ring.entries.len(), 1000);
        assert_eq!(ring.dropped, 200);
        let tx = Arc::new(ArcSwap::from(Arc::new(LogSnapshot {
            revision: 0,
            state: LogState::Following,
            entries: Arc::from([]),
            dropped_entries: 0,
            truncated_chunks: 0,
        })));
        ring.publish(&tx, LogState::Following);
        let demand = Arc::new(AtomicUsize::new(0));
        let mut a = Reader::new(tx.clone(), demand.clone());
        let mut b = Reader::new(tx.clone(), demand);
        a.set_paused(true);
        ring.push(Chunk {
            channel: LogChannel::Stdout,
            bytes: vec![b'x'; MAX_ENTRY * 2],
        });
        ring.publish(&tx, LogState::Ended);
        assert_eq!(a.latest().revision, 1);
        let last = b.latest();
        assert_eq!(last.state, LogState::Ended);
        assert!(last.entries.last().unwrap().truncated);
        assert_eq!(
            last.entries[0].timestamp.as_deref(),
            Some("2026-11-09T00:00:00.000000000Z")
        );
        assert_eq!(last.entries[0].text, "hello");
        assert!(
            LogFilter {
                channel: Some(LogChannel::Stderr),
                contains: "hello".into()
            }
            .matches(&last.entries[0])
        );
        a.set_paused(false);
        assert_eq!(a.latest().revision, last.revision);
        for _ in 0..1000 {
            ring.push(Chunk {
                channel: LogChannel::Combined,
                bytes: vec![b'x'; MAX_ENTRY],
            });
        }
        assert!(ring.bytes <= MAX_BYTES);
        assert!(ring.entries.len() < MAX_ENTRIES);
    }
}
