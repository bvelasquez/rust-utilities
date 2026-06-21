use std::path::{Path, PathBuf};

pub fn find_git_repos(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    if !root.is_dir() {
        return repos;
    }

    fn walk(dir: &Path, depth: usize, max_depth: usize, repos: &mut Vec<PathBuf>) {
        if depth > max_depth {
            return;
        }
        if dir.join(".git").exists() {
            repos.push(dir.to_path_buf());
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            walk(&path, depth + 1, max_depth, repos);
        }
    }

    if root.join(".git").exists() {
        repos.push(root.to_path_buf());
        return repos;
    }

    let Ok(entries) = std::fs::read_dir(root) else {
        return repos;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        walk(&path, 1, max_depth, &mut repos);
    }

    repos.sort();
    repos.dedup();
    repos
}
