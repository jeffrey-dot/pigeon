//! Resolves each frame independently: cells own their URI, never a mutable global table.
use crate::{
    proto::Link,
    term::{
        hyperlink::{valid_uri, MAX_LINKS, MAX_LINK_BYTES},
        Cell,
    },
};
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
    let mut table = BTreeMap::new();
    let mut bytes = 0usize;
    let links = links.unwrap_or_default();
    if links.len() > MAX_LINKS {
        return None;
    }
    for link in links {
        bytes = bytes.checked_add(link.uri.len())?;
        if link.id == 0
            || !valid_uri(&link.uri)
            || bytes > MAX_LINK_BYTES
            || table
                .insert(link.id, Arc::<str>::from(link.uri.as_str()))
                .is_some()
        {
            return None;
        }
    }
    for cell in cells {
        cell.hyperlink = table.get(&cell.link).cloned();
        // A missing reference is inert, even if another frame used the same number.
        if cell.hyperlink.is_none() {
            cell.link = 0;
        }
    }
    Some(())
}
