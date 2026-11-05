//! Validates raw OSC 8 before VTE discards embedded C0 controls or repairs invalid UTF-8.
//! Other VT sequences pass through; this filter never interprets screen state.

use super::hyperlink::{valid_uri, MAX_URI_BYTES};

#[derive(Default)]
enum State {
    #[default]
    Ground,
    Escape,
    Probe,
    Eight,
    Link,
    LinkEscape,
    Osc,
    Intermediate,
}

#[derive(Default)]
pub(super) struct Osc8Filter {
    state: State,
    body: Vec<u8>,
    invalid: bool,
    next_id: u32,
}

impl Osc8Filter {
    pub(super) fn filter(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len());
        for &byte in bytes {
            match self.state {
                State::Ground => {
                    out.push(byte);
                    if byte == 0x1b {
                        self.state = State::Escape;
                    }
                }
                State::Escape => self.escape(byte, &mut out),
                State::Probe => {
                    if matches!(byte, 0x00..=0x06 | 0x08..=0x17 | 0x19 | 0x1c..=0x1f) {
                        self.invalid = true;
                    } else if byte == b'8' {
                        self.state = State::Eight;
                    } else {
                        out.push(b']');
                        self.pass_osc(byte, &mut out);
                    }
                }
                State::Eight => {
                    if matches!(byte, 0x00..=0x06 | 0x08..=0x17 | 0x19 | 0x1c..=0x1f) {
                        self.invalid = true;
                    } else if byte == b';' {
                        self.body.clear();
                        self.state = State::Link;
                    } else if byte == 0x07 || byte == 0x1b {
                        self.body.clear();
                        self.invalid = true;
                        if byte == 0x1b {
                            self.state = State::LinkEscape;
                        } else {
                            self.finish(&mut out);
                            self.state = State::Ground;
                        }
                    } else {
                        out.extend_from_slice(b"]8");
                        self.pass_osc(byte, &mut out);
                    }
                }
                State::Link => match byte {
                    0x07 => {
                        self.finish(&mut out);
                        self.state = State::Ground;
                    }
                    0x1b => self.state = State::LinkEscape,
                    0x18 | 0x1a => {
                        self.invalid = true;
                        self.finish(&mut out);
                        out.push(byte);
                        self.state = State::Ground;
                    }
                    _ => {
                        if byte < 0x20 || byte == 0x7f || self.body.len() >= MAX_URI_BYTES + 1024 {
                            self.invalid = true;
                        } else if !self.invalid {
                            self.body.push(byte);
                        }
                    }
                },
                State::LinkEscape => {
                    if byte == b'\\' {
                        self.finish(&mut out);
                        self.state = State::Ground;
                    } else {
                        self.invalid = true;
                        self.finish(&mut out);
                        out.push(0x1b);
                        self.escape(byte, &mut out);
                    }
                }
                State::Osc => self.pass_osc(byte, &mut out),
                State::Intermediate => {
                    out.push(byte);
                    if byte == 0x1b {
                        self.state = State::Escape;
                    } else if (0x30..=0x7e).contains(&byte) || byte == 0x18 || byte == 0x1a {
                        self.state = State::Ground;
                    }
                }
            }
        }
        out
    }

    fn escape(&mut self, byte: u8, out: &mut Vec<u8>) {
        match byte {
            b']' => {
                self.invalid = false;
                self.state = State::Probe;
            }
            0x20..=0x2f => {
                out.push(byte);
                self.state = State::Intermediate;
            }
            0x00..=0x17 | 0x19 | 0x1b..=0x1f | 0x7f..=0xff => {
                out.push(byte);
            }
            _ => {
                out.push(byte);
                self.state = State::Ground;
            }
        }
    }

    fn pass_osc(&mut self, byte: u8, out: &mut Vec<u8>) {
        out.push(byte);
        self.state = match byte {
            0x07 | 0x18 | 0x1a => State::Ground,
            0x1b => State::Escape,
            _ => State::Osc,
        };
    }

    fn finish(&mut self, out: &mut Vec<u8>) {
        // ESC was already forwarded when the sequence began. Invalid input closes the
        // previous link too, so following text cannot accidentally inherit its target.
        let uri = std::str::from_utf8(&self.body)
            .ok()
            .and_then(|body| body.split_once(';').map(|(_, uri)| uri));
        if let Some(uri) = uri.filter(|uri| !self.invalid && valid_uri(uri)) {
            if let Some(id) = self.next_id.checked_add(1) {
                self.next_id = id;
                out.extend_from_slice(format!("]8;id=pigeon-{id};{uri}\x1b\\").as_bytes());
                return;
            }
        }
        out.extend_from_slice(b"]8;;\x1b\\");
    }
}
