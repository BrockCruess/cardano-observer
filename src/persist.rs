//! Disk persistence: events and the tx-detail cache survive restarts.
//!
//! Format is append-only JSONL (one JSON document per line) - trivially
//! inspectable, crash-tolerant (a torn final line is just skipped on load),
//! and fast enough for chain event rates. Tx cache files are compacted in
//! place once they grow past a multiple of their retention cap. Event history
//! is never compacted so the UI can scroll back to the first event on record.

use crate::model::ChainEvent;
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Compact once a file holds this many times its retention cap.
const COMPACT_FACTOR: usize = 4;

pub struct Persister {
    events: Mutex<LineFile>,
    txs: Mutex<LineFile>,
}

struct LineFile {
    path: PathBuf,
    writer: BufWriter<File>,
    lines: usize,
    /// 0 = never compact (full history).
    cap: usize,
}

impl Persister {
    pub fn open(dir: &Path, tx_cap: usize) -> Result<Persister> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("cannot create data dir {}", dir.display()))?;
        Ok(Persister {
            // Events: retain full history for infinite scroll. The in-memory
            // ring is time-bounded separately (see EVENT_RETENTION_HOURS).
            events: Mutex::new(LineFile::open(dir.join("events.jsonl"), 0)?),
            txs: Mutex::new(LineFile::open(dir.join("txs.jsonl"), tx_cap)?),
        })
    }

    /// Load events with `timestamp >= cutoff` (oldest→newest) for the memory ring.
    pub fn load_events_since(&self, cutoff: i64) -> Vec<ChainEvent> {
        let mut out = Vec::new();
        self.for_each_event_since(cutoff, |ev| out.push(ev.clone()));
        out
    }

    /// Events with `id < before_id`, newest page among those older events,
    /// returned oldest→newest. `exhausted` is true when nothing older remains.
    pub fn events_before(&self, before_id: u64, limit: usize) -> (Vec<ChainEvent>, bool) {
        let limit = limit.clamp(1, 500);
        let f = self.events.lock().unwrap();
        let Ok(file) = File::open(&f.path) else {
            return (Vec::new(), true);
        };
        let mut ring: VecDeque<ChainEvent> = VecDeque::with_capacity(limit + 1);
        let mut overflowed = false;
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(ev) = serde_json::from_str::<ChainEvent>(&line) else {
                continue;
            };
            if ev.id >= before_id {
                continue;
            }
            ring.push_back(ev);
            if ring.len() > limit {
                ring.pop_front();
                overflowed = true;
            }
        }
        (ring.into_iter().collect(), !overflowed)
    }

    /// Restore the newest cached txs as (hash, {"tx":…,"block":…}) pairs.
    pub fn load_txs(&self) -> Vec<(String, Value)> {
        let f = self.txs.lock().unwrap();
        read_tail(&f.path, f.cap)
            .into_iter()
            .filter_map(|line| {
                let v: Value = serde_json::from_str(&line).ok()?;
                let hash = v.get("hash")?.as_str()?.to_string();
                let entry = v.get("entry")?.clone();
                Some((hash, entry))
            })
            .collect()
    }

    /// Stream events with `timestamp >= cutoff` (oldest first). Used to seed
    /// the trending window without loading the entire history into memory.
    pub fn for_each_event_since(&self, cutoff: i64, mut f: impl FnMut(&ChainEvent)) {
        let path = self.events.lock().unwrap().path.clone();
        let Ok(file) = File::open(&path) else {
            return;
        };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(ev) = serde_json::from_str::<ChainEvent>(&line) else {
                continue;
            };
            if ev.timestamp >= cutoff {
                f(&ev);
            }
        }
    }

    pub fn append_event(&self, event: &ChainEvent) {
        if let Ok(line) = serde_json::to_string(event) {
            self.events.lock().unwrap().append(&line);
        }
    }

    pub fn append_tx(&self, hash: &str, entry: &Value) {
        let line = serde_json::json!({ "hash": hash, "entry": entry }).to_string();
        self.txs.lock().unwrap().append(&line);
    }
}

impl LineFile {
    fn open(path: PathBuf, cap: usize) -> Result<LineFile> {
        let lines = count_lines(&path);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("cannot open {}", path.display()))?;
        let mut lf = LineFile {
            path,
            writer: BufWriter::new(file),
            lines,
            cap,
        };
        if lf.cap > 0 && lf.lines > lf.cap * COMPACT_FACTOR {
            lf.compact();
        }
        Ok(lf)
    }

    fn append(&mut self, line: &str) {
        if let Err(e) = writeln!(self.writer, "{line}").and_then(|_| self.writer.flush()) {
            tracing::warn!("persist write failed ({}): {e}", self.path.display());
            return;
        }
        self.lines += 1;
        if self.cap > 0 && self.lines > self.cap * COMPACT_FACTOR {
            self.compact();
        }
    }

    /// Keep only the newest `cap` lines: write tmp, rename over, reopen.
    fn compact(&mut self) {
        if self.cap == 0 {
            return;
        }
        let kept = read_tail(&self.path, self.cap);
        let tmp = self.path.with_extension("jsonl.tmp");
        let result = (|| -> Result<()> {
            let mut w = BufWriter::new(File::create(&tmp)?);
            for line in &kept {
                writeln!(w, "{line}")?;
            }
            w.flush()?;
            std::fs::rename(&tmp, &self.path)?;
            let file = OpenOptions::new().create(true).append(true).open(&self.path)?;
            self.writer = BufWriter::new(file);
            self.lines = kept.len();
            Ok(())
        })();
        if let Err(e) = result {
            tracing::warn!("compaction of {} failed: {e:#}", self.path.display());
        } else {
            tracing::debug!("compacted {} to {} lines", self.path.display(), kept.len());
        }
    }
}

fn count_lines(path: &Path) -> usize {
    File::open(path)
        .map(|f| BufReader::new(f).lines().count())
        .unwrap_or(0)
}

/// Last `cap` complete lines of a file (oldest first). Reads the whole file;
/// compaction keeps tx files small enough that this stays cheap.
fn read_tail(path: &Path, cap: usize) -> Vec<String> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let lines: Vec<String> = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|l| !l.trim().is_empty())
        .collect();
    let start = lines.len().saturating_sub(cap);
    lines[start..].to_vec()
}
