// Lists the files of a mod's source tree, skipping build output, IDE folders and tests.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Directory names never worth reading: build output, caches, IDE and run folders.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".gradle",
    ".idea",
    ".vscode",
    ".architectury-transformer",
    "build",
    "out",
    "bin",
    "run",
    "runs",
    "target",
    "node_modules",
    // Gradle build logic, not the mod.
    "buildSrc",
];

/// One file of the tree.
pub(crate) struct TreeFile {
    /// Path relative to the root, with `/` separators.
    pub rel: String,
    pub abs: PathBuf,
}

impl TreeFile {
    /// The file name, the last part of the path.
    pub fn name(&self) -> &str {
        self.rel.rsplit('/').next().unwrap_or(&self.rel)
    }
}

/// Every file under `root`, sorted by relative path.
pub(crate) fn collect(root: &Path) -> Result<Vec<TreeFile>> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        let entries = fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?;
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                let skipped = SKIP_DIRS.contains(&name.as_str()) && !inside_src(&rel);
                if skipped || is_test_dir(&rel, &name) {
                    continue;
                }
                stack.push((entry.path(), child_rel));
            } else if file_type.is_file() {
                out.push(TreeFile {
                    rel: child_rel,
                    abs: entry.path(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

/// Inside `src/`, folder names are Java packages and resource paths (Create has a
/// package named `target`), not build output.
fn inside_src(rel: &str) -> bool {
    rel.split('/').any(|s| s == "src")
}

/// `src/test` and similar source sets hold tests, not the mod.
fn is_test_dir(parent_rel: &str, name: &str) -> bool {
    let parent_is_src = parent_rel == "src" || parent_rel.ends_with("/src");
    parent_is_src && (name == "test" || name == "testmod" || name == "gametest")
}
