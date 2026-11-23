//! A live terminal: a program the talk names, run in a terminal of its own by a helper, and shown
//! as a recording is. The helper is a separate program, `stagepack-terminal`, spoken to in short
//! frames over its standard input and output, so the app never links a pseudo-terminal library.
//!
//! This is what runs a talk's command, so nothing here starts one by itself: a [`Session`] runs
//! only what it's given, and what asks for the yes is above it.

mod ask;
mod helper;
mod keys;
mod permission;
mod session;

pub use ask::{RUN_ALWAYS_ID, RUN_ASK_ID, RUN_IN_NEW_FOLDER, RUN_LINE_ID, RUN_NO_ID, RUN_QUESTION, RUN_WARNING, RUN_YES_ID, RunAnswer, RunQuestion, ask_to_run};
pub use helper::{HELPER_ENV, find_helper};
pub use keys::{key_bytes, key_bytes_in};
pub use permission::{Decision, RunPermissions, SessionYes, TalkFolder, may_run, start_after_yes};
pub use session::{Look, MOST_FLOOD_A_SECOND, Session, SessionState, THREAD_NAME, threads_running};
pub use slides_terminal::split_command;
