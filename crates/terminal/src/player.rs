//! Steps a recording's screen through time. It keeps the screen as it was every so often, and
//! at every mark, so going back starts again from near where it's going rather than from the
//! start.

use std::sync::Arc;
use std::time::Duration;

use crate::cast::{Recording, What, played};
use crate::screen::Screen;

/// How often, in played time, the screen is kept as it was.
pub const KEPT_EVERY: Duration = Duration::from_secs(10);
/// The most screens kept as they were.
pub const MOST_KEPT_SCREENS: usize = 1000;
/// The most cells the kept screens hold together. A cell is held in 20 bytes, so this is
/// 100 MB at most: a thousand screens of 80 by 24 with their alternate screens fit easily, and
/// fifty of the largest screen, 500 by 200 with its alternate screen.
pub const MOST_KEPT_CELLS: usize = 5_000_000;

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
    stops: Vec<Duration>,
    screen: Screen,
    /// How many events the screen has been fed.
    played: usize,
    /// Screens as they were, by the events played; the first is the screen as it starts.
    kept: Vec<Kept>,
    kept_cells: usize,
    /// How often a screen is kept; it grows when too many have been.
    every: Duration,
}

impl Player {
    pub fn new(recording: Arc<Recording>) -> Player {
        let times = played(&recording);
        let stops = recording.marks.iter().map(|mark| mark.at).collect();
        let screen = Screen::new(recording.width, recording.height);
        let first = Kept { events: 0, at: Duration::ZERO, mark: false, screen: screen.clone() };
        Player {
            kept_cells: first.screen.cells_held(),
            recording,
            times,
            stops,
            screen,
            played: 0,
            kept: vec![first],
            every: KEPT_EVERY,
        }
    }

    /// The screen as it is at a played time: every event at or before it played. Going forward
    /// steps on from where the screen is, or from a kept screen further on; going back starts
    /// again from the nearest kept screen before it.
    pub fn screen_at(&mut self, at: Duration) -> &Screen {
        let target = self.times.partition_point(|time| *time <= at);
        // The kept screens start with the first, of no events, so one is always found.
        let nearest = &self.kept[self.kept.partition_point(|kept| kept.events <= target) - 1];
        if target < self.played || nearest.events > self.played {
            self.screen = nearest.screen.clone();
            self.played = nearest.events;
        }
        while self.played < target {
            self.step();
        }
        &self.screen
    }

    /// The next stop after a played time.
    pub fn next_stop(&self, after: Duration) -> Option<Duration> {
        self.stops.get(self.stops.partition_point(|stop| *stop <= after)).copied()
    }

    /// Where the recording stops until the next click: its marks' played times.
    pub fn stops(&self) -> &[Duration] {
        &self.stops
    }

    /// The screen as the recording ends.
    pub fn last(&self) -> Screen {
        let (mut screen, from) = match self.kept.last() {
            Some(kept) if kept.events > self.played => (kept.screen.clone(), kept.events),
            _ => (self.screen.clone(), self.played),
        };
        for event in &self.recording.events[from..] {
            apply(&mut screen, &event.what);
        }
        screen
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
        if mark || at >= latest.at + self.every {
            self.keep(at, mark);
        }
    }

    /// Keeps the screen as it is, first keeping fewer of those kept before if there'd be too
    /// many.
    fn keep(&mut self, at: Duration, mark: bool) {
        let cells = self.screen.cells_held();
        while self.kept.len() >= MOST_KEPT_SCREENS || self.kept_cells + cells > MOST_KEPT_CELLS {
            if !self.thin(true) && !self.thin(false) {
                return;
            }
        }
        self.kept_cells += cells;
        self.kept.push(Kept { events: self.played, at, mark, screen: self.screen.clone() });
    }

    /// Lets every other kept screen go, only those kept by time if `by_time`, and keeps them
    /// half as often from now on. The first is always kept. Says whether any went.
    fn thin(&mut self, by_time: bool) -> bool {
        self.every = self.every.saturating_mul(2);
        let before = self.kept.len();
        let mut second = false;
        self.kept.retain(|kept| {
            if kept.events == 0 || (by_time && kept.mark) {
                return true;
            }
            second = !second;
            second
        });
        self.kept_cells = self.kept.iter().map(|kept| kept.screen.cells_held()).sum();
        self.kept.len() < before
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
