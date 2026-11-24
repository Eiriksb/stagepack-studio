//! The packaging tool's parts: the icon and the licence notices.

pub mod addon;
pub mod elf;
pub mod icon;
pub mod linux;
pub mod notices;
pub mod stage;
pub mod windows;

pub use icon::{draw_icon, write_ico};
pub use notices::{Notice, notices};
