//! Disk persistence: events and the tx-detail cache survive restarts.
//!
//! Format is append-only JSONL (one JSON document per line) — trivially
//! inspectable, crash-tolerant (a torn final line is just skipped on load),
//! and fast enough for chain event rates. Event and tx history are never
//! compacted so deep scrollback can still open full transaction details.
//!
//! Tx lookups use an in-memory hash→byte-offset index built at open (and
//! updated on append), so `/api/tx/{hash}` seeks one line instead of scanning
//! the whole file. Only the retention-window bodies are kept hot in AppState.

use crate::model::{BlockRef, ChainEvent};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Compact once a capped file holds this many times its retention cap.
const COMPACT_FACTOR: usize = 4;

pub struct Persister {
    events: Mutex<LineFile>,
    txs: Mutex<TxStore>,
}

/// Plan for replaying Ogmios blocks to fill gaps in `txs.jsonl`.
pub struct TxGapPlan {
    /// Chain points for `findIntersection` (newest first).
    pub points: Vec<BlockRef>,
    /// Tx hashes present in events but missing from the tx index.
    pub missing: HashSet<String>,
    /// Stop backfill after this slot (with a small margin).
    pub max_slot: u64,
}

struct LineFile {
    path: PathBuf,
    writer: BufWriter<File>,
    lines: usize,
    /// 0 = never count-compact (full history).
    cap: usize,
}

/// Append-only tx bodies with a hash→offset index for O(1) point lookups.
struct TxStore {
    path: PathBuf,
    writer: BufWriter<File>,
    /// Byte offset of the next append (file length after last flush).
    next_offset: u64,
    /// Newest line offset per tx hash.
    index: HashMap<String, u64>,
    lines: usize,
}

impl Persister {
    pub fn open(dir: &Path) -> Result<Persister> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("cannot create data dir {}", dir.display()))?;
        let txs = TxStore::open(dir.join("txs.jsonl"))?;
        tracing::info!(
            "tx detail index: {} hashes ({} lines) in {}",
            txs.index.len(),
            txs.lines,
            dir.join("txs.jsonl").display()
        );
        Ok(Persister {
            // Events: full history on disk; memory ring is time-bounded separately.
            events: Mutex::new(LineFile::open(dir.join("events.jsonl"), 0)?),
            txs: Mutex::new(txs),
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

    /// Load txs with `block.timestamp >= cutoff` (oldest→newest) for the hot cache.
    /// Last write for a given hash wins when the file has duplicates.
    pub fn load_txs_since(&self, cutoff: i64) -> Vec<(String, Value)> {
        self.txs.lock().unwrap().load_since(cutoff)
    }

    /// Point lookup by hash (indexed seek). Works for any persisted tx age.
    pub fn find_tx(&self, hash: &str) -> Option<Value> {
        let (path, offset) = {
            let txs = self.txs.lock().unwrap();
            let &offset = txs.index.get(hash)?;
            (txs.path.clone(), offset)
        };
        read_tx_at(&path, offset)
    }

    pub fn has_tx(&self, hash: &str) -> bool {
        self.txs.lock().unwrap().index.contains_key(hash)
    }

    /// Compare `events.jsonl` tx hashes against the tx index. If any event still
    /// on disk lacks a body, return intersection points + the missing set so
    /// Ogmios can replay those blocks and `cache_tx` only (no event republish).
    pub fn tx_gap_plan(&self) -> Option<TxGapPlan> {
        let known: HashSet<String> = self.txs.lock().unwrap().index.keys().cloned().collect();
        let path = self.events.lock().unwrap().path.clone();
        let Ok(file) = File::open(&path) else {
            return None;
        };

        let mut missing: HashSet<String> = HashSet::new();
        let mut min_slot = u64::MAX;
        let mut max_slot = 0u64;
        let mut blocks: Vec<BlockRef> = Vec::new();

        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(ev) = serde_json::from_str::<ChainEvent>(&line) else {
                continue;
            };
            if ev.kind == "block" {
                if let (Some(hash), Some(height)) = (ev.block_hash.clone(), ev.height) {
                    blocks.push(BlockRef {
                        hash,
                        slot: ev.slot,
                        height,
                    });
                }
            }
            let Some(tx_hash) = ev.tx_hash.as_deref() else {
                continue;
            };
            if known.contains(tx_hash) || missing.contains(tx_hash) {
                continue;
            }
            missing.insert(tx_hash.to_string());
            min_slot = min_slot.min(ev.slot);
            max_slot = max_slot.max(ev.slot);
        }

        if missing.is_empty() {
            return None;
        }

        // Prefer intersection just before the oldest missing slot; fall back to
        // any known block points so Ogmios can still find a common ancestor.
        let mut points: Vec<BlockRef> = blocks
            .iter()
            .filter(|b| b.slot <= min_slot)
            .cloned()
            .collect();
        if points.is_empty() {
            points = blocks;
        }
        // Newest-first, cap — findIntersection wants recent candidates first.
        points.sort_by(|a, b| b.slot.cmp(&a.slot));
        points.dedup_by(|a, b| a.hash == b.hash);
        points.truncate(40);

        if points.is_empty() {
            tracing::warn!(
                "tx gap: {} missing bodies but no block points in events.jsonl to resume from",
                missing.len()
            );
            return None;
        }

        tracing::info!(
            "tx gap: {} hashes missing from txs.jsonl (slots {}–{}); will backfill via Ogmios",
            missing.len(),
            min_slot,
            max_slot
        );
        Some(TxGapPlan {
            points,
            missing,
            max_slot,
        })
    }

    /// Stream events with `timestamp >= cutoff` (oldest first).
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
        self.txs.lock().unwrap().append(hash, entry);
    }
}

impl TxStore {
    fn open(path: PathBuf) -> Result<TxStore> {
        let (index, lines, next_offset) = build_tx_index(&path)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("cannot open {}", path.display()))?;
        Ok(TxStore {
            path,
            writer: BufWriter::new(file),
            next_offset,
            index,
            lines,
        })
    }

    fn append(&mut self, hash: &str, entry: &Value) {
        let line = serde_json::json!({ "hash": hash, "entry": entry }).to_string();
        let start = self.next_offset;
        if let Err(e) = writeln!(self.writer, "{line}").and_then(|_| self.writer.flush()) {
            tracing::warn!("persist write failed ({}): {e}", self.path.display());
            return;
        }
        // line + trailing '\n'
        self.next_offset = start + line.len() as u64 + 1;
        self.lines += 1;
        self.index.insert(hash.to_string(), start);
    }

    fn load_since(&self, cutoff: i64) -> Vec<(String, Value)> {
        // Single sequential pass at boot — cheaper than N indexed seeks.
        let Ok(file) = File::open(&self.path) else {
            return Vec::new();
        };
        let mut by_hash: HashMap<String, Value> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(hash) = v.get("hash").and_then(|h| h.as_str()).map(str::to_string) else {
                continue;
            };
            let Some(entry) = v.get("entry").cloned() else {
                continue;
            };
            if entry_timestamp(&entry) < cutoff {
                continue;
            }
            if !by_hash.contains_key(&hash) {
                order.push(hash.clone());
            }
            by_hash.insert(hash, entry);
        }
        order
            .into_iter()
            .filter_map(|h| by_hash.remove(&h).map(|e| (h, e)))
            .collect()
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

    /// Replace file contents with `kept` lines (already serialized).
    fn rewrite(&mut self, kept: &[String]) -> Result<()> {
        let tmp = self.path.with_extension("jsonl.tmp");
        {
            let mut w = BufWriter::new(File::create(&tmp)?);
            for line in kept {
                writeln!(w, "{line}")?;
            }
            w.flush()?;
        }
        std::fs::rename(&tmp, &self.path)?;
        let file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        self.writer = BufWriter::new(file);
        self.lines = kept.len();
        Ok(())
    }

    fn compact(&mut self) {
        if self.cap == 0 {
            return;
        }
        let kept = read_tail(&self.path, self.cap);
        if let Err(e) = self.rewrite(&kept) {
            tracing::warn!("compaction of {} failed: {e:#}", self.path.display());
        } else {
            tracing::debug!("compacted {} to {} lines", self.path.display(), kept.len());
        }
    }
}

/// One sequential pass: hash→offset index + file length. Skips torn trailing lines.
fn build_tx_index(path: &Path) -> Result<(HashMap<String, u64>, usize, u64)> {
    let mut index = HashMap::new();
    let mut lines = 0usize;
    let Ok(file) = File::open(path) else {
        return Ok((index, 0, 0));
    };
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    let mut offset = 0u64;
    loop {
        buf.clear();
        let start = offset;
        let n = match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        offset += n as u64;
        let trim_end = buf
            .iter()
            .rposition(|&b| b != b'\n' && b != b'\r')
            .map(|i| i + 1)
            .unwrap_or(0);
        if trim_end == 0 {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<Value>(&buf[..trim_end]) else {
            continue;
        };
        let Some(hash) = v.get("hash").and_then(|h| h.as_str()) else {
            continue;
        };
        index.insert(hash.to_string(), start);
        lines += 1;
    }
    Ok((index, lines, offset))
}

fn read_tx_at(path: &Path, offset: u64) -> Option<Value> {
    let mut file = File::open(path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut line = String::new();
    BufReader::new(file).read_line(&mut line).ok()?;
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    v.get("entry").cloned()
}

fn entry_timestamp(entry: &Value) -> i64 {
    entry
        .get("block")
        .and_then(|b| b.get("timestamp"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

fn count_lines(path: &Path) -> usize {
    File::open(path)
        .map(|f| BufReader::new(f).lines().count())
        .unwrap_or(0)
}

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
