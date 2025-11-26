use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, Write};

use crate::git::GitDiff;
use crate::types::{DiffHunk, FileChange};
use crate::ui::Ui;

pub struct App {
    files: Vec<FileChange>,
    selected_file: usize,
    diff_hunks: Vec<DiffHunk>,
    scroll_offset: usize,
    git: GitDiff,
    ui: Ui,
}

impl App {
    pub fn new(staged: bool, commit: Option<String>, context_lines: usize) -> Result<Self, git2::Error> {
        let git = GitDiff::new(staged, commit, context_lines)?;
        let files = git.load_files()?;
        let ui = Ui::new();

        let mut app = App {
            files,
            selected_file: 0,
            diff_hunks: Vec::new(),
            scroll_offset: 0,
            git,
            ui,
        };

        if !app.files.is_empty() {
            app.load_diff_for_selected()?;
        }

        Ok(app)
    }

    pub fn has_files(&self) -> bool {
        !self.files.is_empty()
    }

    fn load_diff_for_selected(&mut self) -> Result<(), git2::Error> {
        if self.files.is_empty() {
            self.diff_hunks.clear();
            return Ok(());
        }

        let file_path = self.files[self.selected_file].path.clone();
        self.diff_hunks = self.git.load_diff_for_file(&file_path)?;
        self.scroll_offset = 0;
        Ok(())
    }

    fn draw(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        execute!(stdout, Clear(ClearType::All))?;

        self.ui.draw_file_panel(stdout, &self.files, self.selected_file)?;
        self.ui.draw_separator(stdout)?;

        let file_name = if !self.files.is_empty() {
            &self.files[self.selected_file].path
        } else {
            "No files"
        };
        self.ui.draw_diff_panel(stdout, file_name, &self.diff_hunks, self.scroll_offset)?;
        
        // Calculate scroll info for status bar
        let total = self.total_diff_lines();
        let visible = (self.ui.term_height - 3) as usize;
        self.ui.draw_status_bar(stdout, self.scroll_offset, total, visible)?;

        stdout.flush()
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
        execute!(stdout, EnterAlternateScreen, Hide)?;

        loop {
            self.draw(&mut stdout)?;

            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
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
                        _ => {}
                    }
                }
            }
        }

        execute!(stdout, Show, LeaveAlternateScreen)?;
        terminal::disable_raw_mode()?;

        Ok(())
    }
}
