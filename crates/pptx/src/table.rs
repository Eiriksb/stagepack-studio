//! Native tables. A table carries each cell's own borders, margins and anchor on the built-in
//! "No Style, No Grid" table style, so how it looks comes from the talk and not from a style.
//! `table_styles_xml` is the deck's `tableStyles.xml`, with theme colors only, for tables a
//! person inserts later in PowerPoint.

use slides_engine::{NativeParagraph, Rect};
use toolkit::Color;

use crate::text::{BodyOptions, TextCtx, tx_body_xml};
use crate::xml::{NS_A, XML_DECL, emu_from_units, guid, solid_fill_of};

pub const TABLE_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/table";

/// The built-in "No Style, No Grid" table style.
const NO_STYLE: &str = "{2D5ABB26-0587-4C30-8999-92F81FD0307C}";

/// What the engine draws: a cell's rule is one slide unit wide, and its text is kept 12 clear.
const BORDER_UNITS: f32 = 1.0;
const CELL_MARGIN_UNITS: f32 = 12.0;

fn tc_bdr(width: u32) -> String {
    let ln = format!("<a:ln w=\"{width}\" cmpd=\"sng\"><a:solidFill><a:schemeClr val=\"accent4\"/></a:solidFill></a:ln>");
    let sides: String = ["left", "right", "top", "bottom", "insideH", "insideV"].iter().map(|s| format!("<a:{s}>{ln}</a:{s}>")).collect();
    format!("<a:tcBdr>{sides}</a:tcBdr>")
}

fn tc_tx_style(color: &str, bold: bool) -> String {
    let b = if bold { " b=\"on\"" } else { "" };
    format!("<a:tcTxStyle{b}><a:fontRef idx=\"minor\"><a:prstClr val=\"black\"/></a:fontRef><a:schemeClr val=\"{color}\"/></a:tcTxStyle>")
}

pub fn table_styles_xml() -> String {
    let default = guid("deck-table-default");
    let deflist = guid("deck-table-deflist");
    // Header row: panel (bg2) fill with bold accentDark (tx2) text; borders: border color (accent4).
    let deck = format!(
        "<a:tblStyle styleId=\"{default}\" styleName=\"Deck Table\">\
         <a:wholeTbl>{}<a:tcStyle>{}<a:fill><a:noFill/></a:fill></a:tcStyle></a:wholeTbl>\
         <a:firstRow>{}<a:tcStyle><a:tcBdr/><a:fill><a:solidFill><a:schemeClr val=\"bg2\"/></a:solidFill></a:fill></a:tcStyle></a:firstRow>\
         </a:tblStyle>",
        tc_tx_style("tx1", false),
        tc_bdr(12700),
        tc_tx_style("tx2", true),
    );
    let list = format!(
        "<a:tblStyle styleId=\"{deflist}\" styleName=\"Deck Definition List\">\
         <a:wholeTbl>{}<a:tcStyle>{}<a:fill><a:solidFill><a:schemeClr val=\"bg2\"/></a:solidFill></a:fill></a:tcStyle></a:wholeTbl>\
         </a:tblStyle>",
        tc_tx_style("tx1", false),
        tc_bdr(6350),
    );
    // def = the deck style, so tables people insert later in PowerPoint match the deck too.
    format!("{XML_DECL}<a:tblStyleLst xmlns:a=\"{NS_A}\" def=\"{default}\">{deck}{list}</a:tblStyleLst>")
}

fn cell_xml(paragraphs: &[NativeParagraph], line: Color, ctx: &mut TextCtx) -> String {
    // Cell bodies are written like placeholder bodies: a bare <a:bodyPr/>, no wrap or inset.
    let body = tx_body_xml(paragraphs, ctx, &BodyOptions { placeholder: true, ..BodyOptions::default() }, "a:txBody");
    let m = emu_from_units(CELL_MARGIN_UNITS);
    let w = emu_from_units(BORDER_UNITS);
    let rule = |tag: &str| if w > 0 { format!("<a:{tag} w=\"{w}\">{}</a:{tag}>", solid_fill_of(line)) } else { String::new() };
    // CT_TableCellProperties child order: lnL, lnR, lnT, lnB, ..., fill.
    format!(
        "<a:tc>{body}<a:tcPr marL=\"{m}\" marR=\"{m}\" marT=\"{m}\" marB=\"{m}\" anchor=\"t\">{}{}{}{}<a:noFill/></a:tcPr></a:tc>",
        rule("lnL"),
        rule("lnR"),
        rule("lnT"),
        rule("lnB"),
    )
}

/// A table's `a:tbl`. `rows` hold each cell's paragraphs; `column_widths` and `row_heights` are
/// in slide units, and when there aren't as many as there are columns and rows, `rect` is shared
/// out evenly. A row with too few cells is filled out, as a row with fewer `a:tc` than grid
/// columns is a repair prompt.
pub fn table_xml(
    rows: &[Vec<Vec<NativeParagraph>>],
    column_widths: &[f32],
    row_heights: &[f32],
    rect: Rect,
    line: Color,
    ctx: &mut TextCtx,
) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let width = |i: usize| if column_widths.len() == columns { column_widths[i] } else { rect.w / columns as f32 };
    // A row's height is its minimum: rows still grow to fit their text.
    let height = |i: usize| if row_heights.len() == rows.len() { row_heights[i] } else { rect.h / rows.len().max(1) as f32 };
    let empty = [NativeParagraph { runs: Vec::new(), align: slides_engine::Align::Left, level: 0, bullet: slides_engine::Bullet::None }];
    let grid: String = (0..columns).map(|i| format!("<a:gridCol w=\"{}\"/>", emu_from_units(width(i)).max(0))).collect();
    let body: String = rows
        .iter()
        .enumerate()
        .map(|(r, row)| {
            let cells: String = (0..columns).map(|c| cell_xml(row.get(c).map_or(&empty[..], Vec::as_slice), line, ctx)).collect();
            format!("<a:tr h=\"{}\">{cells}</a:tr>", emu_from_units(height(r)).max(0))
        })
        .collect();
    format!("<a:tbl><a:tblPr bandRow=\"0\"><a:tableStyleId>{NO_STYLE}</a:tableStyleId></a:tblPr><a:tblGrid>{grid}</a:tblGrid>{body}</a:tbl>")
}
