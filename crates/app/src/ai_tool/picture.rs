//! A picture of a slide at a click step, drawn as the PDF export draws it: the deck's layers
//! as they settle, a video as its still, a model as the 3D drawer's still, a recording as its
//! last screen.

use std::panic::{AssertUnwindSafe, catch_unwind};

use slides_engine::{Moment, Pixmap, SLIDE_WIDTH, compose};

use super::check::slide_problems;
use super::talk::OpenTalk;

/// The narrowest and widest a picture may be, in pixels.
pub const LEAST_WIDTH: u32 = 64;
pub const MOST_WIDTH: u32 = 3840;
/// The width a picture has when none is asked for.
pub const DEFAULT_WIDTH: u32 = 1920;

/// A slide drawn as a PNG, with what's wrong with the slide.
pub struct Picture {
    pub png: Vec<u8>,
    /// From 1.
    pub slide: usize,
    pub step: u32,
    /// How many clicks the slide has.
    pub steps: u32,
    pub problems: Vec<String>,
}

/// Slide `slide` (from 1) at click `step` (its last when `None`) as a PNG `width` wide and
/// `width * 9 / 16` high. An `Err` is one plain sentence naming what's out of range.
pub fn picture(talk: &mut OpenTalk, slide: usize, step: Option<u32>, width: u32) -> Result<Picture, String> {
    let count = talk.deck.slide_count();
    if slide < 1 || slide > count {
        return Err(match count {
            1 => format!("This talk has 1 slide, so there's no slide {slide}."),
            _ => format!("This talk has {count} slides, so there's no slide {slide}."),
        });
    }
    if !(LEAST_WIDTH..=MOST_WIDTH).contains(&width) {
        return Err(format!("A picture is {LEAST_WIDTH} to {MOST_WIDTH} pixels wide, so {width} won't do."));
    }
    let index = slide - 1;
    let drawn = catch_unwind(AssertUnwindSafe(|| {
        let steps = talk.deck.timeline(index).steps;
        let step = step.unwrap_or(steps);
        if step > steps {
            let clicks = if steps == 1 { "1 click".to_string() } else { format!("{steps} clicks") };
            return Err(format!("Slide {slide} has {clicks}, so there's no step {step}."));
        }
        let height = width * 9 / 16;
        let mut pixmap = Pixmap::new(width, height).ok_or_else(|| format!("A picture {width} wide can't be made."))?;
        let moment = Moment { step, time: 0.0 };
        // Both from the one layout, as the show asks them: a layer waiting for a later click isn't there yet.
        let layers = talk.deck.layers_at(index, moment, width as f32 / SLIDE_WIDTH);
        let timings = talk.deck.timings_at(index, moment);
        let shown: Vec<_> = layers
            .into_iter()
            .zip(timings)
            .filter(|(_, timing)| timing.click.is_none_or(|click| click <= step))
            .map(|(layer, _)| layer)
            .collect();
        compose(&mut pixmap, &shown);
        let png = pixmap.encode_png().map_err(|error| format!("Slide {slide} couldn't be made into a picture: {error}."))?;
        Ok((png, step, steps))
    }));
    let (png, step, steps) = match drawn {
        Ok(drawn) => drawn?,
        Err(_) => return Err(format!("Slide {slide} couldn't be laid out, so it can't be drawn.")),
    };
    let problems = slide_problems(talk, index);
    Ok(Picture { png, slide, step, steps, problems })
}

/// What the command says once it has written the picture to `out`.
pub fn picture_text(picture: &Picture, out: &str) -> String {
    let clicks = match picture.steps {
        0 => String::new(),
        steps => format!(", click {} of {steps}", picture.step),
    };
    let mut text = format!("Slide {}{clicks}, written to {out}.\n", picture.slide);
    for problem in &picture.problems {
        text.push_str(&format!("Slide {}: {problem}\n", picture.slide));
    }
    text
}
