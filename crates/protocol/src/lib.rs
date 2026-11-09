//! Wire types for the Studio editor bridge (version 1).
//!
//! The page sends [`PageMessage`]s to the app; the app sends [`Command`]s to
//! the page. Both carry `"v":1` on the wire.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const BRIDGE_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("Unsupported bridge version: {0}")]
    Version(u64),
    #[error("Bad message: {0}")]
    Bad(String),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DragPos {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: Option<f64>,
    pub rotate: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SelectionInfo {
    pub slide: u32,
    pub range: Option<[u32; 2]>,
    pub kind: String,
    pub tag: Option<String>,
    pub label: String,
    pub positioned: bool,
    pub nested: bool,
    pub pos: Option<DragPos>,
    #[serde(rename = "canReorder")]
    pub can_reorder: bool,
}

/// A message from the page. Serialising directly omits `v`; use
/// [`PageMessage::to_json`] for the wire form.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PageMessage {
    Ready {
        studio: String,
        slide: u32,
        total: u32,
    },
    Selection {
        selection: Option<SelectionInfo>,
        /// The last click hit the slide but nothing traceable. Only true with
        /// no selection.
        missed: bool,
    },
    History {
        #[serde(rename = "canUndo")]
        can_undo: bool,
        #[serde(rename = "canRedo")]
        can_redo: bool,
        #[serde(rename = "lastAction")]
        last_action: Option<String>,
    },
    Edited {
        slide: u32,
        label: String,
    },
    Slide {
        slide: u32,
        total: u32,
    },
    Error {
        message: String,
    },
    Result {
        id: i64,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

impl PageMessage {
    /// The wire form, with `"v":1`.
    pub fn to_json(&self) -> String {
        #[derive(Serialize)]
        struct Wire<'a> {
            v: u32,
            #[serde(flatten)]
            msg: &'a PageMessage,
        }
        serde_json::to_string(&Wire {
            v: BRIDGE_VERSION,
            msg: self,
        })
        .expect("page message serialises")
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum CommandBody {
    SetPos {
        pos: DragPos,
    },
    FreePosition,
    ReturnToFlow,
    Delete,
    Duplicate,
    Undo,
    Redo,
    Deselect,
    Go {
        slide: u32,
    },
    /// Apply the page's inline editor if it is open, and answer once the write
    /// has landed.
    FinishEdit,
}

/// A command for the page: `{"v":1,"id":…,"cmd":…,…}`.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub id: i64,
    pub body: CommandBody,
}

#[derive(Serialize)]
struct CommandOut<'a> {
    v: u32,
    id: i64,
    #[serde(flatten)]
    body: &'a CommandBody,
}

#[derive(Deserialize)]
struct CommandIn {
    v: u64,
    id: i64,
    #[serde(flatten)]
    body: CommandBody,
}

impl Serialize for Command {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        CommandOut {
            v: BRIDGE_VERSION,
            id: self.id,
            body: &self.body,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for Command {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = CommandIn::deserialize(d)?;
        if raw.v != u64::from(BRIDGE_VERSION) {
            return Err(serde::de::Error::custom(format!(
                "Unsupported bridge version: {}",
                raw.v
            )));
        }
        Ok(Command {
            id: raw.id,
            body: raw.body,
        })
    }
}

/// Parse a message from the page, checking `v` first.
pub fn parse_page_message(json: &str) -> Result<PageMessage, ProtocolError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| ProtocolError::Bad(e.to_string()))?;
    let obj = value
        .as_object()
        .ok_or_else(|| ProtocolError::Bad("not an object".into()))?;
    match obj.get("v") {
        None => return Err(ProtocolError::Bad("missing v".into())),
        Some(v) => match v.as_u64() {
            Some(n) if n == u64::from(BRIDGE_VERSION) => {}
            Some(n) => return Err(ProtocolError::Version(n)),
            None => return Err(ProtocolError::Bad(format!("bad v: {v}"))),
        },
    }
    serde_json::from_value(value).map_err(|e| ProtocolError::Bad(e.to_string()))
}

/// The script that hands a command to the page's bridge.
pub fn command_script(cmd: &Command) -> String {
    let json = serde_json::to_string(cmd).expect("command serialises");
    let literal = serde_json::to_string(&json).expect("string serialises");
    format!("window.__studioBridge.dispatch({literal})")
}
