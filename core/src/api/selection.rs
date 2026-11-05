//! Snapshot selection shares one core implementation across native clients.
use super::{PgResult, Snapshot};
pub use crate::term::selection::{
    GridPoint, GridSelection, Selection, SelectionSpan, SelectionUnit,
};

pub fn select_grid(snapshot: &Snapshot, selection: &GridSelection) -> PgResult<Selection> {
    crate::term::selection::select(snapshot, selection)
}
