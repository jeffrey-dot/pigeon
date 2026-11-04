//! Optional sparse cell text (protocol 1.0 additive extension, ADR-0012).

use serde::{Deserialize, Serialize};

pub const MAX_CELL_TEXT_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellText {
    /// Screen row; negative snapshot rows count backwards from the newest history row.
    pub row: i32,
    pub col: u16,
    pub text: String,
}
