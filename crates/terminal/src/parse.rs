//! Reading what a program writes, a character at a time, into text, controls and sequences.
//! It keeps nothing but its state and a bounded list of parameters: a string sequence's
//! content is dropped as it's read, so one that never ends costs nothing.

/// The most parameters a control sequence keeps; any more are read and dropped.
pub(crate) const MOST_PARAMS: usize = 32;
/// What a character read means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Nothing yet, or nothing at all.
    None,
    /// A character to put on the screen.
    Print(char),
    /// A control character, such as a line feed.
    Control(char),
    /// An escape sequence ended by `last`, with the intermediate before it if there was one.
    Escape { intermediate: u8, last: u8 },
    /// A control sequence.
    Csi(Csi),
}

/// A control sequence as read: its parameters, the private marker and intermediate if it had
/// them, and its last character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Csi {
    params: [u16; MOST_PARAMS],
    /// How many parameters were kept.
    len: u8,
    /// Which parameters followed a colon rather than a semicolon, a bit each.
    joined: u32,
    pub(crate) private: u8,
    pub(crate) intermediate: u8,
    pub(crate) last: u8,
}

impl Csi {
    const EMPTY: Csi = Csi { params: [0; MOST_PARAMS], len: 0, joined: 0, private: 0, intermediate: 0, last: 0 };

    /// The parameters kept.
    pub(crate) fn params(&self) -> &[u16] {
        &self.params[..usize::from(self.len)]
    }

    /// The parameter at `index`, or `default` where it's missing or nought.
    pub(crate) fn param(&self, index: usize, default: u16) -> u16 {
        match self.params().get(index) {
            Some(0) | None => default,
            Some(param) => *param,
        }
    }

    /// Whether the parameter at `index` followed a colon, as part of the one before it.
    pub(crate) fn joined(&self, index: usize) -> bool {
        index < MOST_PARAMS && self.joined & (1 << index) != 0
    }
}

/// Where the parser is in what it's reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    /// After an escape.
    Escape,
    /// After an escape and an intermediate, waiting for the sequence's last character.
    EscapeIntermediate(u8),
    /// Within a control sequence.
    Csi,
    /// Within a control sequence that can't mean anything, until its last character.
    CsiIgnored,
    /// Within a string sequence; an operating system command can end with the bell.
    String { osc: bool },
}

/// Reads characters into actions.
#[derive(Clone, Debug)]
pub(crate) struct Parser {
    state: State,
    csi: Csi,
    /// How many parameters have been started, kept or not, up to one more than are kept.
    started: usize,
}

impl Default for Parser {
    fn default() -> Self {
        Parser { state: State::Ground, csi: Csi::EMPTY, started: 0 }
    }
}

/// Two parsers are the same if what they've read means the same: what's left of a sequence
/// already ended doesn't count.
impl PartialEq for Parser {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state && (self.state != State::Csi || (self.csi == other.csi && self.started == other.started))
    }
}

impl Parser {
    /// Reads one character.
    pub(crate) fn advance(&mut self, c: char) -> Action {
        // These mean the same wherever they come.
        match c {
            // Cancel and substitute end any sequence.
            '\u{18}' | '\u{1a}' => {
                self.state = State::Ground;
                return Action::None;
            }
            '\u{1b}' => {
                self.state = State::Escape;
                return Action::None;
            }
            // The eight-bit introducers and string terminator, other than within a string,
            // where all but the terminator are its content.
            '\u{9b}' if !self.in_string() => return self.start_csi(),
            '\u{9d}' if !self.in_string() => return self.start_string(true),
            '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' if !self.in_string() => return self.start_string(false),
            '\u{9c}' => {
                self.state = State::Ground;
                return Action::None;
            }
            _ => {}
        }
        match self.state {
            State::Ground => {
                if is_control(c) {
                    Action::Control(c)
                } else {
                    Action::Print(c)
                }
            }
            State::Escape => match c {
                '[' => self.start_csi(),
                ']' => self.start_string(true),
                'P' | 'X' | '^' | '_' => self.start_string(false),
                ' '..='/' => {
                    self.state = State::EscapeIntermediate(c as u8);
                    Action::None
                }
                '0'..='~' => {
                    self.state = State::Ground;
                    Action::Escape { intermediate: 0, last: c as u8 }
                }
                c if is_control(c) => Action::Control(c),
                _ => {
                    self.state = State::Ground;
                    Action::None
                }
            },
            State::EscapeIntermediate(intermediate) => match c {
                ' '..='/' => Action::None,
                '0'..='~' => {
                    self.state = State::Ground;
                    Action::Escape { intermediate, last: c as u8 }
                }
                c if is_control(c) => Action::Control(c),
                _ => {
                    self.state = State::Ground;
                    Action::None
                }
            },
            State::Csi => self.csi(c),
            State::CsiIgnored => match c {
                '@'..='~' => {
                    self.state = State::Ground;
                    Action::None
                }
                c if is_control(c) => Action::Control(c),
                _ => Action::None,
            },
            State::String { osc } => {
                if osc && c == '\u{7}' {
                    self.state = State::Ground;
                }
                Action::None
            }
        }
    }

    fn in_string(&self) -> bool {
        matches!(self.state, State::String { .. })
    }

    fn start_csi(&mut self) -> Action {
        self.state = State::Csi;
        self.csi = Csi::EMPTY;
        self.started = 0;
        Action::None
    }

    fn start_string(&mut self, osc: bool) -> Action {
        self.state = State::String { osc };
        Action::None
    }

    fn csi(&mut self, c: char) -> Action {
        let csi = &mut self.csi;
        match c {
            '0'..='9' => {
                if csi.intermediate != 0 {
                    self.state = State::CsiIgnored;
                    return Action::None;
                }
                if self.started == 0 {
                    self.started = 1;
                    csi.len = 1;
                }
                if self.started <= MOST_PARAMS {
                    let param = &mut csi.params[self.started - 1];
                    let digit = c as u16 - u16::from(b'0');
                    // A parameter larger than 65,535 is 65,535.
                    *param = param.saturating_mul(10).saturating_add(digit);
                }
                Action::None
            }
            ';' | ':' => {
                if csi.intermediate != 0 {
                    self.state = State::CsiIgnored;
                    return Action::None;
                }
                // An empty first parameter is a parameter too.
                if self.started == 0 {
                    self.started = 1;
                    csi.len = 1;
                }
                if self.started <= MOST_PARAMS {
                    self.started += 1;
                    if self.started <= MOST_PARAMS {
                        csi.len = self.started as u8;
                        if c == ':' {
                            csi.joined |= 1 << (self.started - 1);
                        }
                    }
                }
                Action::None
            }
            '<'..='?' => {
                // A private marker comes first or not at all.
                if self.started == 0 && csi.private == 0 && csi.intermediate == 0 {
                    csi.private = c as u8;
                } else {
                    self.state = State::CsiIgnored;
                }
                Action::None
            }
            ' '..='/' => {
                if csi.intermediate == 0 {
                    csi.intermediate = c as u8;
                } else {
                    // More than one intermediate means nothing here.
                    self.state = State::CsiIgnored;
                }
                Action::None
            }
            '@'..='~' => {
                self.state = State::Ground;
                csi.last = c as u8;
                Action::Csi(*csi)
            }
            c if is_control(c) => Action::Control(c),
            '\u{7f}' => Action::None,
            _ => {
                self.state = State::CsiIgnored;
                Action::None
            }
        }
    }
}

/// The C0 controls, delete, and the C1 controls.
fn is_control(c: char) -> bool {
    matches!(c, '\0'..='\u{1f}' | '\u{7f}'..='\u{9f}')
}
