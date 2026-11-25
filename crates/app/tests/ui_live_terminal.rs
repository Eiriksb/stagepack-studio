//! A live terminal in the show and the editor: asked for, run, typed into, and ended with the
//! talk. Every test uses the real helper and commands of its own choosing.
#![cfg(unix)]

#[path = "support/helper.rs"]
mod helper;
#[path = "support/typing.rs"]
mod typing;

use std::path::Path;
use std::rc::Rc;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use typing::*;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, InputEvent as _, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Modifiers, Point, TestAppContext, px};
use stagepack_studio::live_terminal::{
    HELPER_ENV, LiveId, LiveTerminals, MOST_LIVE, Session, NOT_INSTALLED, RUN_ASK_ID, RUN_HEAD_ID, RUN_INTO_ID, RUN_LINE_ID, RUN_NO_ID, RUN_TITLE, RUN_TITLE_ID,
    RUN_TYPED_ID, RUN_YES_ID, RunPermissions, RunQuestion, TOO_MANY, TYPE_TITLE, ask_to_run, threads_running,
};
use stagepack_studio::present_view::{COUNTER_ID, Presentation};
use stagepack_studio::slide_canvas::{LIVE_HINT_ID, SlideCanvas};
use stagepack_studio::slide_editor::{TERMINAL_END_ID, TERMINAL_KEEP_ID, TERMINAL_START_ID};

static ENVIRONMENT: Mutex<()> = Mutex::new(());

/// What a test of the live terminal needs of its surroundings: the helper to find, and a data
/// folder of its own, kept until the test ends.
struct Surroundings {
    _turn: MutexGuard<'static, ()>,
    scratch: tempfile::TempDir,
}

impl Surroundings {
    fn permissions(&self) -> RunPermissions {
        RunPermissions::load(self.scratch.path().join("terminal-permissions.json"))
    }
}

impl Drop for Surroundings {
    fn drop(&mut self) {
        // SAFETY: the tests that read the environment take their turn on `ENVIRONMENT`.
        unsafe { std::env::remove_var(HELPER_ENV) };
    }
}

fn surroundings(helper: Option<&Path>) -> Surroundings {
    let turn = ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner());
    let scratch = tempfile::tempdir().unwrap();
    // SAFETY: as above.
    unsafe {
        std::env::set_var("HOME", scratch.path());
        std::env::set_var("XDG_DATA_HOME", scratch.path().join("data"));
        std::env::set_var("XDG_CACHE_HOME", scratch.path().join("cache"));
        std::env::set_var("XDG_CONFIG_HOME", scratch.path().join("config"));
        match helper {
            Some(helper) => std::env::set_var(HELPER_ENV, helper),
            None => std::env::set_var(HELPER_ENV, scratch.path().join("no-such-helper")),
        }
    }
    Surroundings { _turn: turn, scratch }
}

/// A terminal tag running `command` at `x`, with a screen of 40 by 6.
fn tag(command: &str, x: u32) -> String {
    format!("<Terminal run='{command}' cols=\"40\" rows=\"6\" x=\"{x}\" y=\"100\" w=\"640\" h=\"360\" />")
}

/// Opens a talk of `source` in a window that keeps what it's said yes to in the test's own folder.
fn open_talk(cx: &mut TestAppContext, around: &Surroundings, source: &str) -> Typing {
    let typing = open(cx, source);
    let permissions = around.permissions();
    typing.view.update(cx, |view, _| view.set_run_permissions_for_test(permissions));
    typing
}

fn live_of(typing: &Typing, cx: &TestAppContext) -> LiveTerminals {
    typing.view.read_with(cx, |view, _| view.live())
}

fn render(cx: &mut TestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
    cx.run_until_parked();
}

/// Lets real time and the test clock pass, drawing `windows`, until `done` or `seconds` run out.
fn until(cx: &mut TestAppContext, windows: &[AnyWindowHandle], seconds: u64, mut done: impl FnMut(&mut TestAppContext) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(seconds);
    loop {
        for window in windows {
            render(cx, *window);
        }
        if done(cx) {
            return true;
        }
        if Instant::now() > end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
        cx.executor().advance_clock(Duration::from_millis(250));
    }
}

fn dialog_open(cx: &mut TestAppContext, window: AnyWindowHandle) -> bool {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(RUN_ASK_ID).is_some()
    })
    .unwrap_or(false)
}

fn click(cx: &mut TestAppContext, window: AnyWindowHandle, id: &'static str) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn press(cx: &mut TestAppContext, window: AnyWindowHandle, keys: &str) {
    cx.simulate_keystrokes(window, keys);
    cx.run_until_parked();
}

/// A show of the talk, rehearsing: its audience's window and its presenter's.
struct Rehearsal {
    show: Entity<Presentation>,
    audience: AnyWindowHandle,
    presenter: AnyWindowHandle,
}

impl Rehearsal {
    fn windows(&self) -> [AnyWindowHandle; 2] {
        [self.audience, self.presenter]
    }

    fn canvas(&self, cx: &TestAppContext) -> Entity<SlideCanvas> {
        let view = cx.read(|cx| self.show.read(cx).audience()).expect("the audience's window is open").1;
        cx.read(|cx| view.read(cx).current().clone())
    }

    fn counter(&self, cx: &mut TestAppContext) -> String {
        cx.update_window(self.presenter, |_, window, cx| {
            window.render_frame(cx);
            window.find(COUNTER_ID).label().map(str::to_string).expect("a counter")
        })
        .unwrap()
    }

    /// The canvas of the current slide in the presenter's window.
    fn presenter_canvas(&self, cx: &TestAppContext) -> Entity<SlideCanvas> {
        let view = cx.read(|cx| self.show.read(cx).presenter()).expect("the presenter's window is open").1;
        cx.read(|cx| view.read(cx).current().clone())
    }

    /// A click at (`x`, `y`) on the slide as the presenter's window shows it.
    fn click_presenter_slide(&self, cx: &mut TestAppContext, x: f32, y: f32) {
        let canvas = self.presenter_canvas(cx);
        let slide = canvas.read_with(cx, |canvas, _| canvas.slide_bounds()).expect("the slide is drawn");
        let unit = f32::from(slide.size.width) / 1920.;
        let at = slide.origin + Point { x: px(x * unit), y: px(y * unit) };
        cx.update_window(self.presenter, |_, window, cx| {
            window.render_frame(cx);
            let modifiers = Modifiers::default();
            window.dispatch_event(MouseMoveEvent { position: at, pressed_button: None, modifiers }.to_platform_input(), cx);
            window.dispatch_event(
                MouseDownEvent { button: MouseButton::Left, position: at, modifiers, click_count: 1, first_mouse: false }.to_platform_input(),
                cx,
            );
            window.dispatch_event(MouseUpEvent { button: MouseButton::Left, position: at, modifiers, click_count: 1 }.to_platform_input(), cx);
            window.render_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
    }

    fn live_layers(&self, cx: &mut TestAppContext) -> Vec<usize> {
        let canvas = self.canvas(cx);
        canvas.read_with(cx, |canvas, _| canvas.live_layers())
    }

    /// Where the middle of what's at (`x`, `y`) on the slide is in the audience's window, clicked.
    fn click_slide(&self, cx: &mut TestAppContext, x: f32, y: f32) {
        let canvas = self.canvas(cx);
        let slide = canvas.read_with(cx, |canvas, _| canvas.slide_bounds()).expect("the slide is drawn");
        let unit = f32::from(slide.size.width) / 1920.;
        let at = slide.origin + Point { x: px(x * unit), y: px(y * unit) };
        cx.update_window(self.audience, |_, window, cx| {
            window.render_frame(cx);
            let modifiers = Modifiers::default();
            window.dispatch_event(MouseMoveEvent { position: at, pressed_button: None, modifiers }.to_platform_input(), cx);
            window.dispatch_event(
                MouseDownEvent { button: MouseButton::Left, position: at, modifiers, click_count: 1, first_mouse: false }.to_platform_input(),
                cx,
            );
            window.dispatch_event(MouseUpEvent { button: MouseButton::Left, position: at, modifiers, click_count: 1 }.to_platform_input(), cx);
            window.render_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
}

/// Starts a rehearsal of the talk in `typing`'s window.
fn rehearse(typing: &Typing, cx: &mut TestAppContext) -> Rehearsal {
    typing.press(cx, "shift-f5");
    cx.run_until_parked();
    let show = typing.view.read_with(cx, |view, _| view.current_show()).expect("a show was started");
    let (audience, presenter) = cx.read(|cx| {
        let show = show.read(cx);
        (show.audience().expect("an audience window").0, show.presenter().expect("a presenter window").0)
    });
    let rehearsal = Rehearsal { show, audience, presenter };
    for window in rehearsal.windows() {
        render(cx, window);
    }
    rehearsal
}

/// The processes whose command line has `needle` in it, which aren't gone or about to be.
fn processes_with(needle: &str) -> Vec<u32> {
    let Ok(listing) = std::fs::read_dir("/proc") else { return Vec::new() };
    listing
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let line = std::fs::read(entry.path().join("cmdline")).ok()?;
            if !String::from_utf8_lossy(&line).replace('\0', " ").contains(needle) {
                return None;
            }
            // One that has ended and waits to be looked at is gone.
            let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
            let state = stat.rsplit(')').next()?.trim_start().chars().next()?;
            (state != 'Z').then_some(pid)
        })
        .collect()
}

fn screen_text(session: &stagepack_studio::live_terminal::Session) -> String {
    let screen = session.screen();
    let mut text = String::new();
    for row in screen.rows() {
        let line: String = row.iter().map(|cell| cell.text).collect();
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

/// Everything a test leaves behind is gone: no program, no thread, no session.
fn nothing_left(cx: &mut TestAppContext, windows: &[AnyWindowHandle], live: &LiveTerminals, needle: &str) {
    let gone = until(cx, windows, 8, |_| processes_with(needle).is_empty() && threads_running() == 0 && live.sessions() == 0);
    assert!(gone, "left over: {:?}, {} threads, {} sessions", processes_with(needle), threads_running(), live.sessions());
}

/// Answers the question about running with *Run*, in `window`.
fn say_yes(cx: &mut TestAppContext, window: AnyWindowHandle) {
    assert!(until(cx, &[window], 5, |cx| dialog_open(cx, window)), "the question was asked");
    click(cx, window, RUN_YES_ID);
}

#[gpui_kit::test]
fn the_show_asks_then_runs(cx: &mut TestAppContext) {
    let around = surroundings(Some(&helper::helper_program()));
    let talk = format!("# One\n\n{}\n\n---\n\n# Two\n", tag("sh -c \"echo ready; exec sleep 31301\"", 100));
    let typing = open_talk(cx, &around, &talk);
    let live = live_of(&typing, cx);
    let show = rehearse(&typing, cx);

    // Reaching the slide asks, over the slide the presenter looks at, and starts nothing.
    assert!(until(cx, &show.windows(), 5, |cx| dialog_open(cx, show.presenter)), "the presenter's window asks");
    assert!(!dialog_open(cx, show.audience));
    assert!(processes_with("31301").is_empty() && live.sessions() == 0);
    // While it's open the show's keys don't advance the slides.
    press(cx, show.audience, "right");
    press(cx, show.audience, "space");
    assert_eq!(show.counter(cx), "1 / 2");

    click(cx, show.presenter, RUN_YES_ID);
    assert!(until(cx, &show.windows(), 10, |_| live.running() == 1), "Run starts it");
    let session = live.get(&live.ids()[0]).unwrap();
    assert!(until(cx, &show.windows(), 10, |_| screen_text(&session).contains("ready")), "{}", screen_text(&session));
    // Its output is in the picture over its layer.
    assert!(until(cx, &show.windows(), 10, |cx| show.canvas(cx).read_with(cx, |canvas, _| canvas.live_pictures().iter().any(Option::is_some))));
    assert_eq!(show.live_layers(cx).len(), 1);
    assert!(!processes_with("31301").is_empty());

    // Leaving the slide stops nothing; coming back shows it as it is.
    press(cx, show.presenter, "right");
    assert_eq!(show.counter(cx), "2 / 2");
    for window in show.windows() {
        render(cx, window);
    }
    assert!(show.live_layers(cx).is_empty());
    assert_eq!(live.running(), 1);
    press(cx, show.presenter, "left");
    assert_eq!(show.counter(cx), "1 / 2");
    assert!(until(cx, &show.windows(), 5, |cx| show.live_layers(cx).len() == 1));
    assert_eq!(live.running(), 1, "the same program, still running");
    assert!(!dialog_open(cx, show.presenter), "nothing is asked twice");

    // Ending the show ends it.
    press(cx, show.presenter, "escape");
    nothing_left(cx, &[typing.handle], &live, "31301");
}

#[gpui_kit::test]
fn dont_run_keeps_the_still_for_the_session(cx: &mut TestAppContext) {
    let around = surroundings(Some(&helper::helper_program()));
    let talk = format!("# One\n\n{}\n\n---\n\n# Two\n", tag("sh -c \"exec sleep 31302\"", 100));
    let typing = open_talk(cx, &around, &talk);
    let live = live_of(&typing, cx);
    let show = rehearse(&typing, cx);
    assert!(until(cx, &show.windows(), 5, |cx| dialog_open(cx, show.presenter)));
    click(cx, show.presenter, RUN_NO_ID);
    for _ in 0..3 {
        press(cx, show.presenter, "right");
        assert_eq!(show.counter(cx), "2 / 2");
        press(cx, show.presenter, "left");
        assert_eq!(show.counter(cx), "1 / 2");
        for window in show.windows() {
            render(cx, window);
        }
        assert!(!dialog_open(cx, show.presenter) && !dialog_open(cx, show.audience), "it isn't asked again");
    }
    std::thread::sleep(Duration::from_millis(500));
    assert!(processes_with("31302").is_empty());
    assert_eq!(live.sessions(), 0);
    assert!(show.live_layers(cx).is_empty(), "its still shows");
    press(cx, show.presenter, "escape");
}

#[gpui_kit::test]
fn the_keys_go_to_one_place(cx: &mut TestAppContext) {
    let around = surroundings(Some(&helper::helper_program()));
    let talk = format!("# One\n\n{}\n\n{}\n\n---\n\n# Two\n", tag("sh -c \"cat; : 31305\"", 100), tag("sh -c \"cat; : 31305\"", 1000));
    let typing = open_talk(cx, &around, &talk);
    let live = live_of(&typing, cx);
    let show = rehearse(&typing, cx);
    say_yes(cx, show.presenter);
    assert!(until(cx, &show.windows(), 10, |_| live.running() == 2), "both run: {}", live.running());
    let mut ids: Vec<LiveId> = live.ids();
    ids.sort_by_key(|id| id.source().start);
    let (first, second) = (live.get(&ids[0]).unwrap(), live.get(&ids[1]).unwrap());
    assert!(until(cx, &show.windows(), 5, |cx| show.canvas(cx).read_with(cx, |canvas, _| canvas.live_layers().len() == 2)));

    // Nothing has the keys until one is clicked, and then the clicked one has them, with its frame.
    assert!(live.keys().is_none());
    show.click_slide(cx, 420., 280.);
    assert_eq!(live.keys().as_ref(), Some(&ids[0]));
    for window in show.windows() {
        render(cx, window);
    }
    let canvas = show.canvas(cx);
    let framed = canvas.read_with(cx, |canvas, _| canvas.live_keys_on());
    assert!(framed.is_some(), "the 3 px frame is drawn on it");
    assert_eq!(show.counter(cx), "1 / 2", "a click on it is no click of the show's");

    // Right Arrow and Space go to the program, from either window, and the slide doesn't change.
    press(cx, show.presenter, "right");
    press(cx, show.audience, "right");
    press(cx, show.presenter, "a space b");
    assert_eq!(show.counter(cx), "1 / 2");
    assert!(until(cx, &show.windows(), 10, |_| screen_text(&first).contains("^[[C^[[C")), "{}", screen_text(&first));
    assert!(screen_text(&first).contains("a b"), "{}", screen_text(&first));
    assert!(!screen_text(&second).contains("^["), "the other has nothing");

    // Escape alone goes to the program too.
    press(cx, show.presenter, "escape");
    assert_eq!(live.running(), 2, "the show wasn't ended");
    assert!(until(cx, &show.windows(), 10, |_| screen_text(&first).contains("^[")));

    // A click on the second moves the keys to it.
    show.click_slide(cx, 1320., 280.);
    assert_eq!(live.keys().as_ref(), Some(&ids[1]));
    for window in show.windows() {
        render(cx, window);
    }
    let framed_now = canvas.read_with(cx, |canvas, _| canvas.live_keys_on());
    assert!(framed_now.is_some() && framed_now != framed, "the frame moved with the keys");
    press(cx, show.presenter, "x");
    assert!(until(cx, &show.windows(), 10, |_| screen_text(&second).contains('x')));

    // Ctrl+Shift+Q gives them back, and Right Arrow then moves the show.
    press(cx, show.presenter, "ctrl-shift-q");
    assert!(live.keys().is_none());
    for window in show.windows() {
        render(cx, window);
    }
    assert!(canvas.read_with(cx, |canvas, _| canvas.live_keys_on()).is_none(), "the frame is gone");
    press(cx, show.presenter, "right");
    assert_eq!(show.counter(cx), "2 / 2");
    press(cx, show.presenter, "left");

    // A click elsewhere on the slide takes them back, and moves nothing.
    show.click_slide(cx, 420., 280.);
    assert!(live.keys().is_some());
    show.click_slide(cx, 50., 50.);
    assert!(live.keys().is_none());
    assert_eq!(show.counter(cx), "1 / 2");

    // A terminal that ends while it holds the keys gives them back.
    show.click_slide(cx, 420., 280.);
    assert!(live.keys().is_some());
    // The end of the input, at the start of a line.
    press(cx, show.presenter, "enter");
    press(cx, show.presenter, "ctrl-d");
    assert!(until(cx, &show.windows(), 10, |_| first.state().is_over()), "{:?}", first.state());
    assert!(live.keys().is_none());
    // Within a fifth of a second the windows follow.
    cx.executor().advance_clock(Duration::from_millis(250));
    for window in show.windows() {
        render(cx, window);
    }
    press(cx, show.presenter, "right");
    assert_eq!(show.counter(cx), "2 / 2", "the show has its keys again");

    press(cx, show.presenter, "escape");
    nothing_left(cx, &[typing.handle], &live, "31305");
}

/// How a test of ending leaves.
#[derive(Clone, Copy, Debug)]
enum Leave {
    EndTheShow,
    CloseTheWindow,
    DeleteTheTagThenUndo,
    ChangeRun,
    Stop,
}

#[gpui_kit::test]
fn every_way_of_leaving_ends_the_program(cx: &mut TestAppContext) {
    let around = surroundings(Some(&helper::helper_program()));
    let ways = [Leave::EndTheShow, Leave::CloseTheWindow, Leave::DeleteTheTagThenUndo, Leave::ChangeRun, Leave::Stop];
    for (number, way) in ways.into_iter().enumerate() {
        let needle = format!("3140{number}");
        let command = format!("sh -c \"exec sleep {needle}\"");
        let talk = format!("# One\n\n{}\n", tag(&command, 100));
        let typing = open_talk(cx, &around, &talk);
        let live = live_of(&typing, cx);
        let mut windows = vec![typing.handle];
        let mut show = None;
        if let Leave::EndTheShow = way {
            let rehearsal = rehearse(&typing, cx);
            say_yes(cx, rehearsal.presenter);
            windows.extend(rehearsal.windows());
            show = Some(rehearsal);
        } else {
            // Selected on the slide and started with *Start*, which asks first.
            let terminal = thing(&typing, cx, "<Terminal");
            let at = typing.at(cx, terminal.rect.x + terminal.rect.w / 5., terminal.rect.y + terminal.rect.h / 5.);
            typing.click(cx, at);
            assert!(bounds_of(&typing, cx, TERMINAL_START_ID).is_some(), "{way:?}: Start is offered");
            assert!(live.sessions() == 0, "selecting starts nothing");
            click_on(&typing, cx, TERMINAL_START_ID);
            assert!(until(cx, &windows, 5, |cx| bounds_of(&typing, cx, RUN_YES_ID).is_some()), "{way:?}: asked");
            click_on(&typing, cx, RUN_YES_ID);
        }
        assert!(until(cx, &windows, 10, |_| live.running() == 1), "{way:?}: it runs");
        assert!(until(cx, &windows, 5, |_| !processes_with(&needle).is_empty()), "{way:?}: the program is there");
        if !matches!(way, Leave::EndTheShow) {
            // Running, the editor offers *Stop* and *Keep as recording* in place of *Start*.
            assert!(bounds_of(&typing, cx, TERMINAL_END_ID).is_some(), "{way:?}: Stop");
            assert!(bounds_of(&typing, cx, TERMINAL_KEEP_ID).is_some(), "{way:?}: Keep as recording");
            assert!(bounds_of(&typing, cx, TERMINAL_START_ID).is_none());
        }
        match way {
            Leave::EndTheShow => press(cx, show.as_ref().unwrap().presenter, "escape"),
            Leave::CloseTheWindow => {
                cx.update_window(typing.handle, |_, window, _| window.remove_window()).unwrap();
                cx.run_until_parked();
            }
            Leave::DeleteTheTagThenUndo => typing.press(cx, "delete"),
            Leave::ChangeRun => {
                let changed = talk.replace(&needle, "31499");
                let document = typing.document(cx);
                cx.update(|cx| {
                    document.update(cx, |document, cx| {
                        document.reload(Some(changed));
                        cx.notify();
                    })
                });
                cx.run_until_parked();
            }
            Leave::Stop => click_on(&typing, cx, TERMINAL_END_ID),
        }
        let windows: Vec<AnyWindowHandle> = windows.into_iter().filter(|window| cx.read(|cx| cx.windows().contains(window))).collect();
        nothing_left(cx, &windows, &live, &needle);
        if let Leave::DeleteTheTagThenUndo = way {
            typing.press(cx, "ctrl-z");
            assert!(typing.source(cx).contains("<Terminal"), "the tag is back");
            std::thread::sleep(Duration::from_millis(300));
            wait(cx, 1_000);
            assert!(processes_with(&needle).is_empty() && live.sessions() == 0, "Undo starts nothing");
        }
        assert!(processes_with("31499").is_empty());
    }
}

#[gpui_kit::test]
fn without_the_add_on_the_recording_shows(cx: &mut TestAppContext) {
    let around = surroundings(None);
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("assets")).unwrap();
    std::fs::write(folder.path().join("assets/demo.cast"), "{\"version\": 2, \"width\": 40, \"height\": 6}\n[0.1, \"o\", \"hello\\r\\n\"]\n").unwrap();
    let talk = "# One\n\n<Terminal run=\"sleep 31303\" src=\"assets/demo.cast\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" />\n\n---\n\n# Two\n";
    std::fs::write(folder.path().join("slides.md"), talk).unwrap();
    let typing = open_folder(cx, folder, None);
    let permissions = around.permissions();
    typing.view.update(cx, |view, _| view.set_run_permissions_for_test(permissions));
    let live = live_of(&typing, cx);
    let show = rehearse(&typing, cx);
    for _ in 0..3 {
        for window in show.windows() {
            render(cx, window);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!dialog_open(cx, show.presenter) && !dialog_open(cx, show.audience) && !dialog_open(cx, typing.handle), "nothing is asked");
    assert_eq!(typing.status(cx).as_deref(), Some(NOT_INSTALLED));
    assert!(until(cx, &show.windows(), 10, |cx| show.canvas(cx).read_with(cx, |canvas, _| !canvas.playing_recordings().is_empty())), "its recording plays");
    assert_eq!(live.sessions(), 0);
    assert!(show.live_layers(cx).is_empty());
    assert!(processes_with("31303").is_empty());
    // Said once: coming back to the slide doesn't say it again.
    cx.update_window(typing.handle, |_, _, _| ()).unwrap();
    assert!(live.first_time("something else") && !live.first_time(NOT_INSTALLED));
    press(cx, show.presenter, "escape");
}

#[gpui_kit::test]
fn at_most_four_run(cx: &mut TestAppContext) {
    let around = surroundings(Some(&helper::helper_program()));
    let tags: Vec<String> = (0..5).map(|n| tag("sleep 31304", 100 + 300 * n)).collect();
    let talk = format!("# One\n\n{}\n", tags.join("\n\n"));
    let typing = open_talk(cx, &around, &talk);
    let live = live_of(&typing, cx);
    let show = rehearse(&typing, cx);
    say_yes(cx, show.presenter);
    assert!(until(cx, &show.windows(), 10, |_| live.running() == MOST_LIVE), "four run");
    assert!(until(cx, &show.windows(), 10, |cx| typing.status(cx).as_deref() == Some(TOO_MANY)), "{:?}", typing.status(cx));
    std::thread::sleep(Duration::from_millis(500));
    for window in show.windows() {
        render(cx, window);
    }
    assert_eq!(live.running(), MOST_LIVE, "no fifth");
    assert_eq!(processes_with("sleep 31304").len(), MOST_LIVE);
    assert_eq!(show.live_layers(cx).len(), MOST_LIVE, "the fifth shows its still");
    press(cx, show.presenter, "escape");
    nothing_left(cx, &[typing.handle], &live, "31304");
}

#[gpui_kit::test]
fn typing_in_the_editor_goes_to_the_terminal_only_while_it_has_the_keys(cx: &mut TestAppContext) {
    let around = surroundings(Some(&helper::helper_program()));
    let talk = format!("# One\n\n{}\n", tag("sh -c \"cat; : 31306\"", 100));
    let typing = open_talk(cx, &around, &talk);
    let live = live_of(&typing, cx);
    let windows = [typing.handle];
    let terminal = thing(&typing, cx, "<Terminal");
    let at = typing.at(cx, terminal.rect.x + terminal.rect.w / 5., terminal.rect.y + terminal.rect.h / 5.);
    typing.click(cx, at);
    click_on(&typing, cx, TERMINAL_START_ID);
    assert!(until(cx, &windows, 5, |cx| bounds_of(&typing, cx, RUN_YES_ID).is_some()));
    click_on(&typing, cx, RUN_YES_ID);
    assert!(until(cx, &windows, 10, |_| live.running() == 1));
    let session = live.get(&live.ids()[0]).unwrap();
    let before = typing.source(cx);
    // Started, it doesn't have the keys: that takes a click on it.
    assert!(live.keys().is_none());
    typing.click(cx, at);
    assert!(live.keys().is_some());
    typing.press(cx, "h");
    typing.press(cx, "i");
    typing.press(cx, "right");
    typing.press(cx, "delete");
    assert!(until(cx, &windows, 10, |_| screen_text(&session).contains("hi^[[C")), "{}", screen_text(&session));
    assert_eq!(typing.source(cx), before, "none of it was the editor's: Delete took nothing away");
    // Ctrl+Shift+Q gives them back: Delete is the editor's again.
    typing.press(cx, "ctrl-shift-q");
    assert!(live.keys().is_none());
    typing.press(cx, "delete");
    assert_ne!(typing.source(cx), before, "the tag was taken away");
    nothing_left(cx, &windows, &live, "31306");
}

/// Starts the terminal on the slide shown in the editor with *Start* and *Run*.
fn start_here(typing: &Typing, cx: &mut TestAppContext, live: &LiveTerminals, windows: &[AnyWindowHandle]) {
    let terminal = thing(typing, cx, "<Terminal");
    let at = typing.at(cx, terminal.rect.x + terminal.rect.w / 5., terminal.rect.y + terminal.rect.h / 5.);
    typing.click(cx, at);
    click_on(typing, cx, TERMINAL_START_ID);
    assert!(until(cx, windows, 5, |cx| bounds_of(typing, cx, RUN_YES_ID).is_some()));
    click_on(typing, cx, RUN_YES_ID);
    assert!(until(cx, windows, 10, |_| live.running() == 1));
}

fn reload(typing: &Typing, cx: &mut TestAppContext, source: String) {
    let document = typing.document(cx);
    cx.update(|cx| {
        document.update(cx, |document, cx| {
            document.reload(Some(source));
            cx.notify();
        })
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn typing_above_a_running_tag_keeps_it_running(cx: &mut TestAppContext) {
    let around = surroundings(Some(&helper::helper_program()));
    let talk = format!("# One\n\nhello\n\n---\n\n# Two\n\n{}\n", tag("sh -c \"cat; : 31309\"", 100));
    let typing = open_talk(cx, &around, &talk);
    let live = live_of(&typing, cx);
    let windows = [typing.handle];
    go_to(&typing, cx, 1);
    start_here(&typing, cx, &live, &windows);
    let id = live.ids()[0].clone();
    let session = live.get(&id).unwrap();
    // An edit on the first slide moves the tag and ends nothing.
    reload(&typing, cx, talk.replacen("hello", "hello there, all of you", 1));
    wait(cx, 1_000);
    assert_eq!(live.running(), 1);
    assert!(std::rc::Rc::ptr_eq(&live.get(&live.ids()[0]).unwrap(), &session), "the same session");
    assert!(live.ids()[0].source().start > id.source().start, "its place was followed");
    assert!(!processes_with("31309").is_empty());
    // Taking the tag away ends it.
    reload(&typing, cx, "# One\n\nhello there, all of you\n\n---\n\n# Two\n".to_string());
    nothing_left(cx, &windows, &live, "31309");
}

#[gpui_kit::test]
fn keys_stay_only_with_a_terminal_that_is_shown(cx: &mut TestAppContext) {
    let around = surroundings(Some(&helper::helper_program()));
    let talk = format!("# One\n\n---\n\n# Two\n\n{}\n", tag("sh -c \"cat; : 31310\"", 100));
    let typing = open_talk(cx, &around, &talk);
    let live = live_of(&typing, cx);
    let windows = [typing.handle];
    go_to(&typing, cx, 1);
    start_here(&typing, cx, &live, &windows);
    let session = live.get(&live.ids()[0]).unwrap();
    let terminal = thing(&typing, cx, "<Terminal");
    let at = typing.at(cx, terminal.rect.x + terminal.rect.w / 5., terminal.rect.y + terminal.rect.h / 5.);
    typing.click(cx, at);
    assert!(live.keys().is_some());
    // The slide changes, as from the strip: the keys go back.
    let document = typing.document(cx);
    cx.update(|cx| document.update(cx, |document, cx| {
        document.go_to(0);
        cx.notify();
    }));
    typing.settle(cx);
    assert!(live.keys().is_none());
    typing.press(cx, "h");
    typing.press(cx, "i");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!screen_text(&session).contains("hi"), "{}", screen_text(&session));
    // Coming back doesn't bring them back.
    cx.update(|cx| document.update(cx, |document, cx| {
        document.go_to(1);
        cx.notify();
    }));
    typing.settle(cx);
    assert!(live.keys().is_none());
    // Nor does a show starting from the ribbon find them taken.
    typing.click(cx, at);
    assert!(live.keys().is_some());
    cx.update_window(typing.handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(stagepack_studio::ribbon::TAB_SLIDE_SHOW_ID, cx);
        window.render_frame(cx);
        window.click(stagepack_studio::ribbon::REHEARSE_ID, cx);
    })
    .unwrap();
    cx.run_until_parked();
    let show = typing.view.read_with(cx, |view, _| view.current_show()).expect("a show");
    let presenter = cx.read(|cx| show.read(cx).presenter().unwrap().0);
    let audience = cx.read(|cx| show.read(cx).audience().unwrap().0);
    let rehearsal = Rehearsal { show, audience, presenter };
    for window in rehearsal.windows() {
        render(cx, window);
    }
    assert!(live.keys().is_none());
    press(cx, rehearsal.presenter, "right");
    assert_eq!(rehearsal.counter(cx), "2 / 2", "the show moves");
    press(cx, rehearsal.presenter, "escape");
    nothing_left(cx, &[typing.handle], &live, "31310");
}

#[gpui_kit::test]
fn keys_never_reach_a_program_while_a_question_is_open(cx: &mut TestAppContext) {
    let around = surroundings(Some(&helper::helper_program()));
    let talk = format!("# One\n\n{}\n\n{}\n", tag("sh -c \"cat; : 31311\"", 100), tag("sh -c \"exec sleep 31312\"", 1000));
    let typing = open_talk(cx, &around, &talk);
    let live = live_of(&typing, cx);
    let show = rehearse(&typing, cx);
    say_yes(cx, show.presenter);
    assert!(until(cx, &show.windows(), 10, |_| live.running() == 1));
    // The second command asks; meanwhile the first is clicked.
    assert!(until(cx, &show.windows(), 5, |cx| dialog_open(cx, show.presenter)));
    show.click_slide(cx, 420., 280.);
    let session = live.get(&live.ids()[0]).unwrap();
    press(cx, show.audience, "x");
    press(cx, show.presenter, "y");
    std::thread::sleep(Duration::from_millis(400));
    let text = screen_text(&session);
    assert!(!text.contains('x') && !text.contains('y'), "{text}");
    click(cx, show.presenter, RUN_NO_ID);
    press(cx, show.presenter, "escape");
    nothing_left(cx, &[typing.handle], &live, "3131");
}

/// Starts the terminal on the slide shown with *Start* and *Run*, and waits for its program to
/// end, as a short one does at once.
fn start_and_let_end(typing: &Typing, cx: &mut TestAppContext, live: &LiveTerminals, windows: &[AnyWindowHandle]) -> Rc<Session> {
    let terminal = thing(typing, cx, "<Terminal");
    let at = typing.at(cx, terminal.rect.x + terminal.rect.w / 5., terminal.rect.y + terminal.rect.h / 5.);
    typing.click(cx, at);
    click_on(typing, cx, TERMINAL_START_ID);
    assert!(until(cx, windows, 5, |cx| bounds_of(typing, cx, RUN_YES_ID).is_some()));
    click_on(typing, cx, RUN_YES_ID);
    assert!(until(cx, windows, 10, |_| live.sessions() == 1 && live.running() == 0), "the program ended");
    live.get(&live.ids()[0]).unwrap()
}

fn can_undo(typing: &Typing, cx: &TestAppContext) -> bool {
    typing.document(cx).read_with(cx, |document, _| document.can_undo())
}

#[gpui_kit::test]
fn a_session_kept_becomes_the_recording(cx: &mut TestAppContext) {
    let around = surroundings(Some(&helper::helper_program()));
    let talk = format!("# One\n\n{}\n", tag("printf hello", 100));
    let typing = open_talk(cx, &around, &talk);
    let mut live = live_of(&typing, cx);
    let windows = [typing.handle];
    let session = start_and_let_end(&typing, cx, &live, &windows);
    assert!(session.can_keep());
    let folder = typing._folder.path().to_path_buf();

    // Kept, then undone: the tag has no `src` again, and a save writes no file.
    assert!(!typing.source(cx).contains("src="));
    click_on(&typing, cx, TERMINAL_KEEP_ID);
    assert!(typing.source(cx).contains("src=\"assets/printf.cast\""), "{}", typing.source(cx));
    assert!(!folder.join("assets").exists(), "nothing is written until a save");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), talk, "Undo takes the src away");
    typing.press(cx, "ctrl-s");
    wait(cx, 500);
    typing.settle(cx);
    assert!(!folder.join("assets/printf.cast").exists(), "undone, the file isn't saved");

    // Kept again, and saved: the file is there, holding what the screen showed and nothing typed.
    click_on(&typing, cx, TERMINAL_KEEP_ID);
    assert!(typing.source(cx).contains("src=\"assets/printf.cast\""));
    typing.press(cx, "ctrl-s");
    wait(cx, 500);
    typing.settle(cx);
    let kept = std::fs::read(folder.join("assets/printf.cast")).expect("the recording is saved");
    let text = String::from_utf8(kept.clone()).unwrap();
    assert!(text.contains("hello"), "{text}");
    assert!(!text.contains("\"i\""), "no input events: {text}");
    slides_terminal::read(&kept).expect("it reads as a recording");
    assert!(can_undo(&typing, cx));
    drop(session);
    live.end_all();
    nothing_left(cx, &windows, &live, "printf hello");
}

#[gpui_kit::test]
fn a_kept_recording_plays_like_any_other(cx: &mut TestAppContext) {
    let around = surroundings(Some(&helper::helper_program()));
    let talk = format!("# One\n\n{}\n", tag("printf hello", 100));
    let typing = open_talk(cx, &around, &talk);
    let mut live = live_of(&typing, cx);
    let windows = [typing.handle];
    let _session = start_and_let_end(&typing, cx, &live, &windows);
    click_on(&typing, cx, TERMINAL_KEEP_ID);
    assert!(until(cx, &windows, 10, |cx| bounds_of(&typing, cx, stagepack_studio::slide_editor::TERMINAL_PLAY_ID).is_some()), "Play is offered on it");
    click_on(&typing, cx, stagepack_studio::slide_editor::TERMINAL_PLAY_ID);
    let canvas = cx.read(|cx| typing.view.read(cx).slide_canvas().clone());
    assert!(
        until(cx, &windows, 20, |cx| cx.read(|cx| canvas.read(cx).replay_pictures().first().is_some_and(Option::is_some))),
        "it played"
    );
    live.end_all();
}

#[test]
fn a_kept_recording_is_named_from_the_program_and_never_leaves_its_folder() {
    use stagepack_studio::live_terminal::recording_stem;
    assert_eq!(recording_stem("/usr/bin/bash"), "bash");
    assert_eq!(recording_stem("C:\\Windows\\System32\\cmd.exe"), "cmd");
    assert_eq!(recording_stem("my_prog.v2.exe"), "my_prog-v2");
    assert_eq!(recording_stem("../../etc/passwd"), "passwd");
    assert_eq!(recording_stem("..\\..\\evil"), "evil");
    assert_eq!(recording_stem("a b  c"), "a-b-c");
    assert_eq!(recording_stem("blåbær"), "bl-b-r");
    assert_eq!(recording_stem(".."), "terminal");
    assert_eq!(recording_stem("/"), "terminal");
    assert_eq!(recording_stem(""), "terminal");
    assert_eq!(recording_stem("日本語"), "terminal");
    assert_eq!(recording_stem(&"x".repeat(100)).len(), 40);
    for odd in ["../x/..", "a/../b", "\\..\\", "x y/z w", "ünï/çödé", "con", "a:b"] {
        let stem = recording_stem(odd);
        assert!(!stem.is_empty() && stem.len() <= 40, "{odd:?}");
        assert!(stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "{odd:?} -> {stem:?}");
    }
}

#[gpui_kit::test]
fn the_presenter_sees_the_live_terminal_and_a_click_there_gives_it_the_keys(cx: &mut TestAppContext) {
    let around = surroundings(Some(&helper::helper_program()));
    let talk = format!("# One\n\n{}\n", tag("sh -c \"echo shown31508; cat; : 31508\"", 100));
    let typing = open_talk(cx, &around, &talk);
    let live = live_of(&typing, cx);
    let show = rehearse(&typing, cx);
    say_yes(cx, show.presenter);
    assert!(until(cx, &show.windows(), 10, |_| live.running() == 1));
    let session = live.get(&live.ids()[0]).unwrap();
    assert!(until(cx, &show.windows(), 10, |_| screen_text(&session).contains("shown31508")));
    // The presenter's own slide has the terminal over its layer, with the picture the audience's has.
    assert!(until(cx, &show.windows(), 10, |cx| {
        let canvas = show.presenter_canvas(cx);
        canvas.read_with(cx, |canvas, _| canvas.live_layers().len() == 1 && canvas.live_pictures().iter().all(Option::is_some))
    }));
    let (theirs, ours) = (show.canvas(cx), show.presenter_canvas(cx));
    assert_eq!(theirs.read_with(cx, |canvas, _| canvas.live_layers()), ours.read_with(cx, |canvas, _| canvas.live_layers()));

    // A click on it in the presenter's window gives it the keys, framed there and in the audience's window.
    assert!(live.keys().is_none());
    show.click_presenter_slide(cx, 420., 280.);
    assert_eq!(live.keys().as_ref(), Some(&live.ids()[0]));
    for window in show.windows() {
        render(cx, window);
    }
    assert!(ours.read_with(cx, |canvas, _| canvas.live_keys_on()).is_some(), "the frame is drawn in the presenter's view");
    assert!(theirs.read_with(cx, |canvas, _| canvas.live_keys_on()).is_some());
    let hinted = cx.update_window(show.presenter, |_, window, cx| {
        window.render_frame(cx);
        window.try_find((LIVE_HINT_ID, 0usize)).is_some()
    });
    assert!(hinted.unwrap(), "it says how to give the keys back");
    assert_eq!(show.counter(cx), "1 / 1");
    press(cx, show.presenter, "q");
    assert!(until(cx, &show.windows(), 10, |_| screen_text(&session).contains('q')));
    // A click elsewhere on the presenter's slide takes the keys back.
    show.click_presenter_slide(cx, 50., 50.);
    assert!(live.keys().is_none());
    press(cx, show.presenter, "escape");
    nothing_left(cx, &[typing.handle], &live, "31508");
}

/// A helper that is a program that can't start the terminal: it counts each time it is run and ends.
fn failing_helper(folder: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let script = folder.join("failing-helper");
    std::fs::write(&script, format!("#!/bin/sh\necho run >> {}\n", folder.join("attempts").display())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[gpui_kit::test]
fn a_terminal_that_fails_to_start_in_the_show_is_tried_once(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let helper = failing_helper(folder.path());
    let around = surroundings(Some(&helper));
    let talk = format!("# One\n\n{}\n\n---\n\n# Two\n", tag("sh -c \"exec sleep 31509\"", 100));
    let typing = open_talk(cx, &around, &talk);
    let live = live_of(&typing, cx);
    let show = rehearse(&typing, cx);
    say_yes(cx, show.presenter);
    assert!(until(cx, &show.windows(), 10, |_| live.sessions() == 1 && live.running() == 0), "it fails");
    assert!(until(cx, &show.windows(), 5, |cx| typing.status(cx).is_some()), "it says so");
    let sentence = typing.status(cx);
    // Two seconds more, with the slide shown and the show asked for frames: nothing is started again.
    let end = Instant::now() + Duration::from_secs(2);
    while Instant::now() < end {
        for window in show.windows() {
            render(cx, window);
        }
        std::thread::sleep(Duration::from_millis(25));
        cx.executor().advance_clock(Duration::from_millis(250));
    }
    let attempts = std::fs::read_to_string(folder.path().join("attempts")).unwrap_or_default();
    assert_eq!(attempts.lines().count(), 1, "the helper was run {} times", attempts.lines().count());
    assert_eq!(live.sessions(), 1, "the failed one is kept");
    assert!(sentence.as_deref().is_some_and(|said| said.contains("couldn't")), "{sentence:?}");
    // Going away and coming back doesn't start it again either.
    press(cx, show.presenter, "right");
    press(cx, show.presenter, "left");
    for window in show.windows() {
        render(cx, window);
    }
    std::thread::sleep(Duration::from_millis(500));
    for window in show.windows() {
        render(cx, window);
    }
    let attempts = std::fs::read_to_string(folder.path().join("attempts")).unwrap_or_default();
    assert_eq!(attempts.lines().count(), 1);
    assert!(processes_with("31509").is_empty());
    press(cx, show.presenter, "escape");
    nothing_left(cx, &[typing.handle], &live, "31509");
}

#[gpui_kit::test]
fn start_runs_a_program_that_ended_again(cx: &mut TestAppContext) {
    let around = surroundings(Some(&helper::helper_program()));
    let talk = format!("# One\n\n{}\n", tag("printf again31510", 100));
    let typing = open_talk(cx, &around, &talk);
    let live = live_of(&typing, cx);
    let windows = [typing.handle];
    let first = start_and_let_end(&typing, cx, &live, &windows);
    assert!(screen_text(&first).contains("again31510"));
    // *Start* again: it has its yes, so it runs at once, in a new session in the old one's place.
    click_on(&typing, cx, TERMINAL_START_ID);
    assert!(until(cx, &windows, 10, |_| live.get(&live.ids()[0]).is_some_and(|now| !Rc::ptr_eq(&now, &first))), "a new session");
    let second = live.get(&live.ids()[0]).unwrap();
    assert!(until(cx, &windows, 10, |_| screen_text(&second).contains("again31510") && second.state().is_over()));
    assert!(until(cx, &windows, 5, |_| live.sessions() == 1), "the old one is let go of");
    assert!(!dialog_open(cx, typing.handle), "nothing was asked again");
}

/// The question for `question`, put directly in `window`, and closed with *Don't run* once looked at.
fn ask_directly(cx: &mut TestAppContext, window: AnyWindowHandle, question: RunQuestion) {
    let asked = cx.update_window(window, |_, window, cx| ask_to_run(window, cx, question, |_, _, _| {})).unwrap();
    cx.run_until_parked();
    assert!(asked);
}

/// The label of `id`, and where its top is, in `window`.
fn label_and_top(cx: &mut TestAppContext, window: AnyWindowHandle, id: impl Into<gpui_kit::ElementId>) -> Option<(String, f32)> {
    let id = id.into();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).filter(|found| found.visible()).map(|found| (found.label().unwrap_or_default().to_string(), f32::from(found.bounds().origin.y)))
    })
    .unwrap()
}

#[gpui_kit::test]
fn the_question_for_typed_text_shows_the_text_then_into_and_the_terminal(cx: &mut TestAppContext) {
    let around = surroundings(Some(&helper::helper_program()));
    let typing = open_talk(cx, &around, &format!("# One\n\n{}\n", tag("sh", 100)));
    let window = typing.handle;
    let question = RunQuestion {
        command: vec!["/bin/sh".to_string(), "-l".to_string()],
        program_in_talk: false,
        program_by_path: false,
        folder: None,
        typed: Some("make demo\t\"fast\"".to_string()),
    };
    ask_directly(cx, window, question);
    let (title, _) = label_and_top(cx, window, RUN_TITLE_ID).expect("a title");
    assert_eq!(title, TYPE_TITLE);
    assert_eq!(title, "Type this into the terminal?");
    // The whole text, as it can be seen: a tab is written out, a quote escaped.
    let (typed, typed_top) = label_and_top(cx, window, RUN_TYPED_ID).expect("the text");
    assert_eq!(typed, "\"make demo\\u{9}\\\"fast\\\"\"");
    let (into, into_top) = label_and_top(cx, window, RUN_INTO_ID).expect("into");
    assert_eq!(into, "into");
    let (first, first_top) = label_and_top(cx, window, (RUN_LINE_ID, 0usize)).expect("the terminal's command");
    let (second, _) = label_and_top(cx, window, (RUN_LINE_ID, 1usize)).expect("its argument");
    assert_eq!((first.as_str(), second.as_str()), ("\"/bin/sh\"", "\"-l\""));
    assert!(typed_top < into_top && into_top < first_top, "the text, then into, then the command: {typed_top} {into_top} {first_top}");
    assert!(label_and_top(cx, window, RUN_HEAD_ID).is_none(), "nothing about running a program");
    click(cx, window, RUN_NO_ID);
    assert!(!dialog_open(cx, window));

    // The question about running a program is as it was.
    let question = RunQuestion { command: vec!["/bin/sh".to_string()], program_in_talk: false, program_by_path: false, folder: None, typed: None };
    ask_directly(cx, window, question);
    assert_eq!(label_and_top(cx, window, RUN_TITLE_ID).map(|(title, _)| title).as_deref(), Some(RUN_TITLE));
    assert!(label_and_top(cx, window, RUN_TYPED_ID).is_none() && label_and_top(cx, window, RUN_INTO_ID).is_none());
    assert!(label_and_top(cx, window, RUN_HEAD_ID).is_some());
    click(cx, window, RUN_NO_ID);
}
