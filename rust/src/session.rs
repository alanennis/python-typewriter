//! the clock for this sitting at the typewriter: how long it's been going
//! and how much of that was spent typing, for the words per minute

use std::time::{Duration, Instant};

/// a gap between keys longer than this is a pause (thinking, a cup of
/// tea) and isn't counted as typing time
pub const PAUSE: Duration = Duration::from_secs(60);

/// typing time needed before words per minute means anything
const WPM_AFTER: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct Session {
    pub started: Instant,
    /// time spent typing: the gaps between keys, leaving out pauses
    pub typing: Duration,
    last_key: Option<Instant>,
}

impl Session {
    pub fn new(now: Instant) -> Self {
        Session { started: now, typing: Duration::ZERO, last_key: None }
    }

    /// a key was pressed at now
    pub fn key(&mut self, now: Instant) {
        if let Some(last) = self.last_key {
            let gap = now.saturating_duration_since(last);
            if gap <= PAUSE {
                self.typing += gap;
            }
        }
        self.last_key = Some(now);
    }

    /// words per minute of typing time, once there's enough to go on
    pub fn wpm(&self, words: usize) -> Option<usize> {
        if self.typing < WPM_AFTER {
            return None;
        }
        Some((words as f64 * 60.0 / self.typing.as_secs_f64()).round() as usize)
    }

    /// time since the start in whole minutes, like 12m or 1h05m
    pub fn elapsed(&self, now: Instant) -> String {
        clock(now.saturating_duration_since(self.started))
    }
}

/// a duration in whole minutes (no seconds ticking away): 12m, or 1h05m
/// after an hour
pub fn clock(d: Duration) -> String {
    let minutes = d.as_secs() / 60;
    let (h, m) = (minutes / 60, minutes % 60);
    if h > 0 { format!("{h}h{m:02}m") } else { format!("{m}m") }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: fn(u64) -> Duration = Duration::from_secs;

    #[test]
    fn typing_time_leaves_out_pauses() {
        let t0 = Instant::now();
        let mut s = Session::new(t0);
        s.key(t0 + S(5)); // the first key starts the count
        s.key(t0 + S(15));
        s.key(t0 + S(45));
        s.key(t0 + S(600)); // a long pause, not counted
        s.key(t0 + S(610));
        assert_eq!(s.typing, S(50));
    }

    #[test]
    fn wpm_waits_for_enough_typing() {
        let t0 = Instant::now();
        let mut s = Session::new(t0);
        s.key(t0);
        s.key(t0 + S(20));
        assert_eq!(s.wpm(10), None);
        s.key(t0 + S(60));
        assert_eq!(s.wpm(30), Some(30));
        assert_eq!(s.wpm(0), Some(0));
    }

    #[test]
    fn elapsed_in_whole_minutes() {
        let t0 = Instant::now();
        let s = Session::new(t0);
        assert_eq!(s.elapsed(t0 + S(59)), "0m");
        assert_eq!(s.elapsed(t0 + S(2 * 60 + 7)), "2m");
        assert_eq!(s.elapsed(t0 + S(65 * 60 + 3)), "1h05m");
    }
}
