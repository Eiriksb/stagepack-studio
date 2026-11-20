//! Steps a recording's screen through time. It keeps the screen as it was every so often, and
//! at marks, so going back starts again from near where it's going rather than from the start.
//!
//! A kept screen shares its rows with the screen played until they change, so keeping one costs
//! its rows' places, and later a copy of each row written again. One is kept only once the work
//! done since the last kept is at least that, and at least the spacing between those kept, so
//! keeping costs no more than playing. When too many are kept, every other one goes and the
//! spacing doubles, so those kept stay spread evenly through the work, and going back plays
//! again only a share of it. Keeping has its own limit of work besides.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::CantPlay;
use crate::cast::{Recording, What, played};
use crate::screen::{Cell, MOST_WORK, ROW_SHARED, Screen};

/// How often, in played time, the screen is kept as it was.
pub const KEPT_EVERY: Duration = Duration::from_secs(10);
/// The most screens kept as they were.
pub const MOST_KEPT_SCREENS: usize = 1000;
/// The most bytes the kept screens hold together, their shared rows counted once.
pub const MOST_KEPT_BYTES: usize = 100_000_000;

/// What a kept screen holds besides its rows, and what each row's place costs.
const KEPT_OVERHEAD: usize = 256;
const ROW_PLACE: usize = size_of::<Arc<[Cell]>>() + 8;
/// The work of keeping each row's place: sharing it, letting it go, and counting it when kept
/// screens go.
const PLACE_WORK: u64 = 3 * ROW_SHARED;
/// Work enough that the screen is kept after it whether a stop or ten seconds have come or not,
/// so going back never plays again much more: about fifteen milliseconds of it.
const KEPT_ANYWAY: u64 = 20_000_000;

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
    /// The least work between kept screens; it doubles each time too many have been kept.
    spacing: u64,
    /// The work keeping has done; past the most a screen may do, no more are kept.
    keeping: u64,
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
            spacing: 0,
            keeping: 0,
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
        let target = self.events_at(at);
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

    /// How many events the screen at a played time has been fed, as [`Self::screen_at`] plays
    /// them: every event before it, and those at it up to the stop there if there is one.
    pub fn events_at(&self, at: Duration) -> usize {
        let target = self.times.partition_point(|time| *time <= at);
        match self.stops.binary_search(&at) {
            Ok(stop) => target.min(self.stop_events[stop] + 1),
            Err(_) => target,
        }
    }

    /// When event `event` is played, counting from none, if the recording has it: after
    /// [`Self::events_at`] has given `event`, the time the screen next changes.
    pub fn time_of(&self, event: usize) -> Option<Duration> {
        self.times.get(event).copied()
    }

    /// When the recording ends: the played time of its last event.
    pub fn end(&self) -> Duration {
        self.times.last().copied().unwrap_or_default()
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

    /// Plays the next event, keeping the screen as it is after it if it's due and paid for.
    fn step(&mut self) {
        let what = &self.recording.events[self.played].what;
        apply(&mut self.screen, what);
        let at = self.times[self.played];
        self.played += 1;
        let Some(latest) = self.kept.last() else { return };
        if self.played <= latest.events || self.keeping > MOST_WORK {
            return;
        }
        let work = self.screen.work().saturating_sub(latest.screen.work());
        let places = self.screen.places();
        let least = (PLACE_WORK * places).max(self.spacing);
        if work < least {
            return;
        }
        let mark = matches!(what, What::Mark(_));
        if !mark && at < latest.at.saturating_add(KEPT_EVERY) && work < KEPT_ANYWAY.max(self.spacing) {
            return;
        }
        // Each row written to again once it's shared is copied.
        let copies = self.screen.owned_cells();
        if work < least + copies {
            return;
        }
        // The rows made since, though no more than the screen holds: those made and let go
        // since are gone.
        let made = self.screen.made().saturating_sub(latest.screen.made()).min(places * u64::from(self.screen.width()));
        let bytes = KEPT_OVERHEAD + places as usize * ROW_PLACE + made as usize * size_of::<Cell>();
        self.keeping = self.keeping.saturating_add(PLACE_WORK * places + copies);
        self.keep(at, mark, bytes);
    }

    /// All the kept screens hold, in bytes, each row counted once however many share it.
    fn count_kept(&mut self) -> usize {
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
        self.keeping = self.keeping.saturating_add(PLACE_WORK * rows.len() as u64);
        bytes
    }

    /// Keeps the screen as it is, first keeping fewer of those kept before if there'd be too
    /// many.
    fn keep(&mut self, at: Duration, mark: bool, bytes: usize) {
        while self.kept.len() >= MOST_KEPT_SCREENS || self.kept_bytes + bytes > MOST_KEPT_BYTES {
            if !self.thin(true) && !self.thin(false) {
                return;
            }
        }
        self.kept_bytes += bytes;
        self.kept.push(Kept { events: self.played, at, mark, screen: self.screen.clone() });
    }

    /// Lets every other kept screen go, only those kept by time if `by_time`, and if any went,
    /// keeps them twice as far apart from now on. The first is always kept. Says whether any
    /// went.
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
        let first = self.kept.first().map_or(0, |kept| kept.screen.work());
        let last = self.kept.last().map_or(0, |kept| kept.screen.work());
        self.spacing = self.spacing.saturating_mul(2).max(last.saturating_sub(first) / self.kept.len() as u64);
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
