//! Generation-bound text query contract; see docs/text-query.md.

use crate::error::{PgError, PgResult};
use serde::{Deserialize, Serialize};

pub const MAX_QUERY_BYTES: usize = 256;
pub const MAX_TEXT_RESULTS: u16 = 128;
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_SCAN_CELLS: usize = 131_072;
pub const MAX_SCAN_ROWS: usize = 128;

/// A UTF-8 boundary inside a cell, or col=cols, byte=0 at a row boundary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TextPoint {
    pub row: i32,
    pub col: u16,
    pub byte: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRange {
    pub start: TextPoint,
    pub end: TextPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextInfo {
    pub generation: u64,
    pub first_row: i32,
    pub last_row: i32,
    pub cols: u16,
    pub alt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextQuery {
    Info,
    Search {
        /// Zero starts a fresh query; continuation must carry the returned generation.
        generation: u64,
        pattern: String,
        case_sensitive: bool,
        max_results: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start: Option<TextPoint>,
    },
    Viewport {
        generation: u64,
        range: TextRange,
    },
    Range {
        generation: u64,
        range: TextRange,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextResult {
    Info {
        info: TextInfo,
    },
    Search {
        info: TextInfo,
        hits: Vec<TextRange>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next: Option<TextPoint>,
    },
    Viewport {
        info: TextInfo,
        range: TextRange,
        viewport: Box<crate::proto::text_viewport::TextViewport>,
    },
    Range {
        info: TextInfo,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next: Option<TextPoint>,
    },
}

impl TextQuery {
    pub fn validate(&self) -> PgResult<()> {
        match self {
            Self::Search {
                generation,
                pattern,
                max_results,
                start,
                ..
            } => {
                if pattern.is_empty()
                    || pattern.len() > MAX_QUERY_BYTES
                    || pattern.chars().any(|ch| ch.is_control() && ch != '\n')
                    || !(1..=MAX_TEXT_RESULTS).contains(max_results)
                    || (*generation == 0 && start.is_some())
                {
                    return Err(PgError::InvalidArg);
                }
            }
            Self::Range { generation, range } | Self::Viewport { generation, range } => {
                if *generation == 0 || range.start > range.end {
                    return Err(PgError::InvalidArg);
                }
            }
            Self::Info => {}
        }
        Ok(())
    }
}

impl TextResult {
    pub fn info(&self) -> TextInfo {
        match self {
            Self::Info { info }
            | Self::Search { info, .. }
            | Self::Range { info, .. }
            | Self::Viewport { info, .. } => *info,
        }
    }

    pub fn next(&self) -> Option<TextPoint> {
        match self {
            Self::Info { .. } | Self::Viewport { .. } => None,
            Self::Search { next, .. } | Self::Range { next, .. } => *next,
        }
    }
}
