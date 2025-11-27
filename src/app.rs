use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind},
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
    needs_full_redraw: bool,
    mouse_enabled: bool,
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
            needs_full_redraw: true,
            mouse_enabled: true,
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
        self.needs_full_redraw = true;
        Ok(())
    }

    fn draw(&mut self, stdout: &mut io::Stdout) -> io::Result<()> {
        if self.needs_full_redraw {
            execute!(stdout, Clear(ClearType::All))?;
            self.needs_full_redraw = false;
        }
        execute!(stdout, MoveTo(0, 0))?;

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
        self.ui.draw_status_bar(stdout, self.scroll_offset, total, visible, self.mouse_enabled)?;

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
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture, Hide)?;

        loop {
            self.draw(&mut stdout)?;

            if event::poll(std::time::Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(key) => match key.code {
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
                            // Click in file panel to select file
                            if mouse.column < self.ui.left_panel_width 
                                && mouse.row >= 1 
                                && (mouse.row as usize) <= self.files.len() 
                            {
                                let clicked_file = (mouse.row - 1) as usize;
                                if clicked_file != self.selected_file {
                                    self.selected_file = clicked_file;
                                    let _ = self.load_diff_for_selected();
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
