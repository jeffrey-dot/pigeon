//! Validation of the additive text_query capability (docs/text-query.md).

use crate::error::{PgError, PgResult};
use crate::term::text_query::{TextInfo, TextPoint, TextResult, MAX_TEXT_BYTES, MAX_TEXT_RESULTS};

pub fn validate_result(result: &TextResult) -> PgResult<()> {
    let info = result.info();
    if info.generation == 0
        || info.first_row > 0
        || info.last_row < 0
        || info.cols < 2
        || (info.alt && info.first_row != 0)
    {
        return Err(PgError::InvalidArg);
    }
    if let Some(next) = result.next() {
        point(info, next)?;
    }
    match result {
        TextResult::Search { hits, .. } => {
            if hits.len() > usize::from(MAX_TEXT_RESULTS) {
                return Err(PgError::InvalidArg);
            }
            for hit in hits {
                point(info, hit.start)?;
                point(info, hit.end)?;
                if hit.start >= hit.end {
                    return Err(PgError::InvalidArg);
                }
            }
            if hits.windows(2).any(|pair| pair[0].start >= pair[1].start) {
                return Err(PgError::InvalidArg);
            }
        }
        TextResult::Range { text, .. } => {
            if text.len() > MAX_TEXT_BYTES || text.chars().any(|ch| ch.is_control() && ch != '\n') {
                return Err(PgError::InvalidArg);
            }
        }
        TextResult::Viewport {
            range, viewport, ..
        } => {
            point(info, range.start)?;
            point(info, range.end)?;
            if range.start > range.end
                || viewport.rows == 0
                || i32::from(viewport.rows) - 1 != info.last_row
                || viewport.offset != range.start.row.min(0).unsigned_abs()
                || viewport.history_len != info.first_row.unsigned_abs()
                || viewport.offset > viewport.history_len
            {
                return Err(PgError::InvalidArg);
            }
            let message = viewport.message(info);
            super::links::validate(&message)?;
            crate::session::snapshot_from_message(&message).ok_or(PgError::InvalidArg)?;
        }
        TextResult::Info { .. } => {}
    }
    Ok(())
}

fn point(info: TextInfo, point: TextPoint) -> PgResult<()> {
    if point.row < info.first_row
        || point.row > info.last_row
        || point.col > info.cols
        || (point.col == info.cols && point.byte != 0)
        || point.byte > 4096
    {
        return Err(PgError::InvalidArg);
    }
    Ok(())
}
