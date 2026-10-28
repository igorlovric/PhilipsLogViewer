use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum MatchMode {
    /// Event ID and description (digits ignored) must both match
    IdAndDescription,
    IdOnly,
    DescriptionOnly,
}

impl MatchMode {
    pub fn label(self) -> &'static str {
        match self {
            MatchMode::IdAndDescription => "Event ID + description",
            MatchMode::IdOnly => "Event ID only",
            MatchMode::DescriptionOnly => "Description only",
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PatternStep {
    pub event_id: String,
    pub description: String,
    pub category: String,
    pub unit: String,
    /// offset from the first step in ms
    pub offset_ms: i64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Pattern {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub comment: String,
    pub color: [u8; 3],
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub source_file: String,
    pub steps: Vec<PatternStep>,
    /// allowed deviation of time offsets, in percent of the offset
    pub time_tolerance_pct: f32,
    /// minimal time window (ms) around each expected step
    pub min_slack_ms: i64,
    /// occurrences below this similarity (%) are ignored
    pub min_similarity: f32,
    pub match_mode: MatchMode,
}

impl Pattern {
    pub fn defaults_from(steps: Vec<PatternStep>, source_file: String, created: String, id: u64) -> Self {
        Pattern {
            id,
            name: String::new(),
            description: String::new(),
            comment: String::new(),
            color: [255, 153, 51],
            created,
            source_file,
            steps,
            time_tolerance_pct: 50.0,
            min_slack_ms: 2000,
            min_similarity: 75.0,
            match_mode: MatchMode::IdAndDescription,
        }
    }
    pub fn span_ms(&self) -> i64 {
        self.steps.last().map(|s| s.offset_ms).unwrap_or(0)
    }
}

#[derive(Clone, Debug)]
pub struct Occurrence {
    /// event index per pattern step (None = step not found)
    pub events: Vec<Option<u32>>,
    pub start_ts: i64,
    pub similarity: f32,
    pub found: usize,
}

#[derive(Clone, Debug)]
pub struct PatternResult {
    pub pattern_id: u64,
    pub occurrences: Vec<Occurrence>,
}

pub fn steps_from_selection(log: &LogData, selected: &[u32]) -> Vec<PatternStep> {
    let mut sel: Vec<u32> = selected.to_vec();
    sel.sort_by_key(|&i| (log.events[i as usize].ts, i));
    let t0 = sel.first().map(|&i| log.events[i as usize].ts).unwrap_or(0);
    sel.iter()
        .map(|&i| {
            let e = &log.events[i as usize];
            PatternStep {
                event_id: log.pool.get(e.event_id).to_owned(),
                description: log.pool.get(e.description).to_owned(),
                category: log.pool.get(e.category).to_owned(),
                unit: log.pool.get(e.unit).to_owned(),
                offset_ms: e.ts - t0,
            }
        })
        .collect()
}

/// Finds approximate occurrences of a pattern in the log.
///
/// Every step must match by key (event id / description); the timing between
/// steps may deviate by `time_tolerance_pct` (at least `min_slack_ms`), and the
/// order may differ slightly (inversions lower the similarity score).
pub fn match_pattern(p: &Pattern, log: &LogData) -> Vec<Occurrence> {
    let n = p.steps.len();
    if n == 0 {
        return vec![];
    }
    // resolve step keys to pool symbols
    let keys: Vec<(Option<Sym>, Option<Sym>)> = p
        .steps
        .iter()
        .map(|s| (log.pool.find(&s.event_id), log.pool.find(&normalize_desc(&s.description))))
        .collect();

    // candidate positions (in by_time order) per step
    let mut cands: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (pos, &ei) in log.by_time.iter().enumerate() {
        let e = &log.events[ei as usize];
        for (s, (id, ds)) in keys.iter().enumerate() {
            let ok = match p.match_mode {
                MatchMode::IdAndDescription => *id == Some(e.event_id) && *ds == Some(e.desc_norm),
                MatchMode::IdOnly => *id == Some(e.event_id),
                MatchMode::DescriptionOnly => *ds == Some(e.desc_norm),
            };
            if ok {
                cands[s].push(pos as u32);
            }
        }
    }
    let possible = cands.iter().filter(|c| !c.is_empty()).count();
    if (possible as f32 / n as f32) * 100.0 < p.min_similarity {
        return vec![];
    }
    // anchor on the rarest step
    let r = (0..n)
        .filter(|&i| !cands[i].is_empty())
        .min_by_key(|&i| cands[i].len())
        .unwrap();
    let ts_at = |pos: u32| log.events[log.by_time[pos as usize] as usize].ts;
    let off: Vec<i64> = p.steps.iter().map(|s| s.offset_ms).collect();
    let pct = (p.time_tolerance_pct.max(0.0) / 100.0) as f64;

    let mut found_all: Vec<Occurrence> = Vec::new();
    let mut picked: Vec<Option<u32>> = vec![None; n];
    let mut score_t: Vec<f32> = vec![0.0; n];
    for &a in &cands[r] {
        let base = ts_at(a) - off[r];
        picked.iter_mut().for_each(|x| *x = None);
        picked[r] = Some(a);
        score_t[r] = 1.0;
        for i in 0..n {
            if i == r || cands[i].is_empty() {
                continue;
            }
            let expected = base + off[i];
            let tol = ((off[i] - off[r]).abs() as f64 * pct) as i64;
            let tol = tol.max(p.min_slack_ms).max(1);
            let list = &cands[i];
            let start = list.partition_point(|&q| ts_at(q) < expected - tol);
            let mut best: Option<(u32, i64)> = None;
            for &q in &list[start..] {
                let t = ts_at(q);
                if t > expected + tol {
                    break;
                }
                if picked.contains(&Some(q)) {
                    continue;
                }
                let d = (t - expected).abs();
                if best.map_or(true, |(_, bd)| d < bd) {
                    best = Some((q, d));
                }
            }
            if let Some((q, d)) = best {
                picked[i] = Some(q);
                score_t[i] = 1.0 - (d as f32 / tol as f32).min(1.0);
            }
        }
        let found = picked.iter().filter(|x| x.is_some()).count();
        let ratio = found as f32 / n as f32;
        // order score: share of step pairs that keep the original order
        let fp: Vec<u32> = picked.iter().flatten().copied().collect();
        let mut pairs = 0u32;
        let mut inv = 0u32;
        for i in 0..fp.len() {
            for j in i + 1..fp.len() {
                pairs += 1;
                if fp[i] > fp[j] {
                    inv += 1;
                }
            }
        }
        let order = if pairs == 0 { 1.0 } else { 1.0 - inv as f32 / pairs as f32 };
        let timing: f32 = {
            let v: Vec<f32> = (0..n).filter(|&i| picked[i].is_some()).map(|i| score_t[i]).collect();
            v.iter().sum::<f32>() / v.len().max(1) as f32
        };
        let sim = 100.0 * ratio * (0.7 + 0.3 * order) * (0.85 + 0.15 * timing);
        if sim + 0.001 >= p.min_similarity {
            let first = fp.iter().min().copied().unwrap_or(a);
            found_all.push(Occurrence {
                events: picked.iter().map(|o| o.map(|pos| log.by_time[pos as usize])).collect(),
                start_ts: ts_at(first),
                similarity: sim.min(100.0),
                found,
            });
        }
    }
    // remove overlapping occurrences, keep the best ones
    found_all.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap().then(a.start_ts.cmp(&b.start_ts)));
    let mut used: HashSet<u32> = HashSet::new();
    let mut out = Vec::new();
    for o in found_all {
        if o.events.iter().flatten().any(|e| used.contains(e)) {
            continue;
        }
        used.extend(o.events.iter().flatten().copied());
        out.push(o);
    }
    out.sort_by_key(|o| o.start_ts);
    out
}

pub fn match_all(patterns: &[Pattern], log: &LogData) -> Vec<PatternResult> {
    patterns
        .iter()
        .map(|p| PatternResult { pattern_id: p.id, occurrences: match_pattern(p, log) })
        .filter(|r| !r.occurrences.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    #[test]
    fn parse_and_match_real_log() {
        let Ok(path) = std::env::var("LOGFILE") else { return };
        let t = std::time::Instant::now();
        let log = parser::load(std::path::Path::new(&path), parser::Progress::new()).unwrap();
        println!("parsed {} events in {:?} (counts {:?})", log.events.len(), t.elapsed(), log.counts);
        println!("range {} -> {}", fmt_ts(log.t_min), fmt_ts(log.t_max));
        println!("mode segments: {}", log.mode_segments.len());
        // build a pattern from the first error and the 4 events following it in time
        let first_err = log.by_time.iter().position(|&i| log.events[i as usize].severity == Severity::Error).unwrap();
        let sel: Vec<u32> = log.by_time[first_err..first_err + 5].to_vec();
        let mut p = Pattern::defaults_from(steps_from_selection(&log, &sel), "x".into(), "now".into(), 1);
        p.name = "test".into();
        for s in &p.steps {
            println!("  step +{}ms {} {}", s.offset_ms, s.event_id, s.description);
        }
        let t = std::time::Instant::now();
        let occ = match_pattern(&p, &log);
        println!("matched {} occurrences in {:?}", occ.len(), t.elapsed());
        for o in &occ {
            println!("  {} sim {:.1}% found {}", fmt_ts(o.start_ts), o.similarity, o.found);
        }
        assert!(occ.iter().any(|o| o.similarity > 99.0));
        // startup-like pattern: pick events around a "Startup" mode switch
        let st = log.pool.find("Startup").unwrap();
        let pos = log.by_time.iter().position(|&i| log.events[i as usize].system_mode == st).unwrap();
        let sel: Vec<u32> = log.by_time[pos..pos + 8].to_vec();
        let mut p2 = Pattern::defaults_from(steps_from_selection(&log, &sel), "x".into(), "now".into(), 2);
        p2.min_similarity = 70.0;
        let occ = match_pattern(&p2, &log);
        println!("startup pattern: {} occurrences", occ.len());
        for o in &occ {
            println!("  {} sim {:.1}% found {}/{}", fmt_ts(o.start_ts), o.similarity, o.found, p2.steps.len());
        }
    }
}
