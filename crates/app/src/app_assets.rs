//! The icons the app's window draws: those that come with the components, and the tools' own.

use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::*;

// The editor's tools show these, beyond the icons that come with the components.
icon_assets!(
    ToolIcons,
    [
        Save, SquarePlus, CopyPlus, Trash, Presentation, Timer, CodeXml, ClipboardPaste, Scissors, Bold, Italic,
        Underline, DropletOff, List, ListOrdered, ListIndentDecrease, ListIndentIncrease, TextAlignStart,
        TextAlignCenter, TextAlignEnd, ImagePlus, Shapes, RectangleHorizontal, SquareRoundCorner, Circle, Minus,
        MoveRight, MessageSquare, TextCursorInput, Star, Table, BetweenHorizontalStart, BetweenHorizontalEnd,
        BetweenVerticalStart, BetweenVerticalEnd, Blocks, BringToFront, Film, CopyCheck, Sparkles, StepBack, StepForward,
        ListStart, LayoutTemplate, Video, Box, SquareTerminal,
    ]
);

/// The components' icons and the tools'.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if let Some(bytes) = ToolIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(ToolIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
