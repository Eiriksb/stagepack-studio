//! A live terminal: a program the talk names, run in a terminal of its own by a helper, and shown
//! as a recording is. The helper is a separate program, `stagepack-terminal`, spoken to in short
//! frames over its standard input and output, so the app never links a pseudo-terminal library.
//!
//! This is what runs a talk's command, so nothing here starts one by itself: a [`Session`] runs
//! only what it's given, and what asks for the yes is above it.

mod helper;
mod keys;
mod session;

pub use helper::{HELPER_ENV, find_helper};
pub use keys::{key_bytes, key_bytes_in};
pub use session::{Look, MOST_FLOOD_A_SECOND, Session, SessionState, THREAD_NAME, threads_running};
pub use slides_terminal::split_command;
