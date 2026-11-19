//! Steps a recording's screen through time. It keeps the screen as it was every so often, and
//! at marks, so going back starts again from near where it's going rather than from the start.
//!
//! A kept screen shares its rows with the screen played until they change, so keeping one costs
//! its rows' places and the rows changed since the last kept. One is kept only once the work
//! done since the last is at least what keeping it costs, so keeping costs no more than playing,
//! and going back plays again no more than that work.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::CantPlay;
use crate::cast::{MOST_LENGTH, Recording, What, played};
use crate::screen::{Cell, ROW_SHARED, Screen};

/// How often, in played time, the screen is kept as it was.
pub const KEPT_EVERY: Duration = Duration::from_secs(10);
/// The most screens kept as they were.
pub const MOST_KEPT_SCREENS: usize = 1000;
/// The most bytes the kept screens hold together, their shared rows counted once.
pub const MOST_KEPT_BYTES: usize = 100_000_000;

/// What a kept screen holds besides its rows, and what each row's place costs.
const KEPT_OVERHEAD: usize = 256;
const ROW_PLACE: usize = size_of::<Arc<[Cell]>>();

/// The screen as it was after a number of events.
struct Kept {
    /// How many events had been played.
    events: usize,
    /// The played time of the last of them.
    at: Duration,
    /// Whether the last of them was a mark.
    mark: bool,
    screen: Screen,
}

/// Plays a recording's screen.
pub struct Player {
    recording: Arc<Recording>,
    /// The time each event is played at.
    times: Vec<Duration>,
    /// Each stop's time, once however many marks share it, and the event of its first mark.
    stops: Vec<Duration>,
    stop_events: Vec<usize>,
    screen: Screen,
    /// How many events the screen has been fed.
    played: usize,
    /// Screens as they were, by the events played; the first is the screen as it starts.
    kept: Vec<Kept>,
    kept_bytes: usize,
    /// How often a screen is kept; it grows when too many have been.
    every: Duration,
}

/// Never set: for playing that isn't to be stopped.
static NEVER: AtomicBool = AtomicBool::new(false);

impl Player {
    pub fn new(recording: Arc<Recording>) -> Player {
        let times = played(&recording);
        let (mut stops, mut stop_events) = (Vec::new(), Vec::new());
        for mark in &recording.marks {
            if stops.last() != Some(&mark.at) {
                stops.push(mark.at);
                stop_events.push(mark.event);
            }
        }
        let screen = Screen::new(recording.width, recording.height);
        let first = Kept { events: 0, at: Duration::ZERO, mark: false, screen: screen.clone() };
        let mut player = Player {
            recording,
            times,
            stops,
            stop_events,
            screen,
            played: 0,
            kept: vec![first],
            kept_bytes: 0,
            every: KEPT_EVERY,
        };
        player.kept_bytes = player.count_kept();
        player
    }

    /// The screen as it is at a played time: every event before it played, and those at it up
    /// to the stop there if there is one, so at a stop the screen is as it was when its mark was
    /// made. Going forward steps on from where the screen is, or from a kept screen further on;
    /// going back starts again from the nearest kept screen before it.
    pub fn screen_at(&mut self, at: Duration) -> &Screen {
        // Playing that's never stopped always gets there.
        let _ = self.screen_at_within(at, &NEVER);
        &self.screen
    }

    /// As `screen_at`, but looking at `stop` between events, and giving nothing if it was set
    /// before the screen was reached. The screen is left where it got to.
    pub fn screen_at_within(&mut self, at: Duration, stop: &AtomicBool) -> Option<&Screen> {
        let mut target = self.times.partition_point(|time| *time <= at);
        if let Ok(stop) = self.stops.binary_search(&at) {
            target = target.min(self.stop_events[stop] + 1);
        }
        // The kept screens start with the first, of no events, so one is always found.
        let nearest = &self.kept[self.kept.partition_point(|kept| kept.events <= target) - 1];
        if target < self.played || nearest.events > self.played {
            self.screen = nearest.screen.clone();
            self.played = nearest.events;
        }
        while self.played < target {
            if stop.load(Ordering::Relaxed) {
                return None;
            }
            self.step();
        }
        Some(&self.screen)
    }

    /// The next stop after a played time.
    pub fn next_stop(&self, after: Duration) -> Option<Duration> {
        self.stops.get(self.stops.partition_point(|stop| *stop <= after)).copied()
    }

    /// Where the recording stops until the next click: its marks' played times, each once.
    pub fn stops(&self) -> &[Duration] {
        &self.stops
    }

    /// The screen as the recording ends, or why it can't be played: it costs too much work.
    pub fn last(&self) -> Result<Screen, CantPlay> {
        // Playing that's never stopped always gets there.
        self.last_within(&NEVER).unwrap_or(Err(CantPlay::TooLarge))
    }

    /// As `last`, but looking at `stop` between events, and giving nothing if it was set.
    pub fn last_within(&self, stop: &AtomicBool) -> Option<Result<Screen, CantPlay>> {
        let (mut screen, from) = match self.kept.last() {
            Some(kept) if kept.events > self.played => (kept.screen.clone(), kept.events),
            _ => (self.screen.clone(), self.played),
        };
        for event in &self.recording.events[from..] {
            if stop.load(Ordering::Relaxed) {
                return None;
            }
            apply(&mut screen, &event.what);
        }
        Some(if screen.too_costly() { Err(CantPlay::TooLarge) } else { Ok(screen) })
    }

    /// Plays the next event, keeping the screen as it is after it if it's due.
    fn step(&mut self) {
        let what = &self.recording.events[self.played].what;
        apply(&mut self.screen, what);
        let at = self.times[self.played];
        self.played += 1;
        let Some(latest) = self.kept.last() else { return };
        if self.played <= latest.events {
            return;
        }
        let mark = matches!(what, What::Mark(_));
        if !mark && at < latest.at.saturating_add(self.every) {
            return;
        }
        // Keeping costs at least sharing each row; the rows changed since are found only once
        // the work done would pay for that.
        let work = self.screen.work().saturating_sub(latest.screen.work());
        if work < ROW_SHARED * self.screen.held_rows().count() as u64 {
            return;
        }
        let (bytes, cost) = self.cost(latest);
        if work >= cost {
            self.keep(at, mark, bytes);
        }
    }

    /// What keeping the screen as it is would cost: the bytes it would hold, its rows' places
    /// and the rows not held by the last kept, and the work of keeping it, sharing each row and
    /// later copying those changed.
    fn cost(&self, latest: &Kept) -> (usize, u64) {
        let held: HashSet<*const Cell> = latest.screen.held_rows().map(|row| row.as_ptr()).collect();
        let (mut bytes, mut work) = (KEPT_OVERHEAD, 0);
        for row in self.screen.held_rows() {
            bytes += ROW_PLACE;
            work += ROW_SHARED;
            if !held.contains(&row.as_ptr()) {
                bytes += size_of_val::<[Cell]>(row);
                work += row.len() as u64;
            }
        }
        (bytes, work)
    }

    /// All the kept screens hold, in bytes, each row counted once however many share it.
    fn count_kept(&self) -> usize {
        let mut rows = HashSet::new();
        let mut bytes = 0;
        for kept in &self.kept {
            bytes += KEPT_OVERHEAD;
            for row in kept.screen.held_rows() {
                bytes += ROW_PLACE;
                if rows.insert(row.as_ptr()) {
                    bytes += size_of_val::<[Cell]>(row);
                }
            }
        }
        bytes
    }

    /// Keeps the screen as it is, first keeping fewer of those kept before if there'd be too
    /// many.
    fn keep(&mut self, at: Duration, mark: bool, cost: usize) {
        while self.kept.len() >= MOST_KEPT_SCREENS || self.kept_bytes + cost > MOST_KEPT_BYTES {
            if !self.thin(true) && !self.thin(false) {
                return;
            }
        }
        self.kept_bytes += cost;
        self.kept.push(Kept { events: self.played, at, mark, screen: self.screen.clone() });
    }

    /// Lets every other kept screen go, only those kept by time if `by_time`, and if any went,
    /// keeps them half as often from now on. The first is always kept. Says whether any went.
    fn thin(&mut self, by_time: bool) -> bool {
        let before = self.kept.len();
        let mut second = false;
        self.kept.retain(|kept| {
            if kept.events == 0 || (by_time && kept.mark) {
                return true;
            }
            second = !second;
            second
        });
        if self.kept.len() == before {
            return false;
        }
        self.every = self.every.saturating_mul(2).min(MOST_LENGTH);
        self.kept_bytes = self.count_kept();
        true
    }
}

/// Plays one event on a screen.
fn apply(screen: &mut Screen, what: &What) {
    match what {
        What::Output(text) => screen.feed(text),
        What::Resize(width, height) => screen.resize(*width, *height),
        What::Mark(_) => {}
    }
}
