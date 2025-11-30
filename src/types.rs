use similar::ChangeTag;
use syntect::highlighting::Style;

#[derive(Clone, PartialEq)]
pub struct FileChange {
    pub path: String,
    pub status: String,
}

#[derive(PartialEq)]
pub struct DiffLine {
    pub old_num: Option<u32>,
    pub new_num: Option<u32>,
    pub tag: ChangeTag,
    pub content: String,
    pub highlighted: Option<Vec<(Style, String)>>,
}

#[derive(PartialEq)]
pub struct DiffHunk {
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, PartialEq)]
pub struct CommitInfo {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
    pub author: String,
    pub is_local_changes: bool,
}
