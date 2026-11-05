//! Resolves each frame independently: cells own their URI, never a mutable global table.
use crate::{proto::Link, term::Cell};
use std::{collections::BTreeMap, sync::Arc};

pub(crate) fn collect<'a>(cells: impl Iterator<Item = &'a Cell>) -> Option<Vec<Link>> {
    let mut links = BTreeMap::new();
    for cell in cells {
        if let Some(uri) = &cell.hyperlink {
            if cell.link != 0 {
                links.entry(cell.link).or_insert_with(|| uri.to_string());
            }
        }
    }
    (!links.is_empty()).then(|| {
        links
            .into_iter()
            .map(|(id, uri)| Link { id, uri })
            .collect()
    })
}

pub(super) fn apply<'a>(
    cells: impl Iterator<Item = &'a mut Cell>,
    links: Option<&[Link]>,
) -> Option<()> {
    let links = links.unwrap_or_default();
    crate::proto::links::validate_table(links).ok()?;
    let table: BTreeMap<_, _> = links.iter()
        .map(|link| (link.id, Arc::<str>::from(link.uri.as_str())))
        .collect();
    for cell in cells {
        cell.hyperlink = table.get(&cell.link).cloned();
        // A missing reference is inert, even if another frame used the same number.
        if cell.hyperlink.is_none() {
            cell.link = 0;
        }
    }
    Some(())
}
