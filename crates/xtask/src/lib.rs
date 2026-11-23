//! The packaging tool's parts: the icon and the licence notices.

pub mod icon;
pub mod notices;

pub use icon::{draw_icon, write_ico};
pub use notices::{Notice, notices};
