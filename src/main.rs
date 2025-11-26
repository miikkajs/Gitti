use clap::Parser;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use git2::{DiffOptions, Repository};
use similar::{ChangeTag, TextDiff};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::easy::HighlightLines;

#[derive(Parser)]
#[command(name = "gitti")]
#[command(about = "Fast git diff viewer with IntelliJ-style output", long_about = None)]
struct Cli {
    /// Show staged changes
    #[arg(long, short)]
    staged: bool,

    /// Compare with specific commit
    #[arg(long, short)]
    commit: Option<String>,

    /// Specific file to diff
    file: Option<PathBuf>,

    /// Context lines around changes (default 5)
    #[arg(long, short = 'C', default_value = "5")]
    context: usize,
}

// ANSI escape codes for IntelliJ Darcula-like theme
// Using 256-color palette for better terminal compatibility
mod ansi {
    pub const RESET: &str = "\x1b[0m";
    pub const CLEAR_LINE: &str = "\x1b[2K";
    
    // Backgrounds - using 256-color for compatibility
    pub const BG_DARK: &str = "\x1b[48;5;236m";       // Dark gray
    pub const BG_ADDED: &str = "\x1b[48;5;235m";      // Slightly lighter dark (we'll use gutter color)
    pub const BG_REMOVED: &str = "\x1b[48;5;235m";    // Same base, rely on text color
    pub const BG_HEADER: &str = "\x1b[48;5;238m";
    pub const BG_SELECTED: &str = "\x1b[48;5;24m";
    pub const BG_PANEL: &str = "\x1b[48;5;235m";
    pub const BG_HUNK: &str = "\x1b[48;5;239m";
    
    // Foregrounds - 256-color palette
    pub const FG_DEFAULT: &str = "\x1b[38;5;252m";
    pub const FG_LINE_NUM: &str = "\x1b[38;5;243m";
    pub const FG_ADDED: &str = "\x1b[38;5;114m";      // Green
    pub const FG_REMOVED: &str = "\x1b[38;5;210m";    // Light red/salmon
    pub const FG_HEADER: &str = "\x1b[38;5;75m";
    pub const FG_SEPARATOR: &str = "\x1b[38;5;240m";
    pub const FG_CONTEXT: &str = "\x1b[38;5;250m";
    pub const FG_DIM: &str = "\x1b[38;5;245m";
    
    pub const BOLD: &str = "\x1b[1m";
}

// Convert RGB to closest 256-color palette index
fn rgb_to_256(r: u8, g: u8, b: u8) -> u8 {
    // Check for grayscale first (where r ≈ g ≈ b)
    if r == g && g == b {
        if r < 8 {
            return 16; // black
        }
        if r > 248 {
            return 231; // white
        }
        return (((r as u16 - 8) / 10) as u8) + 232; // grayscale 232-255
    }
    
    // Convert to 6x6x6 color cube (indices 16-231)
    let r_idx = if r < 48 { 0 } else { ((r as u16 - 35) / 40).min(5) as u8 };
    let g_idx = if g < 48 { 0 } else { ((g as u16 - 35) / 40).min(5) as u8 };
    let b_idx = if b < 48 { 0 } else { ((b as u16 - 35) / 40).min(5) as u8 };
    
    16 + 36 * r_idx + 6 * g_idx + b_idx
}

#[derive(Clone)]
struct FileChange {
    path: String,
    status: String,
}

struct DiffLine {
    old_num: Option<u32>,
    new_num: Option<u32>,
    tag: ChangeTag,
    content: String,
    highlighted: Option<Vec<(Style, String)>>,
}

struct DiffHunk {
    lines: Vec<DiffLine>,
}

struct Highlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl Highlighter {
    fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }
    
    fn highlight_lines(&self, path: &str, lines: &[String]) -> Vec<Vec<(Style, String)>> {
        let extension = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        
        let syntax = self.syntax_set
            .find_syntax_by_extension(extension)
            .or_else(|| self.syntax_set.find_syntax_by_first_line(lines.first().map(|s| s.as_str()).unwrap_or("")))
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        
        let theme = &self.theme_set.themes["base16-eighties.dark"];
        let mut highlighter = HighlightLines::new(syntax, theme);
        
        lines.iter().map(|line| {
            highlighter.highlight_line(line, &self.syntax_set)
                .map(|ranges| ranges.into_iter().map(|(style, text)| (style, text.to_string())).collect())
                .unwrap_or_else(|_| vec![(Style::default(), line.clone())])
        }).collect()
    }
}

struct App {
    files: Vec<FileChange>,
    selected_file: usize,
    diff_hunks: Vec<DiffHunk>,
    scroll_offset: usize,
    term_width: u16,
    term_height: u16,
    left_panel_width: u16,
    repo: Repository,
    staged: bool,
    commit: Option<String>,
    context_lines: usize,
    highlighter: Highlighter,
}

impl App {
    fn new(cli: &Cli) -> Result<Self, git2::Error> {
        let repo = Repository::discover(".")?;
        let (width, height) = terminal::size().unwrap_or((120, 40));
        let left_panel_width = (width / 4).max(25).min(50);
        
        let mut app = App {
            files: Vec::new(),
            selected_file: 0,
            diff_hunks: Vec::new(),
            scroll_offset: 0,
            term_width: width,
            term_height: height,
            left_panel_width,
            repo,
            staged: cli.staged,
            commit: cli.commit.clone(),
            context_lines: cli.context,
            highlighter: Highlighter::new(),
        };
        
        app.load_files()?;
        if !app.files.is_empty() {
            app.load_diff_for_selected()?;
        }
        
        Ok(app)
    }
    
    fn load_files(&mut self) -> Result<(), git2::Error> {
        let mut diff_opts = DiffOptions::new();
        diff_opts.include_untracked(true);
        
        // Get the HEAD tree, or None if repo is empty (no commits yet)
        let head_tree = self.repo.head()
            .ok()
            .and_then(|h| h.peel_to_tree().ok());
        
        let diff = if self.staged {
            // Staged changes: compare HEAD (or empty) to index
            self.repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut diff_opts))?
        } else if let Some(ref commit_ref) = self.commit {
            let obj = self.repo.revparse_single(commit_ref)?;
            let commit = obj.peel_to_commit()?;
            let tree = commit.tree()?;
            self.repo.diff_tree_to_workdir_with_index(Some(&tree), Some(&mut diff_opts))?
        } else {
            // Default: show both staged AND unstaged changes
            // First get staged changes
            let staged = self.repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut diff_opts))?;
            // Then get unstaged changes
            let unstaged = self.repo.diff_index_to_workdir(None, Some(&mut diff_opts))?;
            
            // Collect files from both
            self.files.clear();
            for diff in [&staged, &unstaged] {
                diff.foreach(
                    &mut |delta, _| {
                        if let Some(path) = delta.new_file().path().or(delta.old_file().path()) {
                            let path_str = path.to_string_lossy().to_string();
                            // Skip target directory and avoid duplicates
                            if !path_str.starts_with("target/") && !self.files.iter().any(|f| f.path == path_str) {
                                self.files.push(FileChange {
                                    path: path_str,
                                    status: match delta.status() {
                                        git2::Delta::Added => "added".to_string(),
                                        git2::Delta::Deleted => "deleted".to_string(),
                                        git2::Delta::Modified => "modified".to_string(),
                                        _ => "changed".to_string(),
                                    },
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
            return Ok(());
        };
        
        self.files.clear();
        diff.foreach(
            &mut |delta, _| {
                if let Some(path) = delta.new_file().path().or(delta.old_file().path()) {
                    let path_str = path.to_string_lossy().to_string();
                    // Skip target directory
                    if !path_str.starts_with("target/") {
                        self.files.push(FileChange {
                            path: path_str,
                            status: match delta.status() {
                                git2::Delta::Added => "added".to_string(),
                                git2::Delta::Deleted => "deleted".to_string(),
                                git2::Delta::Modified => "modified".to_string(),
                                _ => "changed".to_string(),
                            },
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
    
    fn load_diff_for_selected(&mut self) -> Result<(), git2::Error> {
        if self.files.is_empty() {
            self.diff_hunks.clear();
            return Ok(());
        }
        
        let file = &self.files[self.selected_file];
        let file_path = file.path.clone();
        
        // Skip binary files (common binary extensions)
        let binary_extensions = ["png", "jpg", "jpeg", "gif", "ico", "pdf", "zip", "tar", "gz", 
                                  "bin", "exe", "dll", "so", "dylib", "o", "a", "class", "jar",
                                  "rlib", "rmeta", "d"];
        if let Some(ext) = std::path::Path::new(&file_path).extension() {
            if binary_extensions.contains(&ext.to_str().unwrap_or("").to_lowercase().as_str()) {
                self.diff_hunks = vec![DiffHunk {
                    lines: vec![DiffLine {
                        old_num: None,
                        new_num: Some(1),
                        tag: ChangeTag::Insert,
                        content: "[Binary file]".to_string(),
                        highlighted: None,
                    }]
                }];
                self.scroll_offset = 0;
                return Ok(());
            }
        }
        
        let (old_content, new_content) = match self.get_file_contents(&file_path) {
            Ok(contents) => contents,
            Err(_) => {
                self.diff_hunks = vec![DiffHunk {
                    lines: vec![DiffLine {
                        old_num: None,
                        new_num: Some(1),
                        tag: ChangeTag::Insert,
                        content: "[Unable to read file]".to_string(),
                        highlighted: None,
                    }]
                }];
                self.scroll_offset = 0;
                return Ok(());
            }
        };
        
        // Check if content looks binary
        if old_content.contains('\0') || new_content.contains('\0') {
            self.diff_hunks = vec![DiffHunk {
                lines: vec![DiffLine {
                    old_num: None,
                    new_num: Some(1),
                    tag: ChangeTag::Insert,
                    content: "[Binary file]".to_string(),
                    highlighted: None,
                }]
            }];
            self.scroll_offset = 0;
            return Ok(());
        }
        
        let text_diff = TextDiff::from_lines(&old_content, &new_content);
        
        // Collect all line contents for highlighting
        let line_contents: Vec<String> = text_diff.iter_all_changes()
            .map(|c| c.value().trim_end_matches('\n').to_string())
            .collect();
        
        // Apply syntax highlighting to all lines
        let highlighted = self.highlighter.highlight_lines(&file_path, &line_contents);
        
        // Collect all lines with their change info
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
        
        // Extract hunks: changed lines + context
        self.diff_hunks = self.extract_hunks(&all_lines);
        self.scroll_offset = 0;
        
        Ok(())
    }
    
    fn extract_hunks(&self, lines: &[DiffLine]) -> Vec<DiffHunk> {
        let mut hunks = Vec::new();
        let mut i = 0;
        let ctx = self.context_lines;
        
        while i < lines.len() {
            // Find next change
            if lines[i].tag != ChangeTag::Equal {
                let mut hunk_lines = Vec::new();
                
                // Add context before
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
                
                // Add all consecutive changes
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
                
                // Add context after
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
        
        // Get old content from HEAD (or empty if new file/no commits)
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
            // Try to get from HEAD, return empty if no HEAD or file not in HEAD
            self.repo.head()
                .ok()
                .and_then(|h| h.peel_to_tree().ok())
                .and_then(|tree| tree.get_path(std::path::Path::new(path)).ok())
                .and_then(|entry| self.repo.find_blob(entry.id()).ok())
                .map(|blob| String::from_utf8_lossy(blob.content()).to_string())
                .unwrap_or_default()
        };
        
        // Get new content - try index first, then workdir
        let new_content = {
            // First try index (for staged files)
            let index_content = self.repo.index().ok().and_then(|index| {
                index.get_path(std::path::Path::new(path), 0).and_then(|entry| {
                    self.repo.find_blob(entry.id).ok().map(|blob| {
                        String::from_utf8_lossy(blob.content()).to_string()
                    })
                })
            });
            
            // If staged flag or found in index, use index content
            if self.staged {
                index_content.unwrap_or_default()
            } else {
                // Otherwise try workdir file
                let file_path = workdir.join(path);
                std::fs::read_to_string(&file_path).unwrap_or_else(|_| {
                    // Fall back to index if file doesn't exist in workdir
                    index_content.unwrap_or_default()
                })
            }
        };
        
        Ok((old_content, new_content))
    }
    
    fn draw(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        execute!(stdout, Clear(ClearType::All))?;
        
        // Draw left panel (file list)
        self.draw_file_panel(stdout)?;
        
        // Draw vertical separator
        self.draw_separator(stdout)?;
        
        // Draw right panel (diff)
        self.draw_diff_panel(stdout)?;
        
        // Draw status bar
        self.draw_status_bar(stdout)?;
        
        stdout.flush()
    }
    
    fn draw_file_panel(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        let panel_width = self.left_panel_width as usize;
        
        // Header
        execute!(stdout, MoveTo(0, 0))?;
        let header = format!(" Changes ({}) ", self.files.len());
        let header_padded = format!("{:<width$}", header, width = panel_width);
        write!(stdout, "{}{}{}{}", ansi::BG_HEADER, ansi::FG_DEFAULT, header_padded, ansi::RESET)?;
        
        // File list
        for (i, file) in self.files.iter().enumerate() {
            if i + 1 >= self.term_height as usize - 1 {
                break;
            }
            
            execute!(stdout, MoveTo(0, (i + 1) as u16))?;
            
            let (icon, color) = match file.status.as_str() {
                "added" => ("+", ansi::FG_ADDED),
                "deleted" => ("-", ansi::FG_REMOVED),
                _ => ("~", ansi::FG_HEADER),
            };
            
            let bg = if i == self.selected_file { ansi::BG_SELECTED } else { ansi::BG_PANEL };
            
            // Truncate filename if too long
            let max_name_len = panel_width.saturating_sub(4);
            let display_name = if file.path.len() > max_name_len {
                format!("…{}", &file.path[file.path.len() - max_name_len + 1..])
            } else {
                file.path.clone()
            };
            
            let line = format!(" {} {:<width$}", icon, display_name, width = max_name_len);
            write!(stdout, "{}{}{}{}", bg, color, line, ansi::RESET)?;
        }
        
        // Fill remaining space
        for i in self.files.len() + 1..self.term_height as usize - 1 {
            execute!(stdout, MoveTo(0, i as u16))?;
            write!(stdout, "{}{:width$}{}", ansi::BG_PANEL, "", ansi::RESET, width = panel_width)?;
        }
        
        Ok(())
    }
    
    fn draw_separator(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        let x = self.left_panel_width;
        for y in 0..self.term_height - 1 {
            execute!(stdout, MoveTo(x, y))?;
            write!(stdout, "{}{}│{}", ansi::BG_DARK, ansi::FG_SEPARATOR, ansi::RESET)?;
        }
        Ok(())
    }
    
    fn draw_diff_panel(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        let start_x = self.left_panel_width + 1;
        let diff_width = (self.term_width - start_x) as usize;
        
        // Header
        execute!(stdout, MoveTo(start_x, 0))?;
        let file_name = if !self.files.is_empty() {
            &self.files[self.selected_file].path
        } else {
            "No files"
        };
        let header = format!(" {} ", file_name);
        let header_padded = format!("{:<width$}", header, width = diff_width);
        write!(stdout, "{}{}{}{}", ansi::BG_HEADER, ansi::FG_HEADER, header_padded, ansi::RESET)?;
        
        if self.diff_hunks.is_empty() {
            execute!(stdout, MoveTo(start_x, 2))?;
            write!(stdout, "{}{}  No changes{}", ansi::BG_DARK, ansi::FG_DIM, ansi::RESET)?;
            return Ok(());
        }
        
        // Flatten hunks for display
        let mut row = 1u16;
        let max_rows = self.term_height - 2;
        let mut line_idx = 0usize;
        
        for (hunk_idx, hunk) in self.diff_hunks.iter().enumerate() {
            if row >= max_rows {
                break;
            }
            
            // Skip to scroll offset
            if line_idx + hunk.lines.len() <= self.scroll_offset {
                line_idx += hunk.lines.len() + 1; // +1 for separator
                continue;
            }
            
            // Hunk separator (except for first)
            if hunk_idx > 0 && line_idx >= self.scroll_offset {
                execute!(stdout, MoveTo(start_x, row))?;
                let sep = format!("{:─<width$}", "─", width = diff_width);
                write!(stdout, "{}{}{}{}", ansi::BG_HUNK, ansi::FG_SEPARATOR, sep, ansi::RESET)?;
                row += 1;
                if row >= max_rows {
                    break;
                }
            }
            line_idx += 1;
            
            for line in &hunk.lines {
                if line_idx < self.scroll_offset {
                    line_idx += 1;
                    continue;
                }
                
                if row >= max_rows {
                    break;
                }
                
                execute!(stdout, MoveTo(start_x, row))?;
                self.draw_diff_line(stdout, line, diff_width)?;
                row += 1;
                line_idx += 1;
            }
        }
        
        // Fill remaining rows
        while row < max_rows {
            execute!(stdout, MoveTo(start_x, row))?;
            write!(stdout, "{}{:width$}{}", ansi::BG_DARK, "", ansi::RESET, width = diff_width)?;
            row += 1;
        }
        
        Ok(())
    }
    
    fn draw_diff_line(&self, stdout: &mut io::Stdout, line: &DiffLine, width: usize) -> io::Result<()> {
        let old_str = line.old_num.map(|n| format!("{:>4}", n)).unwrap_or_else(|| "    ".to_string());
        let new_str = line.new_num.map(|n| format!("{:>4}", n)).unwrap_or_else(|| "    ".to_string());
        
        let content_width = width.saturating_sub(14);
        
        // Build the content string with syntax highlighting
        let mut content = String::new();
        if let Some(ref highlighted) = line.highlighted {
            let mut chars_written = 0;
            for (style, text) in highlighted {
                if chars_written >= content_width {
                    break;
                }
                let remaining = content_width - chars_written;
                let display_text = if text.len() > remaining {
                    &text[..remaining]
                } else {
                    text.as_str()
                };
                let color_code = rgb_to_256(style.foreground.r, style.foreground.g, style.foreground.b);
                content.push_str(&format!("\x1b[38;5;{}m{}", color_code, display_text));
                chars_written += display_text.len();
            }
            // Pad
            if chars_written < content_width {
                content.push_str(&" ".repeat(content_width - chars_written));
            }
        } else {
            if line.content.len() > content_width {
                content = format!("{}…", &line.content[..content_width.saturating_sub(1)]);
            } else {
                content = format!("{:<width$}", line.content, width = content_width);
            }
        }
        
        // Now output the complete line based on change type
        match line.tag {
            ChangeTag::Insert => {
                write!(stdout, "\x1b[48;5;236m\x1b[38;5;243m{} {}\x1b[38;5;240m│\x1b[48;5;22m\x1b[38;5;114m+ {}\x1b[0m", 
                    old_str, new_str, content)?;
            }
            ChangeTag::Delete => {
                write!(stdout, "\x1b[48;5;236m\x1b[38;5;243m{} {}\x1b[38;5;240m│\x1b[48;5;52m\x1b[38;5;210m- {}\x1b[0m", 
                    old_str, new_str, content)?;
            }
            ChangeTag::Equal => {
                write!(stdout, "\x1b[48;5;236m\x1b[38;5;243m{} {}\x1b[38;5;240m│\x1b[48;5;236m\x1b[38;5;250m  {}\x1b[0m", 
                    old_str, new_str, content)?;
            }
        }
        
        Ok(())
    }
    
    fn draw_status_bar(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        execute!(stdout, MoveTo(0, self.term_height - 1))?;
        let status = " ↑↓ Navigate files │ j/k Scroll diff │ q Quit ";
        let status_padded = format!("{:<width$}", status, width = self.term_width as usize);
        write!(stdout, "{}{}{}{}", ansi::BG_HEADER, ansi::FG_DIM, status_padded, ansi::RESET)
    }
    
    fn select_prev_file(&mut self) -> Result<(), git2::Error> {
        if self.selected_file > 0 {
            self.selected_file -= 1;
            self.load_diff_for_selected()?;
        }
        Ok(())
    }
    
    fn select_next_file(&mut self) -> Result<(), git2::Error> {
        if self.selected_file < self.files.len().saturating_sub(1) {
            self.selected_file += 1;
            self.load_diff_for_selected()?;
        }
        Ok(())
    }
    
    fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(3);
    }
    
    fn scroll_down(&mut self) {
        let total_lines: usize = self.diff_hunks.iter().map(|h| h.lines.len() + 1).sum();
        let max_scroll = total_lines.saturating_sub((self.term_height - 3) as usize);
        self.scroll_offset = (self.scroll_offset + 3).min(max_scroll);
    }
}

fn run_app(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new(cli)?;
    
    if app.files.is_empty() {
        println!("No changes detected.");
        return Ok(());
    }
    
    let mut stdout = io::stdout();
    
    // Setup terminal
    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, Hide)?;
    
    // Main loop
    loop {
        app.draw(&mut stdout)?;
        
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Up => { let _ = app.select_prev_file(); }
                    KeyCode::Down => { let _ = app.select_next_file(); }
                    KeyCode::Char('k') | KeyCode::PageUp => app.scroll_up(),
                    KeyCode::Char('j') | KeyCode::PageDown => app.scroll_down(),
                    _ => {}
                }
            }
        }
    }
    
    // Cleanup
    execute!(stdout, Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    
    // Set up panic hook to show backtrace
    std::panic::set_hook(Box::new(|panic_info| {
        // Restore terminal first
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
        
        // Print panic info
        eprintln!("\n\x1b[31m=== Application Crashed ===\x1b[0m\n");
        
        if let Some(location) = panic_info.location() {
            eprintln!("Location: {}:{}:{}", location.file(), location.line(), location.column());
        }
        
        if let Some(message) = panic_info.payload().downcast_ref::<&str>() {
            eprintln!("Message: {}", message);
        } else if let Some(message) = panic_info.payload().downcast_ref::<String>() {
            eprintln!("Message: {}", message);
        }
        
        eprintln!("\nBacktrace:");
        eprintln!("{}", std::backtrace::Backtrace::force_capture());
    }));
    
    // Run the app
    if let Err(e) = run_app(&cli) {
        // Restore terminal on error
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
