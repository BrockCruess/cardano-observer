//! Rolling 24-hour keyword frequency for the trending ticker.
//!
//! Only **subjects** are considered — never action types. That means we read
//! asset tickers/names, pool tickers, and full CIP-20 metadata messages from
//! event payloads, and we ignore kind, title, side, DEX venue, vote, and other
//! action/context fields. A Minswap "Order Fill" therefore contributes the
//! traded token (if any), not "Order Fill" or "Minswap".
//!
//! Display casing is preserved (e.g. `USDCx`); counting is case-insensitive.

use crate::model::ChainEvent;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

const TOP_N: usize = 10;
const MAX_TERM_LEN: usize = 48;
const MAX_PHRASE_LEN: usize = 120;

/// Sync lookups for human labels (CIP-26 tickers, pool tickers).
pub trait KeywordMeta: Send + Sync {
    fn asset_label(&self, unit: &str) -> Option<String>;
    fn pool_label(&self, pool_id: &str) -> Option<String>;
}

#[derive(Clone, Debug)]
struct Hit {
    ts: i64,
    /// Lowercase key used for counting / eviction.
    key: Arc<str>,
}

struct CountEntry {
    count: u32,
    /// Preferred display form (original ticker / message casing).
    display: String,
}

pub struct Trending {
    hits: VecDeque<Hit>,
    counts: HashMap<Arc<str>, CountEntry>,
    window_secs: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TrendTerm {
    pub term: String,
    pub count: u32,
}

impl Trending {
    pub fn new(window_secs: i64) -> Self {
        Trending {
            hits: VecDeque::new(),
            counts: HashMap::new(),
            window_secs: window_secs.max(3600),
        }
    }

    pub fn record(&mut self, ts: i64, terms: &[String]) {
        self.evict(ts);
        for term in terms {
            let key: Arc<str> = Arc::from(term.to_ascii_lowercase().as_str());
            match self.counts.get_mut(&key) {
                Some(entry) => {
                    entry.count += 1;
                    if better_display(term, &entry.display) {
                        entry.display = term.clone();
                    }
                }
                None => {
                    self.counts.insert(
                        Arc::clone(&key),
                        CountEntry {
                            count: 1,
                            display: term.clone(),
                        },
                    );
                }
            }
            self.hits.push_back(Hit { ts, key });
        }
    }

    pub fn record_event(&mut self, event: &ChainEvent, meta: Option<&dyn KeywordMeta>) {
        let terms = extract_keywords(event, meta);
        if !terms.is_empty() {
            self.record(event.timestamp, &terms);
        }
    }

    /// Recompute top-N.
    pub fn top(&mut self, now: i64) -> Vec<TrendTerm> {
        self.evict(now);
        let mut items: Vec<TrendTerm> = self
            .counts
            .values()
            .map(|e| TrendTerm {
                term: e.display.clone(),
                count: e.count,
            })
            .collect();
        items.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.term.to_ascii_lowercase().cmp(&b.term.to_ascii_lowercase()))
        });
        items.truncate(TOP_N);
        items
    }

    pub fn snapshot(&mut self, now: i64) -> Vec<TrendTerm> {
        self.top(now)
    }

    fn evict(&mut self, now: i64) {
        let cutoff = now - self.window_secs;
        while self.hits.front().is_some_and(|h| h.ts < cutoff) {
            if let Some(hit) = self.hits.pop_front() {
                if let Some(c) = self.counts.get_mut(&hit.key) {
                    c.count = c.count.saturating_sub(1);
                    if c.count == 0 {
                        self.counts.remove(&hit.key);
                    }
                }
            }
        }
    }
}

/// Prefer ticker-like casing (more uppercase) when merging case-insensitive hits.
fn better_display(candidate: &str, current: &str) -> bool {
    let score = |s: &str| {
        let upper = s.chars().filter(|c| c.is_ascii_uppercase()).count();
        let lower = s.chars().filter(|c| c.is_ascii_lowercase()).count();
        // Tickers are often ALL CAPS; prefer that, then mixed case over all-lower.
        (upper, upper.saturating_sub(lower), s.len())
    };
    score(candidate) > score(current)
}

/// Extract subject keywords only — never action-type fields (kind, title, side,
/// dex venue, vote, …).
pub fn extract_keywords(event: &ChainEvent, meta: Option<&dyn KeywordMeta>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let d = &event.data;

    push_assets(&mut out, d.get("assets"), meta);
    push_assets(&mut out, d.get("want"), meta);

    for key in ["pool", "fromPool", "issuerPool"] {
        if let Some(id) = d.get(key).and_then(Value::as_str) {
            if let Some(meta) = meta {
                if let Some(label) = meta.pool_label(id) {
                    push_term(&mut out, Some(&format_ticker_display(&label)));
                }
            }
        }
    }

    // CIP-20: full message as one phrase (not split into words).
    if let Some(msg) = d.get("msg").and_then(Value::as_str) {
        push_phrase(&mut out, msg);
    }

    out
}

fn push_assets(out: &mut Vec<String>, assets: Option<&Value>, meta: Option<&dyn KeywordMeta>) {
    let Some(items) = assets
        .and_then(|a| a.get("items"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for a in items {
        // Prefer CIP-26 ticker (proper casing); fall back to on-chain name.
        let mut pushed = false;
        if let Some(unit) = a.get("unit").and_then(Value::as_str) {
            if let Some(meta) = meta {
                if let Some(label) = meta.asset_label(unit) {
                    push_term(out, Some(&format_ticker_display(&label)));
                    pushed = true;
                }
            }
        }
        if !pushed {
            if let Some(name) = a.get("name").and_then(Value::as_str) {
                push_term(out, Some(&format_ticker_display(name)));
            }
        }
    }
}

/// Preserve mixed/upper casing; promote short all-lowercase labels to TICKER form.
fn format_ticker_display(s: &str) -> String {
    let t = s.trim();
    if (2..=10).contains(&t.len())
        && t.chars().all(|c| c.is_ascii_alphanumeric())
        && t.bytes().all(|b| !b.is_ascii_uppercase())
    {
        t.to_ascii_uppercase()
    } else {
        t.to_string()
    }
}

fn push_term(out: &mut Vec<String>, raw: Option<&str>) {
    let Some(raw) = raw else { return };
    let t = raw.trim();
    if t.is_empty() || t.len() > MAX_TERM_LEN || !is_subject_term(t) {
        return;
    }
    push_unique(out, t.to_string());
}

fn push_phrase(out: &mut Vec<String>, raw: &str) {
    let phrase: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let phrase = phrase.trim();
    if phrase.len() < 3 {
        return;
    }
    let clipped = if phrase.chars().count() > MAX_PHRASE_LEN {
        // Prefix-only clip so the term remains a substring of the live message.
        phrase.chars().take(MAX_PHRASE_LEN).collect::<String>()
    } else {
        phrase.to_string()
    };
    if !is_subject_term(&clipped) {
        return;
    }
    push_unique(out, clipped);
}

fn push_unique(out: &mut Vec<String>, term: String) {
    let key = term.to_ascii_lowercase();
    if out.iter().any(|e| e.eq_ignore_ascii_case(&key)) {
        if let Some(existing) = out.iter_mut().find(|e| e.eq_ignore_ascii_case(&key)) {
            if better_display(&term, existing) {
                *existing = term;
            }
        }
        return;
    }
    out.push(term);
}

/// Structural checks only (ids / empty noise). Action types are excluded by
/// never reading those fields in `extract_keywords`, not via a word list.
fn is_subject_term(t: &str) -> bool {
    if t.len() < 2 {
        return false;
    }
    if t.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    // Hex blobs (hashes, policies)
    if t.len() >= 16 && t.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    // Bech32-ish unique ids
    let lower = t.to_ascii_lowercase();
    !(lower.starts_with("addr")
        || lower.starts_with("stake")
        || lower.starts_with("pool1")
        || lower.starts_with("drep")
        || lower.starts_with("asset1"))
}
