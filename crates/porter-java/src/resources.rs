// Counts a mod's resource files (assets and data) by category.

use std::collections::BTreeSet;

use porter_model::ResourceSummary;

/// Folders under `data/<namespace>/` whose next folder is worth its own category,
/// such as `data/tags/block` or `data/worldgen/biome`.
const SPLIT_DATA: &[&str] = &["tags", "tag", "worldgen"];

/// Builds the summary from relative file paths (with `/` separators).
pub(crate) fn summarize<'a>(paths: impl IntoIterator<Item = &'a str>) -> ResourceSummary {
    let mut summary = ResourceSummary::default();
    let mut namespaces = BTreeSet::new();
    let mut roots = BTreeSet::new();
    for path in paths {
        let Some((root, kind, namespace, rest)) = split(path) else {
            continue;
        };
        let category = match rest.split_once('/') {
            None => format!("{kind}/{rest}"),
            Some((dir, deeper)) if kind == "data" && SPLIT_DATA.contains(&dir) => {
                match deeper.split_once('/') {
                    Some((sub, _)) => format!("{kind}/{dir}/{sub}"),
                    None => format!("{kind}/{dir}"),
                }
            }
            Some((dir, _)) => format!("{kind}/{dir}"),
        };
        *summary.categories.entry(category).or_default() += 1;
        namespaces.insert(namespace.to_string());
        roots.insert(if root.is_empty() {
            "(jar root)".to_string()
        } else {
            root.to_string()
        });
    }
    summary.namespaces = namespaces.into_iter().collect();
    summary.roots = roots.into_iter().collect();
    summary
}

/// Splits `src/main/resources/assets/demo/models/block/x.json` into
/// (`src/main/resources`, `assets`, `demo`, `models/block/x.json`). A resource
/// folder is `assets/` or `data/` at the root (a jar) or right under `resources/`.
fn split(path: &str) -> Option<(&str, &str, &str, &str)> {
    let mut search_from = 0;
    loop {
        let rel = &path[search_from..];
        let (offset, kind) = ["assets/", "data/"]
            .iter()
            .filter_map(|k| find_segment(rel, k).map(|i| (i, *k)))
            .min_by_key(|(i, _)| *i)?;
        let start = search_from + offset;
        let root = path[..start].trim_end_matches('/');
        let at_root = start == 0;
        let under_resources = root == "resources" || root.ends_with("/resources");
        if at_root || under_resources {
            let after = &path[start + kind.len()..];
            let (namespace, rest) = after.split_once('/')?;
            if rest.is_empty() {
                return None;
            }
            return Some((root, kind.trim_end_matches('/'), namespace, rest));
        }
        search_from = start + kind.len();
    }
}

/// Index of `segment` in `path` where it starts a path segment.
fn find_segment(path: &str, segment: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = path[from..].find(segment) {
        let at = from + i;
        if at == 0 || path.as_bytes()[at - 1] == b'/' {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_by_category() {
        let s = summarize([
            "src/main/resources/assets/demo/textures/block/a.png",
            "src/main/resources/assets/demo/textures/item/b.png",
            "src/main/resources/assets/demo/sounds.json",
            "src/generated/resources/data/demo/recipe/c.json",
            "src/generated/resources/data/minecraft/tags/block/mineable/pickaxe.json",
            "src/main/java/com/demo/data/Foo.java",
            "assets/demo/lang/en_us.json",
        ]);
        assert_eq!(s.categories["assets/textures"], 2);
        assert_eq!(s.categories["assets/sounds.json"], 1);
        assert_eq!(s.categories["assets/lang"], 1);
        assert_eq!(s.categories["data/recipe"], 1);
        assert_eq!(s.categories["data/tags/block"], 1);
        assert_eq!(s.namespaces, vec!["demo", "minecraft"]);
        assert_eq!(s.assets(), 4);
        assert_eq!(s.data(), 2);
        assert!(!s.categories.keys().any(|k| k.contains("Foo")));
    }
}
