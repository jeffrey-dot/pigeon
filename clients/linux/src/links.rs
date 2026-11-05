//! Explicit OSC 8 opening policy; terminal metadata is never executable UI markup.
use gtk4::glib::{Uri, UriFlags};

#[cfg(test)]
mod tests;

pub fn opening_uri(raw: &str) -> Option<&str> {
    if raw.is_empty() || raw.len() > 2048 || raw.contains('\\') || raw.chars().any(forbidden) {
        return None;
    }
    let mut decoded = Vec::with_capacity(raw.len());
    let mut bytes = raw.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = char::from(bytes.next()?).to_digit(16)?;
            let low = char::from(bytes.next()?).to_digit(16)?;
            decoded.push((high * 16 + low) as u8);
        } else {
            decoded.push(byte);
        }
    }
    let decoded = std::str::from_utf8(&decoded).ok()?;
    if decoded.contains('\\') || decoded.chars().any(|c| c != ' ' && forbidden(c)) {
        return None;
    }
    let uri = Uri::parse(raw, UriFlags::ENCODED).ok()?;
    if !matches!(uri.scheme().to_ascii_lowercase().as_str(), "http" | "https")
        || uri.userinfo().is_some()
        || uri.host().is_none_or(|s| s.is_empty())
        || !(uri.port() == -1 || (1..=65535).contains(&uri.port()))
    {
        return None;
    }
    let authority = raw.split_once("://")?.1.split(['/', '?', '#']).next()?;
    if authority.is_empty()
        || authority.ends_with(':')
        || authority.contains('@')
        || authority.contains('%')
    {
        return None;
    }
    Some(raw)
}
fn forbidden(c: char) -> bool {
    c.is_control()
        || c.is_whitespace()
        || matches!(c, '\u{00ad}' | '\u{0600}'..='\u{0605}' | '\u{061c}' | '\u{06dd}' | '\u{070f}' | '\u{0890}'..='\u{0891}' | '\u{08e2}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}' | '\u{110bd}' | '\u{110cd}' | '\u{13430}'..='\u{1343f}' | '\u{1bca0}'..='\u{1bca3}' | '\u{1d173}'..='\u{1d17a}' | '\u{e0001}' | '\u{e0020}'..='\u{e007f}')
}
#[derive(Clone, PartialEq, Eq)]
pub struct LinkTarget {
    pub pane: u32,
    pub revision: u64,
    pub column: u16,
    pub row: u16,
    pub uri: String,
}
#[derive(Default)]
pub struct LinkPress {
    target: Option<LinkTarget>,
    pub consumed: bool,
}
impl LinkPress {
    pub fn begin(&mut self, target: Option<LinkTarget>) {
        self.consumed = target.is_some();
        self.target = target;
    }
    pub fn cancel(&mut self) {
        self.target = None;
    }
    pub fn validate(&mut self, target: Option<&LinkTarget>) {
        if self.target.as_ref() != target {
            self.cancel();
        }
    }
    pub fn target(&self) -> Option<&LinkTarget> {
        self.target.as_ref()
    }
    pub fn finish(&mut self, target: Option<&LinkTarget>, control: bool, open: impl FnOnce(&str)) {
        if control && self.target.as_ref().is_some_and(|t| Some(t) == target) {
            if let Some(uri) = self.target.as_ref().and_then(|t| opening_uri(&t.uri)) {
                open(uri);
            }
        }
        self.target = None;
        self.consumed = false;
    }
}
