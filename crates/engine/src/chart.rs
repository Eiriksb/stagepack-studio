//! The ready-made chart: bars, a line or a pie, from numbers written in the tag or from a
//! CSV file in the talk's `assets` folder. It is an ordinary component, drawn on a canvas
//! in the theme's colours, so it exports as shapes, text and paths.

use toolkit::prelude::*;

/// The most labels, and so bars, points or slices, a chart shows.
pub const MAX_POINTS: usize = 1000;
pub const MAX_SERIES: usize = 16;
/// Past this, a value's nice ceiling and tick labels stop being numbers a person reads.
const MAX_VALUE: f64 = 1e15;
const NO_DATA: &str = "The chart has no data";

const DEFAULT_WIDTH: f32 = 1200.0;
const MAX_DEFAULT_HEIGHT: f32 = 675.0;
/// As for any element, so that a chart can't ask for a layer that needs gigabytes.
const MAX_SIZE: f32 = 3840.0;

const TEXT_SIZE: f32 = 24.0;
const TITLE_SIZE: f32 = 40.0;
/// How tall a line of text is, for its size: as the engine sets it.
const LINE_HEIGHT: f32 = 1.25;
/// A generous guess at the width of a character, for its size. A component can't measure
/// text, so labels are kept apart by this.
const CHAR_WIDTH: f32 = 0.62;
/// The least space between two labels.
const LABEL_GAP: f32 = 16.0;
/// Longer labels are cut short, with an ellipsis.
const MAX_LABEL_CHARS: usize = 40;
const MAX_LEGEND_CHARS: usize = 24;

const TITLE_ROOM: f32 = 64.0;
const LEGEND_ROOM: f32 = 48.0;
const TICK_ROOM: f32 = 96.0;
const LABEL_ROOM: f32 = 56.0;
/// Above the plot, so that the top tick's label isn't cut off.
const TOP_PADDING: f32 = 16.0;
/// Right of the plot, so that the last label isn't cut off.
const RIGHT_PADDING: f32 = 24.0;
/// Between the plot and the labels beside and below it.
const LABEL_MARGIN: f32 = 12.0;
/// The most intervals between ticks on an axis.
const MAX_INTERVALS: i64 = 6;

const GROUP_SHARE: f32 = 0.8;
const LINE_WIDTH: f32 = 6.0;
const POINT_SIZE: f32 = 16.0;
/// Points are only marked where they are at least this far apart.
const POINT_ROOM: f32 = 24.0;
const GRID_WIDTH: f32 = 2.0;
const AXIS_WIDTH: f32 = 3.0;
const SWATCH: f32 = 20.0;
const SWATCH_GAP: f32 = 8.0;
const LEGEND_ITEM_GAP: f32 = 32.0;
const PIE_ROW: f32 = 40.0;
const PIE_GAP: f32 = 56.0;
const SLICE_EDGE: f32 = 3.0;
/// A pie of more slices than this has no edges between them, which would hide them.
const MAX_EDGED_SLICES: usize = 100;
const HUE_TURN: f32 = 47.0;

#[derive(Prop, Clone, Copy, Debug, PartialEq)]
pub enum ChartKind {
    Bar,
    Line,
    Pie,
}

/// What a chart shows: a label for each point, and one or more series of values, each
/// with a name and a value for each label.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartData {
    pub labels: Vec<String>,
    pub series: Vec<(String, Vec<f64>)>,
}

/// "1 label", "3 labels".
fn count(n: usize, singular: &str, plural: &str) -> String {
    format!("{n} {}", if n == 1 { singular } else { plural })
}

fn number(text: &str) -> Result<f64, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("A value is missing".to_string());
    }
    let lower = text.to_ascii_lowercase();
    match text.parse::<f64>() {
        Ok(value) if value.is_nan() || lower.contains("inf") => Err(format!("\"{text}\" isn't a number")),
        Ok(value) if !value.is_finite() || value.abs() > MAX_VALUE => Err(format!("\"{text}\" is too large for a chart")),
        Ok(value) => Ok(value),
        Err(_) => Err(format!("\"{text}\" isn't a number")),
    }
}

/// The items of a list written `a, b, c`. Nothing written is no items.
fn list(text: &str) -> Vec<&str> {
    if text.trim().is_empty() { Vec::new() } else { text.split(',').map(str::trim).collect() }
}

/// One series from labels and values written in the tag.
pub fn parse_inline(labels: &str, values: &str) -> Result<ChartData, String> {
    let (labels, values) = (list(labels), list(values));
    if labels.is_empty() && values.is_empty() {
        return Err(NO_DATA.to_string());
    }
    if labels.len() != values.len() {
        return Err(format!("The chart has {} and {}", count(labels.len(), "label", "labels"), count(values.len(), "value", "values")));
    }
    if labels.len() > MAX_POINTS {
        return Err(format!("The chart has {} points, and can show at most {MAX_POINTS}", labels.len()));
    }
    let values = values.into_iter().map(number).collect::<Result<Vec<_>, _>>()?;
    Ok(ChartData { labels: labels.into_iter().map(String::from).collect(), series: vec![(String::new(), values)] })
}

/// The records of CSV `text`, with blank lines left out. A field in double quotes can hold
/// commas, line breaks and `""` for a quote. It stops at the limits, so that a large file
/// can't take a lot of memory.
fn csv_records(text: &str) -> Result<Vec<Vec<String>>, String> {
    let too_many_rows = || format!("The chart's data has more than {MAX_POINTS} rows, and a chart can show at most {MAX_POINTS}");
    let too_many_series = || format!("The chart's data has more than {MAX_SERIES} series, and a chart can show at most {MAX_SERIES}");
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let (mut quoted, mut was_quoted) = (false, false);
    let mut chars = text.strip_prefix('\u{feff}').unwrap_or(text).chars().peekable();

    let end_field = |record: &mut Vec<String>, field: &mut String, was_quoted: &mut bool| -> Result<(), String> {
        let value = std::mem::take(field);
        record.push(if *was_quoted { value } else { value.trim().to_string() });
        *was_quoted = false;
        // A label column, and a column for each series.
        if record.len() > MAX_SERIES + 1 { Err(too_many_series()) } else { Ok(()) }
    };
    let end_record = |records: &mut Vec<Vec<String>>, record: &mut Vec<String>| -> Result<(), String> {
        let done = std::mem::take(record);
        if !(done.len() == 1 && done[0].is_empty()) {
            records.push(done);
        }
        // A header, and a row for each point.
        if records.len() > MAX_POINTS + 1 { Err(too_many_rows()) } else { Ok(()) }
    };

    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if !was_quoted && field.trim().is_empty() => {
                field.clear();
                quoted = true;
                was_quoted = true;
            }
            ',' => end_field(&mut record, &mut field, &mut was_quoted)?,
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                end_field(&mut record, &mut field, &mut was_quoted)?;
                end_record(&mut records, &mut record)?;
            }
            // What follows a closing quote, before the comma, is left out.
            _ if was_quoted => {}
            _ => field.push(c),
        }
    }
    if !field.is_empty() || was_quoted || !record.is_empty() {
        end_field(&mut record, &mut field, &mut was_quoted)?;
        end_record(&mut records, &mut record)?;
    }
    Ok(records)
}

/// A series for each column after the first, which holds the labels. The first row names them.
pub fn parse_csv(text: &str) -> Result<ChartData, String> {
    let records = csv_records(text)?;
    let Some((header, rows)) = records.split_first() else {
        return Err(NO_DATA.to_string());
    };
    if rows.is_empty() {
        return Err(NO_DATA.to_string());
    }
    if header.len() < 2 {
        return Err("The chart's data needs a column of labels and at least one column of numbers".to_string());
    }
    let mut series: Vec<(String, Vec<f64>)> = header[1..].iter().map(|name| (name.clone(), Vec::with_capacity(rows.len()))).collect();
    let mut labels = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        if row.len() != header.len() {
            return Err(format!(
                "Row {} of the data has {}, and the header names {}",
                index + 2,
                count(row.len() - 1, "value", "values"),
                count(header.len() - 1, "series", "series"),
            ));
        }
        labels.push(row[0].clone());
        for (column, cell) in series.iter_mut().zip(&row[1..]) {
            column.1.push(number(cell)?);
        }
    }
    Ok(ChartData { labels, series })
}

fn to_hsl(color: Color) -> (f32, f32, f32) {
    let [r, g, b] = [color.r, color.g, color.b].map(|channel| channel as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let lightness = (max + min) / 2.0;
    let chroma = max - min;
    if chroma == 0.0 {
        return (0.0, 0.0, lightness);
    }
    let saturation = chroma / (1.0 - (2.0 * lightness - 1.0).abs());
    let hue = if max == r {
        60.0 * ((g - b) / chroma).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / chroma + 2.0)
    } else {
        60.0 * ((r - g) / chroma + 4.0)
    };
    (hue, saturation, lightness)
}

fn from_hsl(hue: f32, saturation: f32, lightness: f32, alpha: u8) -> Color {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue.rem_euclid(360.0) / 60.0;
    let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    let channel = |value: f32| ((value + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Color { r: channel(r), g: channel(g), b: channel(b), a: alpha }
}

/// The colour of series `index`: the accent for the first, then the accent's hue turned
/// 47° for each one after it.
pub fn series_color(theme: &Theme, index: usize) -> Color {
    if index == 0 {
        return theme.accent;
    }
    let (hue, saturation, lightness) = to_hsl(theme.accent);
    let turn = (index % 360) as f32 * HUE_TURN;
    from_hsl(hue + turn, saturation, lightness, theme.accent.a)
}

/// `color`, `share` of the way from `under` to it.
fn tint(color: Color, under: Color, share: f32) -> Color {
    let mix = |a: u8, b: u8| (b as f32 + (a as f32 - b as f32) * share).round() as u8;
    Color::rgb(mix(color.r, under.r), mix(color.g, under.g), mix(color.b, under.b))
}

/// The plot's left edge, top, width and height on a chart `width` by `height`.
pub fn plot_area(width: f32, height: f32, title: bool, legend: bool) -> (f32, f32, f32, f32) {
    let top = if title { TITLE_ROOM } else { 0.0 } + TOP_PADDING;
    let bottom = height - LABEL_ROOM - if legend { LEGEND_ROOM } else { 0.0 };
    (TICK_ROOM, top, (width - RIGHT_PADDING - TICK_ROOM).max(1.0), (bottom - top).max(1.0))
}

/// The chart's size: the width of its box, or 1200, by the height of its box, or else 9/16
/// of the width, held to 675.
fn chart_size(ctx: &Ctx) -> (f32, f32) {
    let given = |value: f32| (value.is_finite() && value > 0.0).then(|| value.min(MAX_SIZE));
    let width = given(ctx.width()).unwrap_or(DEFAULT_WIDTH);
    let height = given(ctx.height()).unwrap_or((width * 9.0 / 16.0).min(MAX_DEFAULT_HEIGHT));
    (width, height)
}

fn shortened(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_string();
    }
    let mut short: String = text.chars().take(most - 1).collect();
    short.push('…');
    short
}

/// A guess at how wide `text` is, at `size`.
fn text_width(text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * CHAR_WIDTH * size
}

fn label(text: impl Into<String>, color: Color) -> Element {
    toolkit::text(text).size(TEXT_SIZE).fill(color)
}

/// A small square with round corners, for a legend.
fn swatch(color: Color) -> Element {
    match shape(ShapeKind::Rounded, SWATCH, SWATCH) {
        Element::Shape { kind, width, height, outline, outline_width, padding, child, .. } => {
            Element::Shape { kind, width, height, fill: Some(color), outline, outline_width, radius: 6.0, padding, child }
        }
        other => other,
    }
}

/// The SVG number for a coordinate: every coordinate a chart writes is finite and on the canvas.
fn n(value: f32) -> String {
    let value = if value.is_finite() { value.clamp(-MAX_SIZE, 2.0 * MAX_SIZE) } else { 0.0 };
    format!("{}", (value * 100.0).round() / 100.0)
}

/// A line drawn in the coordinates of a canvas `width` by `height`, placed at its corner.
fn stroked(data: String, (width, height): (f32, f32), color: Color, line_width: f32) -> Placed {
    at(0.0, 0.0, path(data, width, height).stroke(color, line_width))
}

/// `element` at (`x`, `y`), laid out `width` wide.
fn at_width(x: f32, y: f32, width: f32, element: Element) -> Placed {
    Placed { x, y, width, align: TextAlign::Left, element }
}

/// A value axis: ticks at whole multiples of `step`, from `low` to `high`, with 0 among them.
#[derive(Clone, Debug, PartialEq)]
pub struct Axis {
    pub low: f64,
    pub high: f64,
    pub step: f64,
    /// The ticks, as whole multiples of `step`.
    first: i64,
    last: i64,
    /// The decimals that write a tick exactly.
    decimals: usize,
}

impl Axis {
    /// The ticks' values, lowest first.
    pub fn ticks(&self) -> Vec<f64> {
        (self.first..=self.last).map(|index| index as f64 * self.step).collect()
    }

    /// The ticks as they are written, without the noise of floating point.
    pub fn labels(&self) -> Vec<String> {
        self.ticks()
            .into_iter()
            .map(|value| {
                let text = format!("{value:.*}", self.decimals);
                // 2.5, 5 and 7.5 rather than 2.5, 5.0 and 7.5.
                let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.') } else { &text };
                // So that a tick a hair below 0 isn't written "-0".
                if text == "-0" { "0".to_string() } else { text.to_string() }
            })
            .collect()
    }
}

/// The axis for values from `smallest` to `largest`: the smallest step of 1, 2, 2.5 or 5
/// times a power of ten that covers them, and 0, in at most 6 intervals.
pub fn axis_for(smallest: f64, largest: f64) -> Axis {
    let finite = |value: f64| if value.is_finite() { value } else { 0.0 };
    let (low, high) = (finite(smallest).min(0.0), finite(largest).max(0.0));
    // With nothing, or next to nothing, to show, the axis runs to 1.
    let (low, high) = if high - low < 1e-290 { (0.0, 1.0) } else { (low, high) };
    let start = ((high - low) / MAX_INTERVALS as f64).log10().floor() as i32 - 1;
    for exponent in start..start + 3 {
        for (mantissa, extra) in [(1.0, 0), (2.0, 0), (2.5, 1), (5.0, 0)] {
            let step = mantissa * 10f64.powi(exponent);
            let first = (low / step + 1e-9).floor();
            let last = (high / step - 1e-9).ceil();
            if last - first <= MAX_INTERVALS as f64 {
                let decimals = (extra - exponent).max(0) as usize;
                return Axis { low: first * step, high: last * step, step, first: first as i64, last: last as i64, decimals };
            }
        }
    }
    // Not reached: a step ten times the range's sixth always fits.
    Axis { low: 0.0, high: 1.0, step: 1.0, first: 0, last: 1, decimals: 0 }
}

/// What the chart draws on, and where its plot is.
struct Plot {
    width: f32,
    height: f32,
    left: f32,
    top: f32,
    plot_width: f32,
    plot_height: f32,
    low: f64,
    high: f64,
}

impl Plot {
    fn y_of(&self, value: f64) -> f32 {
        let share = ((self.high - value) / (self.high - self.low)).clamp(0.0, 1.0);
        self.top + self.plot_height * share as f32
    }

    fn bottom(&self) -> f32 {
        self.top + self.plot_height
    }

    fn slot(&self, points: usize) -> f32 {
        self.plot_width / points as f32
    }
}

/// Gridlines and their labels, and the line at 0.
fn axes(plot: &Plot, axis: &Axis, labels: &[String], theme: &Theme, items: &mut Vec<Placed>) {
    let grid = tint(theme.text, theme.background, 0.18);
    let mut lines = String::new();
    for (value, tick) in axis.ticks().into_iter().zip(labels) {
        let y = plot.y_of(value);
        lines.push_str(&format!("M{} {} L{} {} ", n(plot.left), n(y), n(plot.left + plot.plot_width), n(y)));
        let text = label(tick.clone(), tint(theme.text, theme.background, 0.8));
        let room = plot.left - LABEL_MARGIN;
        items.push(at(0.0, y - TEXT_SIZE * LINE_HEIGHT / 2.0, text).right_aligned_in(room));
    }
    items.push(stroked(lines, (plot.width, plot.height), grid, GRID_WIDTH));
    let zero = plot.y_of(0.0);
    let axis = format!("M{} {} L{} {}", n(plot.left), n(zero), n(plot.left + plot.plot_width), n(zero));
    let color = tint(theme.text, theme.background, 0.6);
    items.push(stroked(axis, (plot.width, plot.height), color, AXIS_WIDTH));
}

/// Which labels to show below the plot, as `(index, left edge, width)`: every `k`th, for
/// the smallest `k` that keeps them inside the chart and clear of each other.
fn label_places(labels: &[String], plot: &Plot) -> Vec<(usize, f32, f32)> {
    let points = labels.len();
    let slot = plot.slot(points);
    let widest = labels.iter().map(|label| text_width(label, TEXT_SIZE)).fold(0.0, f32::max);
    let room = (widest + LABEL_GAP).min(plot.width);
    let places = |every: usize| -> Option<Vec<(usize, f32, f32)>> {
        let mut places = Vec::new();
        let mut end = f32::NEG_INFINITY;
        for index in (0..points).step_by(every) {
            let centre = plot.left + (index as f32 + 0.5) * slot;
            let left = (centre - room / 2.0).clamp(0.0, (plot.width - room).max(0.0));
            if left < end {
                return None;
            }
            end = left + room;
            places.push((index, left, room));
        }
        Some(places)
    };
    let first = ((room / slot).ceil() as usize).clamp(1, points);
    (first..=points).find_map(places).unwrap_or_default()
}

fn category_labels(labels: &[String], plot: &Plot, theme: &Theme, items: &mut Vec<Placed>) {
    let labels: Vec<String> = labels.iter().map(|label| shortened(label, MAX_LABEL_CHARS)).collect();
    for (index, left, room) in label_places(&labels, plot) {
        items.push(at(left, plot.bottom() + LABEL_MARGIN, label(labels[index].clone(), theme.text)).centered_in(room));
    }
}

/// A row of swatches and series names along the bottom, as many as fit.
fn legend(names: &[String], width: f32, height: f32, theme: &Theme, items: &mut Vec<Placed>) {
    let names: Vec<String> = names.iter().map(|name| shortened(name, MAX_LEGEND_CHARS)).collect();
    let item_width = |name: &str| SWATCH + SWATCH_GAP + text_width(name, TEXT_SIZE);
    let mut shown = Vec::new();
    let mut total = 0.0;
    for (index, name) in names.iter().enumerate() {
        let more = format!("+{} more", names.len() - index - 1);
        let needed = total + item_width(name) + if index + 1 < names.len() { LEGEND_ITEM_GAP + text_width(&more, TEXT_SIZE) } else { 0.0 };
        if needed > width && !shown.is_empty() {
            break;
        }
        total += item_width(name) + LEGEND_ITEM_GAP;
        shown.push(index);
    }
    let hidden = names.len() - shown.len();
    let more = format!("+{hidden} more");
    if hidden > 0 {
        total += text_width(&more, TEXT_SIZE);
    } else {
        total -= LEGEND_ITEM_GAP;
    }
    let text_height = TEXT_SIZE * LINE_HEIGHT;
    let y = height - LEGEND_ROOM + (LEGEND_ROOM - text_height) / 2.0;
    let mut x = ((width - total) / 2.0).max(0.0);
    for index in shown {
        items.push(at(x, y + (text_height - SWATCH) / 2.0, swatch(series_color(theme, index))));
        x += SWATCH + SWATCH_GAP;
        let room = text_width(&names[index], TEXT_SIZE) + LABEL_GAP;
        items.push(at_width(x, y, room, label(names[index].clone(), theme.text)));
        x += room - LABEL_GAP + LEGEND_ITEM_GAP;
    }
    if hidden > 0 {
        let room = text_width(&more, TEXT_SIZE) + LABEL_GAP;
        items.push(at(x, y, label(more, theme.text)).centered_in(room));
    }
}

fn title_item(title: &str, width: f32, theme: &Theme) -> Placed {
    let text = toolkit::text(title).size(TITLE_SIZE).bold().fill(theme.text);
    at(0.0, (TITLE_ROOM - TITLE_SIZE * LINE_HEIGHT) / 2.0, text).centered_in(width)
}

/// How much of the data is drawn: the series, and in each the points, up to these counts.
struct Shown {
    series: usize,
    points: usize,
}

fn bars(data: &ChartData, plot: &Plot, shown: &Shown, theme: &Theme, items: &mut Vec<Placed>) {
    let slot = plot.slot(data.labels.len());
    let bar_width = slot * GROUP_SHARE / data.series.len() as f32;
    let zero = plot.y_of(0.0);
    for (series, (_, values)) in data.series.iter().enumerate().take(shown.series) {
        let color = series_color(theme, series);
        for (index, value) in values.iter().enumerate().take(shown.points) {
            let y = plot.y_of(*value);
            let (top, height) = (y.min(zero), (y - zero).abs());
            // A shape with no height would be given a size of its own.
            if height > 0.0 && bar_width > 0.0 {
                let x = plot.left + index as f32 * slot + slot * (1.0 - GROUP_SHARE) / 2.0 + series as f32 * bar_width;
                items.push(at(x, top, shape(ShapeKind::Rect, bar_width, height).fill(color)));
            }
        }
    }
}

fn lines(data: &ChartData, plot: &Plot, shown: &Shown, theme: &Theme, items: &mut Vec<Placed>) {
    let slot = plot.slot(data.labels.len());
    for (series, (_, values)) in data.series.iter().enumerate().take(shown.series) {
        let color = series_color(theme, series);
        let points: Vec<(f32, f32)> = values
            .iter()
            .take(shown.points)
            .enumerate()
            .map(|(index, value)| (plot.left + (index as f32 + 0.5) * slot, plot.y_of(*value)))
            .collect();
        if points.len() > 1 {
            let data: Vec<String> = points
                .iter()
                .enumerate()
                .map(|(index, (x, y))| format!("{}{} {}", if index == 0 { "M" } else { "L" }, n(*x), n(*y)))
                .collect();
            items.push(stroked(data.join(" "), (plot.width, plot.height), color, LINE_WIDTH));
        }
        if slot >= POINT_ROOM {
            for (x, y) in points {
                let point = shape(ShapeKind::Ellipse, POINT_SIZE, POINT_SIZE).fill(color);
                items.push(at(x - POINT_SIZE / 2.0, y - POINT_SIZE / 2.0, point));
            }
        }
    }
}

/// How large a share is, for a pie's legend.
fn percent(share: f64) -> String {
    match share * 100.0 {
        0.0 => "0%".to_string(),
        value if value < 1.0 => "<1%".to_string(),
        value => format!("{}%", value.round()),
    }
}

fn pie(data: &ChartData, title: Option<&str>, size: (f32, f32), shown: usize, theme: &Theme) -> Element {
    let (width, height) = size;
    let (labels, values) = (&data.labels, &data.series[0].1);
    if let Some((index, value)) = values.iter().enumerate().find(|(_, value)| **value < 0.0) {
        return problem(format!("A pie chart can't show a value below 0, and {} is {value}", shortened(&labels[index], MAX_LABEL_CHARS)));
    }
    let total: f64 = values.iter().sum();
    if total <= 0.0 {
        return problem("A pie chart needs at least one value above 0");
    }
    let mut items = Vec::new();
    let top = if let Some(title) = title {
        items.push(title_item(title, width, theme));
        TITLE_ROOM
    } else {
        0.0
    };
    let area = (height - top).max(1.0);
    let entries: Vec<String> = labels
        .iter()
        .zip(values)
        .map(|(label, value)| format!("{}  {}", shortened(label, MAX_LEGEND_CHARS), percent(value / total)))
        .collect();
    let rows_that_fit = ((area / PIE_ROW).floor() as usize).max(1);
    let (listed, more) = if entries.len() <= rows_that_fit { (entries.len(), None) } else { (rows_that_fit - 1, Some(entries.len() + 1 - rows_that_fit)) };
    let widest = entries.iter().take(listed).map(|entry| text_width(entry, TEXT_SIZE)).fold(0.0, f32::max).max(text_width("and 999 more", TEXT_SIZE));
    let legend_width = (SWATCH + SWATCH_GAP + widest + LABEL_GAP).min(width * 0.4);
    let diameter = (area - 2.0 * TOP_PADDING).min(width - legend_width - PIE_GAP).max(1.0);
    let left = ((width - diameter - PIE_GAP - legend_width) / 2.0).max(0.0);
    let radius = diameter / 2.0;
    let (cx, cy) = (left + radius, top + area / 2.0);

    let edged = values.len() <= MAX_EDGED_SLICES;
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (index, value) in values.iter().enumerate().take(shown) {
        let share = value / total;
        let color = series_color(theme, index);
        let end = angle + share * std::f64::consts::TAU;
        if share >= 1.0 - 1e-9 {
            items.push(at(cx - radius, cy - radius, shape(ShapeKind::Ellipse, diameter, diameter).fill(color)));
        } else if share > 0.0 {
            let point = |angle: f64| (cx + radius * angle.cos() as f32, cy + radius * angle.sin() as f32);
            let ((x0, y0), (x1, y1)) = (point(angle), point(end));
            let large = if share > 0.5 { 1 } else { 0 };
            let r = n(radius);
            let data = format!("M{} {} L{} {} A{r} {r} 0 {large} 1 {} {} Z", n(cx), n(cy), n(x0), n(y0), n(x1), n(y1));
            let slice = path(data, width, height).fill(color);
            let slice = if edged { slice.stroke(theme.background, SLICE_EDGE) } else { slice };
            items.push(at(0.0, 0.0, slice));
        }
        angle = end;
    }

    let rows = listed + usize::from(more.is_some());
    let text_height = TEXT_SIZE * LINE_HEIGHT;
    let legend_left = left + diameter + PIE_GAP;
    let mut y = cy - rows as f32 * PIE_ROW / 2.0 + (PIE_ROW - text_height) / 2.0;
    let room = (legend_width - SWATCH - SWATCH_GAP).max(1.0);
    for (index, entry) in entries.iter().enumerate().take(listed) {
        items.push(at(legend_left, y + (text_height - SWATCH) / 2.0, swatch(series_color(theme, index))));
        items.push(at_width(legend_left + SWATCH + SWATCH_GAP, y, room, label(entry.clone(), theme.text)));
        y += PIE_ROW;
    }
    if let Some(more) = more {
        items.push(at_width(legend_left + SWATCH + SWATCH_GAP, y, room, label(format!("and {more} more"), theme.text)));
    }
    canvas(width, height, items)
}

fn bar_or_line(kind: ChartKind, data: &ChartData, title: Option<&str>, size: (f32, f32), shown: &Shown, theme: &Theme) -> Element {
    let (width, height) = size;
    let several = data.series.len() > 1;
    let (left, top, plot_width, plot_height) = plot_area(width, height, title.is_some(), several);
    let values = data.series.iter().flat_map(|(_, values)| values.iter().copied());
    let (smallest, largest) = values.fold((0.0f64, 0.0f64), |(low, high), value| (low.min(value), high.max(value)));
    let axis = axis_for(smallest, largest);
    let ticks = axis.labels();
    // Wider than usual for long tick labels, up to a third of the chart.
    let widest = ticks.iter().map(|tick| text_width(tick, TEXT_SIZE) + 2.0 * LABEL_MARGIN).fold(left, f32::max);
    let wider = widest.min(width / 3.0).max(left) - left;
    let (left, plot_width) = (left + wider, (plot_width - wider).max(1.0));
    let plot = Plot { width, height, left, top, plot_width, plot_height, low: axis.low, high: axis.high };
    let mut items = Vec::new();
    if let Some(title) = title {
        items.push(title_item(title, width, theme));
    }
    axes(&plot, &axis, &ticks, theme, &mut items);
    match kind {
        ChartKind::Line => lines(data, &plot, shown, theme, &mut items),
        _ => bars(data, &plot, shown, theme, &mut items),
    }
    category_labels(&data.labels, &plot, theme, &mut items);
    if several {
        let names: Vec<String> = data.series.iter().map(|(name, _)| name.clone()).collect();
        legend(&names, width, height, theme, &mut items);
    }
    canvas(width, height, items)
}

/// The chart's data: from the file if it names one, or else from the tag.
fn read_data(labels: &str, values: &str, data: Option<&DataFile>, ctx: &Ctx) -> Result<ChartData, String> {
    let Some(DataFile(path)) = data else {
        return parse_inline(labels, values);
    };
    let path = path.trim();
    if !asset_path_allowed(path) {
        return Err(format!("A chart's data has to be a file in the talk's assets folder, and {path} isn't"));
    }
    let text = ctx.asset_text(path).ok_or_else(|| format!("Can't read {path}"))?;
    parse_csv(&text)
}

/// A bar, line or pie chart of `labels` and `values`, or of the CSV file `data`. With `build`,
/// it comes in a series per click, or for one series a bar, point or slice per click.
#[component]
pub fn Chart(kind: ChartKind, labels: String, values: String, data: Option<DataFile>, title: Option<String>, build: bool, ctx: &Ctx) -> Element {
    let data = match read_data(&labels, &values, data.as_ref(), ctx) {
        Ok(data) => data,
        Err(message) => return problem(message),
    };
    let title = title.as_deref().map(str::trim).filter(|title| !title.is_empty());
    let size = chart_size(ctx);
    let theme = ctx.theme();
    // A pie shows the first series only.
    let series = if kind == ChartKind::Pie { 1 } else { data.series.len() };
    let points = data.labels.len();
    let shown = match (build, series > 1) {
        (false, _) => Shown { series, points },
        (true, true) => Shown { series: ctx.steps(series as u32) as usize, points },
        (true, false) => Shown { series, points: ctx.steps(points as u32) as usize },
    };
    match kind {
        ChartKind::Pie => pie(&data, title, size, shown.points, theme),
        _ => bar_or_line(kind, &data, title, size, &shown, theme),
    }
}
