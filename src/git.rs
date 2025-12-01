use git2::{DiffOptions, Repository};
use similar::{ChangeTag, TextDiff};

use crate::highlighter::Highlighter;
use crate::types::{BranchInfo, CommitInfo, DiffHunk, DiffLine, FileChange};

pub struct GitDiff {
    repo: Repository,
    staged: bool,
    commit: Option<String>,
    context_lines: usize,
    highlighter: Highlighter,
    current_branch: Option<String>,
}

impl GitDiff {
    pub fn new(staged: bool, commit: Option<String>, context_lines: usize) -> Result<Self, git2::Error> {
        let repo = Repository::discover(".")?;
        let current_branch = repo.head().ok()
            .and_then(|h| h.shorthand().map(|s| s.to_string()));
        Ok(Self {
            repo,
            staged,
            commit,
            context_lines,
            highlighter: Highlighter::new(),
            current_branch,
        })
    }

    pub fn get_current_branch(&self) -> Option<&str> {
        self.current_branch.as_deref()
    }

    pub fn load_branches(&self) -> Result<Vec<BranchInfo>, git2::Error> {
        let mut branches = Vec::new();
        let current = self.current_branch.as_deref();

        for branch in self.repo.branches(Some(git2::BranchType::Local))? {
            let (branch, _) = branch?;
            if let Some(name) = branch.name()? {
                branches.push(BranchInfo {
                    name: name.to_string(),
                    is_current: Some(name) == current,
                    is_remote: false,
                });
            }
        }

        // Sort with current branch first, then alphabetically
        branches.sort_by(|a, b| {
            match (a.is_current, b.is_current) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });

        Ok(branches)
    }

    pub fn load_commits_for_branch(&self, branch_name: &str, limit: usize) -> Result<Vec<CommitInfo>, git2::Error> {
        let mut commits = Vec::new();

        // Only show local changes if on current branch
        if Some(branch_name) == self.current_branch.as_deref() && self.has_local_changes()? {
            commits.push(CommitInfo {
                sha: String::new(),
                short_sha: String::new(),
                message: "Local Changes".to_string(),
                author: String::new(),
                is_local_changes: true,
            });
        }

        // Get commit history for the branch
        let branch = self.repo.find_branch(branch_name, git2::BranchType::Local)?;
        let reference = branch.into_reference();
        let oid = reference.target().ok_or(git2::Error::from_str("No target"))?;

        let mut revwalk = self.repo.revwalk()?;
        revwalk.push(oid)?;

        for oid in revwalk.take(limit) {
            let oid = oid?;
            let commit = self.repo.find_commit(oid)?;
            let message = commit.summary().unwrap_or("").to_string();
            let author = commit.author().name().unwrap_or("").to_string();
            let sha = oid.to_string();
            let short_sha = sha[..7.min(sha.len())].to_string();

            commits.push(CommitInfo {
                sha,
                short_sha,
                message,
                author,
                is_local_changes: false,
            });
        }

        Ok(commits)
    }

    fn has_local_changes(&self) -> Result<bool, git2::Error> {
        let mut diff_opts = DiffOptions::new();
        diff_opts.include_untracked(true);

        let head_tree = self.repo.head().ok().and_then(|h| h.peel_to_tree().ok());

        // Check staged changes
        let staged = self.repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut diff_opts))?;
        if staged.stats()?.files_changed() > 0 {
            return Ok(true);
        }

        // Check unstaged changes
        let unstaged = self.repo.diff_index_to_workdir(None, Some(&mut diff_opts))?;
        if unstaged.stats()?.files_changed() > 0 {
            return Ok(true);
        }

        Ok(false)
    }

    pub fn load_files_for_commit(&self, commit_sha: &str) -> Result<Vec<FileChange>, git2::Error> {
        let mut files = Vec::new();
        let mut diff_opts = DiffOptions::new();

        let commit = self.repo.revparse_single(commit_sha)?.peel_to_commit()?;
        let tree = commit.tree()?;

        // Get parent tree (or empty if first commit)
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

        let diff = self.repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut diff_opts))?;
        self.collect_files_from_diff(&diff, &mut files)?;

        Ok(files)
    }

    pub fn load_diff_for_commit_file(&self, commit_sha: &str, file_path: &str) -> Result<Vec<DiffHunk>, git2::Error> {
        let commit = self.repo.revparse_single(commit_sha)?.peel_to_commit()?;
        let tree = commit.tree()?;
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

        let old_content = parent_tree
            .as_ref()
            .and_then(|t| t.get_path(std::path::Path::new(file_path)).ok())
            .and_then(|entry| self.repo.find_blob(entry.id()).ok())
            .map(|blob| String::from_utf8_lossy(blob.content()).to_string())
            .unwrap_or_default();

        let new_content = tree
            .get_path(std::path::Path::new(file_path))
            .ok()
            .and_then(|entry| self.repo.find_blob(entry.id()).ok())
            .map(|blob| String::from_utf8_lossy(blob.content()).to_string())
            .unwrap_or_default();

        self.compute_diff(file_path, &old_content, &new_content)
    }

    pub fn load_files(&self) -> Result<Vec<FileChange>, git2::Error> {
        let mut diff_opts = DiffOptions::new();
        diff_opts.include_untracked(true);

        let head_tree = self.repo.head().ok().and_then(|h| h.peel_to_tree().ok());

        let mut files = Vec::new();

        if self.staged {
            let diff = self.repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut diff_opts))?;
            self.collect_files_from_diff(&diff, &mut files)?;
        } else if let Some(ref commit_ref) = self.commit {
            let obj = self.repo.revparse_single(commit_ref)?;
            let commit = obj.peel_to_commit()?;
            let tree = commit.tree()?;
            let diff = self.repo.diff_tree_to_workdir_with_index(Some(&tree), Some(&mut diff_opts))?;
            self.collect_files_from_diff(&diff, &mut files)?;
        } else {
            // Default: show both staged AND unstaged changes
            let staged = self.repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut diff_opts))?;
            let unstaged = self.repo.diff_index_to_workdir(None, Some(&mut diff_opts))?;

            for diff in [&staged, &unstaged] {
                diff.foreach(
                    &mut |delta, _| {
                        if let Some(path) = delta.new_file().path().or(delta.old_file().path()) {
                            let path_str = path.to_string_lossy().to_string();
                            if !path_str.starts_with("target/") && !files.iter().any(|f: &FileChange| f.path == path_str) {
                                files.push(FileChange {
                                    path: path_str,
                                    status: Self::delta_to_status(delta.status()),
                                });
                            }
                        }
                        true
                    },
                    None,
                    None,
                    None,
                )?;
            }
        }

        Ok(files)
    }

    fn collect_files_from_diff(&self, diff: &git2::Diff, files: &mut Vec<FileChange>) -> Result<(), git2::Error> {
        diff.foreach(
            &mut |delta, _| {
                if let Some(path) = delta.new_file().path().or(delta.old_file().path()) {
                    let path_str = path.to_string_lossy().to_string();
                    if !path_str.starts_with("target/") {
                        files.push(FileChange {
                            path: path_str,
                            status: Self::delta_to_status(delta.status()),
                        });
                    }
                }
                true
            },
            None,
            None,
            None,
        )?;
        Ok(())
    }

    fn delta_to_status(delta: git2::Delta) -> String {
        match delta {
            git2::Delta::Added => "added".to_string(),
            git2::Delta::Deleted => "deleted".to_string(),
            git2::Delta::Modified => "modified".to_string(),
            _ => "changed".to_string(),
        }
    }

    pub fn load_diff_for_file(&self, file_path: &str) -> Result<Vec<DiffHunk>, git2::Error> {
        let (old_content, new_content) = match self.get_file_contents(file_path) {
            Ok(contents) => contents,
            Err(_) => {
                return Ok(vec![DiffHunk {
                    lines: vec![DiffLine {
                        old_num: None,
                        new_num: Some(1),
                        tag: ChangeTag::Insert,
                        content: "[Unable to read file]".to_string(),
                        highlighted: None,
                    }],
                }]);
            }
        };

        self.compute_diff(file_path, &old_content, &new_content)
    }

    fn compute_diff(&self, file_path: &str, old_content: &str, new_content: &str) -> Result<Vec<DiffHunk>, git2::Error> {
        // Skip binary files
        let binary_extensions = [
            "png", "jpg", "jpeg", "gif", "ico", "pdf", "zip", "tar", "gz", "bin", "exe", "dll",
            "so", "dylib", "o", "a", "class", "jar", "rlib", "rmeta", "d",
        ];
        if let Some(ext) = std::path::Path::new(file_path).extension() {
            if binary_extensions.contains(&ext.to_str().unwrap_or("").to_lowercase().as_str()) {
                return Ok(vec![DiffHunk {
                    lines: vec![DiffLine {
                        old_num: None,
                        new_num: Some(1),
                        tag: ChangeTag::Insert,
                        content: "[Binary file]".to_string(),
                        highlighted: None,
                    }],
                }]);
            }
        }

        // Check if content looks binary
        if old_content.contains('\0') || new_content.contains('\0') {
            return Ok(vec![DiffHunk {
                lines: vec![DiffLine {
                    old_num: None,
                    new_num: Some(1),
                    tag: ChangeTag::Insert,
                    content: "[Binary file]".to_string(),
                    highlighted: None,
                }],
            }]);
        }

        let text_diff = TextDiff::from_lines(old_content, new_content);

        let line_contents: Vec<String> = text_diff
            .iter_all_changes()
            .map(|c| c.value().trim_end_matches('\n').to_string())
            .collect();

        let highlighted = self.highlighter.highlight_lines(file_path, &line_contents);

        let mut all_lines: Vec<DiffLine> = Vec::new();
        let mut old_line = 1u32;
        let mut new_line = 1u32;

        for (idx, change) in text_diff.iter_all_changes().enumerate() {
            let (old_num, new_num) = match change.tag() {
                ChangeTag::Delete => {
                    let n = old_line;
                    old_line += 1;
                    (Some(n), None)
                }
                ChangeTag::Insert => {
                    let n = new_line;
                    new_line += 1;
                    (None, Some(n))
                }
                ChangeTag::Equal => {
                    let (o, n) = (old_line, new_line);
                    old_line += 1;
                    new_line += 1;
                    (Some(o), Some(n))
                }
            };

            all_lines.push(DiffLine {
                old_num,
                new_num,
                tag: change.tag(),
                content: change.value().trim_end_matches('\n').to_string(),
                highlighted: highlighted.get(idx).cloned(),
            });
        }

        Ok(self.extract_hunks(&all_lines))
    }

    fn extract_hunks(&self, lines: &[DiffLine]) -> Vec<DiffHunk> {
        let mut hunks = Vec::new();
        let mut i = 0;
        let ctx = self.context_lines;

        while i < lines.len() {
            if lines[i].tag != ChangeTag::Equal {
                let mut hunk_lines = Vec::new();

                let start = i.saturating_sub(ctx);
                for j in start..i {
                    hunk_lines.push(DiffLine {
                        old_num: lines[j].old_num,
                        new_num: lines[j].new_num,
                        tag: lines[j].tag,
                        content: lines[j].content.clone(),
                        highlighted: lines[j].highlighted.clone(),
                    });
                }

                while i < lines.len() && lines[i].tag != ChangeTag::Equal {
                    hunk_lines.push(DiffLine {
                        old_num: lines[i].old_num,
                        new_num: lines[i].new_num,
                        tag: lines[i].tag,
                        content: lines[i].content.clone(),
                        highlighted: lines[i].highlighted.clone(),
                    });
                    i += 1;
                }

                let end = (i + ctx).min(lines.len());
                for j in i..end {
                    hunk_lines.push(DiffLine {
                        old_num: lines[j].old_num,
                        new_num: lines[j].new_num,
                        tag: lines[j].tag,
                        content: lines[j].content.clone(),
                        highlighted: lines[j].highlighted.clone(),
                    });
                }
                i = end;

                hunks.push(DiffHunk { lines: hunk_lines });
            } else {
                i += 1;
            }
        }

        hunks
    }

    fn get_file_contents(&self, path: &str) -> Result<(String, String), git2::Error> {
        let workdir = self.repo.workdir().unwrap();

        let old_content = if let Some(ref commit_ref) = self.commit {
            let obj = self.repo.revparse_single(commit_ref)?;
            let commit = obj.peel_to_commit()?;
            let tree = commit.tree()?;
            match tree.get_path(std::path::Path::new(path)) {
                Ok(entry) => {
                    let blob = self.repo.find_blob(entry.id())?;
                    String::from_utf8_lossy(blob.content()).to_string()
                }
                Err(_) => String::new(),
            }
        } else {
            self.repo
                .head()
                .ok()
                .and_then(|h| h.peel_to_tree().ok())
                .and_then(|tree| tree.get_path(std::path::Path::new(path)).ok())
                .and_then(|entry| self.repo.find_blob(entry.id()).ok())
                .map(|blob| String::from_utf8_lossy(blob.content()).to_string())
                .unwrap_or_default()
        };

        let new_content = {
            let index_content = self.repo.index().ok().and_then(|index| {
                index
                    .get_path(std::path::Path::new(path), 0)
                    .and_then(|entry| {
                        self.repo
                            .find_blob(entry.id)
                            .ok()
                            .map(|blob| String::from_utf8_lossy(blob.content()).to_string())
                    })
            });

            if self.staged {
                index_content.unwrap_or_default()
            } else {
                let file_path = workdir.join(path);
                std::fs::read_to_string(&file_path)
                    .unwrap_or_else(|_| index_content.unwrap_or_default())
            }
        };

        Ok((old_content, new_content))
    }
}
