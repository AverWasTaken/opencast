//! Local metadata snapshots and ranked filename/path search. File contents are never read.
use serde::{Deserialize, Serialize};
use std::{
    collections::BinaryHeap,
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub directory: bool,
    pub modified: u64,
    #[serde(skip)]
    searchable: String,
}
impl Entry {
    fn prepare(&mut self) {
        self.searchable = self.path.to_string_lossy().to_lowercase();
    }
}
#[derive(Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub entries: Vec<Entry>,
    pub roots: Vec<PathBuf>,
    pub skipped: usize,
}
impl Snapshot {
    pub fn scan(roots: &[PathBuf]) -> Self {
        let mut snapshot = Self {
            roots: roots.to_vec(),
            ..Self::default()
        };
        let mut seen = std::collections::HashSet::new();
        for root in roots {
            let walker = ignore::WalkBuilder::new(root)
                .hidden(true)
                .git_ignore(false)
                .git_exclude(false)
                .git_global(false)
                .follow_links(false)
                .filter_entry(|e| {
                    !matches!(
                        e.file_name().to_str(),
                        Some(
                            "node_modules"
                                | "target"
                                | "$RECYCLE.BIN"
                                | "System Volume Information"
                                | "AppData"
                                | ".git"
                        )
                    )
                })
                .build();
            for item in walker {
                let item = match item {
                    Ok(item) => item,
                    Err(_) => {
                        snapshot.skipped += 1;
                        continue;
                    }
                };
                if item.depth() == 0 || item.file_type().is_none_or(|t| t.is_symlink()) {
                    continue;
                }
                if !seen.insert(item.path().to_path_buf()) {
                    continue;
                }
                let metadata = match item.metadata() {
                    Ok(m) => m,
                    Err(_) => {
                        snapshot.skipped += 1;
                        continue;
                    }
                };
                let mut entry = Entry {
                    path: item.path().to_path_buf(),
                    name: item.file_name().to_string_lossy().into(),
                    directory: metadata.is_dir(),
                    modified: metadata
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_secs()),
                    searchable: String::new(),
                };
                entry.prepare();
                snapshot.entries.push(entry);
            }
        }
        snapshot.entries.sort_by(|a, b| a.path.cmp(&b.path));
        snapshot
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut snapshot: Self =
            serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        for entry in &mut snapshot.entries {
            entry.prepare();
        }
        Ok(snapshot)
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path.parent().ok_or("Invalid index location")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = path.with_extension("tmp");
        fs::write(
            &temporary,
            serde_json::to_vec(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        // Windows cannot rename over an existing file. The cache is disposable;
        // startup always rebuilds it if interrupted between removal and rename.
        if path.exists() {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        fs::rename(temporary, path).map_err(|e| e.to_string())
    }
    pub fn search(&self, query: &str, limit: usize) -> Vec<Entry> {
        if limit == 0 {
            return vec![];
        }
        let query = query.trim().to_lowercase();
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut best = BinaryHeap::new();
        for (i, entry) in self.entries.iter().enumerate() {
            let name = entry.name.to_lowercase();
            let score = if query.is_empty() || name == query {
                0
            } else if name.starts_with(&query) {
                1
            } else if name.contains(&query) {
                2
            } else if words.iter().all(|w| entry.searchable.contains(w)) {
                3
            } else if query.len() >= 2 && subsequence(&name, &query) {
                4
            } else {
                continue;
            };
            // Lowest score, most recent modification, then stable path order.
            best.push((score, std::cmp::Reverse(entry.modified), i));
            if best.len() > limit {
                best.pop();
            }
        }
        best.into_sorted_vec()
            .into_iter()
            .map(|(_, _, i)| self.entries[i].clone())
            .collect()
    }
}
fn subsequence(haystack: &str, needle: &str) -> bool {
    let mut chars = haystack.chars();
    needle.chars().all(|n| chars.any(|c| c == n))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scan_search_cache_and_deletion() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("node_modules")).unwrap();
        fs::write(tmp.path().join("node_modules/ignored.txt"), "").unwrap();
        for name in ["Budget.xlsx", "Budget notes.txt", "Résumé.pdf", ".secret"] {
            fs::write(tmp.path().join(name), "").unwrap();
        }
        let roots = vec![tmp.path().to_owned(), tmp.path().to_owned()];
        let snapshot = Snapshot::scan(&roots);
        assert_eq!(snapshot.entries.len(), 3);
        assert_eq!(snapshot.search("BUDGET.XLSX", 10)[0].name, "Budget.xlsx");
        assert_eq!(snapshot.search("bdgt", 1).len(), 1);
        assert_eq!(snapshot.search("résumé", 10)[0].name, "Résumé.pdf");
        assert!(snapshot.search("missing", 10).is_empty());
        let cache = tempfile::tempdir().unwrap();
        let path = cache.path().join("index.json");
        snapshot.save(&path).unwrap();
        snapshot.save(&path).unwrap();
        assert_eq!(Snapshot::load(&path).unwrap().search("budget", 10).len(), 2);
        fs::remove_file(tmp.path().join("Budget.xlsx")).unwrap();
        assert_eq!(Snapshot::scan(&roots).entries.len(), 2);
    }
    #[test]
    fn search_benchmark() {
        let mut snapshot = Snapshot::default();
        for i in 0..100_000 {
            let mut entry = Entry {
                path: PathBuf::from(format!("/Documents/project-{i}/report-{i}.pdf")),
                name: format!("report-{i}.pdf"),
                directory: false,
                modified: i,
                searchable: String::new(),
            };
            entry.prepare();
            snapshot.entries.push(entry);
        }
        let start = std::time::Instant::now();
        assert_eq!(
            snapshot.search("report-99999", 30)[0].name,
            "report-99999.pdf"
        );
        println!("100,000-entry search: {:?}", start.elapsed());
    }
}
