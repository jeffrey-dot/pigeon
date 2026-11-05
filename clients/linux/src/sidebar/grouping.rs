//! Pure local-project grouping; the caller resolves git roots from the filesystem.
use std::path::Path;

pub fn project(cwd: &str, git_root: Option<&str>) -> (String, String) {
    let path = git_root.unwrap_or(cwd).trim_end_matches('/');
    let key = if path.is_empty() { "/" } else { path };
    let name = Path::new(key)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Local");
    (key.to_owned(), name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn git_subdirectories_share_a_group_but_same_names_do_not() {
        assert_eq!(
            project("/src/a/sub", Some("/src/a")),
            project("/src/a", Some("/src/a"))
        );
        assert_ne!(project("/src/a", None).0, project("/other/a", None).0);
        assert_eq!(project("/src/a/sub/", None).1, "sub");
    }
}
