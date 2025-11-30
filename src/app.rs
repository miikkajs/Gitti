use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind},
    execute,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, Write};
use std::time::Instant;

use crate::git::GitDiff;
use crate::types::{CommitInfo, DiffHunk, FileChange};
use crate::ui::Ui;

const REFRESH_INTERVAL_MS: u128 = 1000;
const MAX_COMMITS: usize = 50;

pub struct App {
    commits: Vec<CommitInfo>,
    selected_commit: usize,
    commit_scroll_offset: usize,
    files: Vec<FileChange>,
    selected_file: usize,
    file_scroll_offset: usize,
    diff_hunks: Vec<DiffHunk>,
    scroll_offset: usize,
    git: GitDiff,
    ui: Ui,
    needs_full_redraw: bool,
    mouse_enabled: bool,
    last_refresh: Instant,
}

impl App {
    pub fn new(staged: bool, commit: Option<String>, context_lines: usize) -> Result<Self, git2::Error> {
        let git = GitDiff::new(staged, commit, context_lines)?;
        let commits = git.load_commits(MAX_COMMITS)?;
        let ui = Ui::new();

        let mut app = App {
            commits,
            selected_commit: 0,
            commit_scroll_offset: 0,
            files: Vec::new(),
            selected_file: 0,
            file_scroll_offset: 0,
            diff_hunks: Vec::new(),
            scroll_offset: 0,
            git,
            ui,
            needs_full_redraw: true,
            mouse_enabled: true,
            last_refresh: Instant::now(),
        };

        app.load_files_for_selected_commit()?;

        Ok(app)
    }

    pub fn has_files(&self) -> bool {
        !self.commits.is_empty()
    }

    fn load_files_for_selected_commit(&mut self) -> Result<(), git2::Error> {
        if self.commits.is_empty() {
            self.files.clear();
            self.diff_hunks.clear();
            return Ok(());
        }

        let commit = &self.commits[self.selected_commit];
        
        if commit.is_local_changes {
            self.files = self.git.load_files()?;
        } else {
            self.files = self.git.load_files_for_commit(&commit.sha)?;
        }

        self.selected_file = 0;
        self.file_scroll_offset = 0;
        self.load_diff_for_selected()?;
        self.needs_full_redraw = true;
        Ok(())
    }

    fn refresh_if_needed(&mut self) {
        if self.last_refresh.elapsed().as_millis() < REFRESH_INTERVAL_MS {
            return;
        }
        self.last_refresh = Instant::now();

        // Reload commits
        let new_commits = match self.git.load_commits(MAX_COMMITS) {
            Ok(c) => c,
            Err(_) => return,
        };

        let commits_changed = new_commits.len() != self.commits.len()
            || new_commits.iter().zip(self.commits.iter()).any(|(a, b)| a.sha != b.sha || a.is_local_changes != b.is_local_changes);

        if commits_changed {
            self.commits = new_commits;
            self.selected_commit = self.selected_commit.min(self.commits.len().saturating_sub(1));
            let _ = self.load_files_for_selected_commit();
            return;
        }

        // Only refresh files/diff for local changes
        if !self.commits.is_empty() && self.commits[self.selected_commit].is_local_changes {
            let new_files = match self.git.load_files() {
                Ok(f) => f,
                Err(_) => return,
            };

            let files_changed = new_files.len() != self.files.len()
                || new_files.iter().zip(self.files.iter()).any(|(a, b)| a.path != b.path);

            if files_changed {
                self.files = new_files;
                self.selected_file = self.selected_file.min(self.files.len().saturating_sub(1));
                self.needs_full_redraw = true;
            }

            if !self.files.is_empty() {
                let file_path = self.files[self.selected_file].path.clone();
                if let Ok(new_hunks) = self.git.load_diff_for_file(&file_path) {
                    if new_hunks != self.diff_hunks {
                        self.diff_hunks = new_hunks;
                        self.needs_full_redraw = true;
                    }
                }
            }
        }
    }

    fn load_diff_for_selected(&mut self) -> Result<(), git2::Error> {
        if self.files.is_empty() {
            self.diff_hunks.clear();
            return Ok(());
        }

        let file_path = self.files[self.selected_file].path.clone();
        let commit = &self.commits[self.selected_commit];

        if commit.is_local_changes {
            self.diff_hunks = self.git.load_diff_for_file(&file_path)?;
        } else {
            self.diff_hunks = self.git.load_diff_for_commit_file(&commit.sha, &file_path)?;
        }
        
        self.scroll_offset = 0;
        self.needs_full_redraw = true;
        Ok(())
    }

    fn draw(&mut self, stdout: &mut io::Stdout) -> io::Result<()> {
        if self.needs_full_redraw {
            execute!(stdout, Clear(ClearType::All))?;
            self.needs_full_redraw = false;
        }
        execute!(stdout, MoveTo(0, 0))?;

        self.ui.draw_commit_panel(stdout, &self.commits, self.selected_commit, self.commit_scroll_offset)?;
        self.ui.draw_file_panel(stdout, &self.files, self.selected_file, self.file_scroll_offset)?;
        self.ui.draw_separator(stdout)?;

        let file_name = if !self.files.is_empty() {
            &self.files[self.selected_file].path
        } else {
            "No files"
        };
        self.ui.draw_diff_panel(stdout, file_name, &self.diff_hunks, self.scroll_offset)?;
        
        let total = self.total_diff_lines();
        let visible = (self.ui.term_height - 3) as usize;
        self.ui.draw_status_bar(stdout, self.scroll_offset, total, visible, self.mouse_enabled)?;

        stdout.flush()
    }

    fn select_prev_commit(&mut self) -> Result<(), git2::Error> {
        if self.selected_commit > 0 {
            self.selected_commit -= 1;
            // Scroll up if needed
            if self.selected_commit < self.commit_scroll_offset {
                self.commit_scroll_offset = self.selected_commit;
            }
            self.load_files_for_selected_commit()?;
        }
        Ok(())
    }

    fn select_next_commit(&mut self) -> Result<(), git2::Error> {
        if self.selected_commit < self.commits.len().saturating_sub(1) {
            self.selected_commit += 1;
            // Scroll down if needed
            let visible_commits = (self.ui.commit_panel_height - 1) as usize;
            if self.selected_commit >= self.commit_scroll_offset + visible_commits {
                self.commit_scroll_offset = self.selected_commit - visible_commits + 1;
            }
            self.load_files_for_selected_commit()?;
        }
        Ok(())
    }

    fn select_prev_file(&mut self) -> Result<(), git2::Error> {
        if self.selected_file > 0 {
            self.selected_file -= 1;
            // Scroll up if needed
            if self.selected_file < self.file_scroll_offset {
                self.file_scroll_offset = self.selected_file;
            }
            self.load_diff_for_selected()?;
        }
        Ok(())
    }

    fn select_next_file(&mut self) -> Result<(), git2::Error> {
        if self.selected_file < self.files.len().saturating_sub(1) {
            self.selected_file += 1;
            // Scroll down if needed
            let visible_files = (self.ui.term_height - self.ui.commit_panel_height - 2) as usize;
            if self.selected_file >= self.file_scroll_offset + visible_files {
                self.file_scroll_offset = self.selected_file - visible_files + 1;
            }
            self.load_diff_for_selected()?;
        }
        Ok(())
    }

    fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(3);
    }

    fn scroll_down(&mut self) {
        let total_lines: usize = self.diff_hunks.iter().map(|h| h.lines.len() + 1).sum();
        let max_scroll = total_lines.saturating_sub((self.ui.term_height - 3) as usize);
        self.scroll_offset = (self.scroll_offset + 3).min(max_scroll);
    }

    fn page_up(&mut self) {
        let page_size = (self.ui.term_height - 4) as usize;
        self.scroll_offset = self.scroll_offset.saturating_sub(page_size);
    }

    fn page_down(&mut self) {
        let total_lines: usize = self.diff_hunks.iter().map(|h| h.lines.len() + 1).sum();
        let max_scroll = total_lines.saturating_sub((self.ui.term_height - 3) as usize);
        let page_size = (self.ui.term_height - 4) as usize;
        self.scroll_offset = (self.scroll_offset + page_size).min(max_scroll);
    }

    fn total_diff_lines(&self) -> usize {
        self.diff_hunks.iter().map(|h| h.lines.len() + 1).sum()
    }

    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut stdout = io::stdout();

        terminal::enable_raw_mode()?;
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture, Hide)?;

        loop {
            self.refresh_if_needed();
            self.draw(&mut stdout)?;

            if event::poll(std::time::Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(key) => match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                        KeyCode::Left => {
                            let _ = self.select_prev_commit();
                        }
                        KeyCode::Right => {
                            let _ = self.select_next_commit();
                        }
                        KeyCode::Up => {
                            let _ = self.select_prev_file();
                        }
                        KeyCode::Down => {
                            let _ = self.select_next_file();
                        }
                        KeyCode::Char('k') => self.scroll_up(),
                        KeyCode::Char('j') => self.scroll_down(),
                        KeyCode::PageUp => self.page_up(),
                        KeyCode::PageDown => self.page_down(),
                        KeyCode::Char('m') => {
                            self.mouse_enabled = !self.mouse_enabled;
                            if self.mouse_enabled {
                                execute!(stdout, EnableMouseCapture)?;
                            } else {
                                execute!(stdout, DisableMouseCapture)?;
                            }
                        }
                        _ => {}
                    },
                    Event::Mouse(mouse) if self.mouse_enabled => match mouse.kind {
                        MouseEventKind::ScrollUp => self.scroll_up(),
                        MouseEventKind::ScrollDown => self.scroll_down(),
                        MouseEventKind::Down(_) => {
                            let commit_panel_height = self.ui.commit_panel_height;
                            
                            if mouse.column < self.ui.left_panel_width {
                                if mouse.row >= 1 && mouse.row < commit_panel_height {
                                    // Click in commit panel
                                    let clicked = (mouse.row - 1) as usize;
                                    if clicked < self.commits.len() && clicked != self.selected_commit {
                                        self.selected_commit = clicked;
                                        let _ = self.load_files_for_selected_commit();
                                    }
                                } else if mouse.row >= commit_panel_height + 1 {
                                    // Click in file panel
                                    let clicked = (mouse.row - commit_panel_height - 1) as usize;
                                    if clicked < self.files.len() && clicked != self.selected_file {
                                        self.selected_file = clicked;
                                        let _ = self.load_diff_for_selected();
                                    }
                                }
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }

        execute!(stdout, Show, DisableMouseCapture, LeaveAlternateScreen)?;
        terminal::disable_raw_mode()?;

        Ok(())
    }
}
