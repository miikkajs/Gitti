use crossterm::{cursor::MoveTo, execute};
use similar::ChangeTag;
use std::io::{self, Write};

use crate::theme;
use crate::types::{DiffHunk, DiffLine, FileChange};

pub struct Ui {
    pub term_width: u16,
    pub term_height: u16,
    pub left_panel_width: u16,
}

impl Ui {
    pub fn new() -> Self {
        let (width, height) = crossterm::terminal::size().unwrap_or((120, 40));
        let left_panel_width = (width / 4).max(25).min(50);
        Self {
            term_width: width,
            term_height: height,
            left_panel_width,
        }
    }

    pub fn draw_file_panel(
        &self,
        stdout: &mut io::Stdout,
        files: &[FileChange],
        selected: usize,
    ) -> io::Result<()> {
        let panel_width = self.left_panel_width as usize;

        // Header
        execute!(stdout, MoveTo(0, 0))?;
        let header = format!(" Changes ({}) ", files.len());
        let header_padded = format!("{:<width$}", header, width = panel_width);
        write!(
            stdout,
            "{}{}{}{}",
            theme::BG_HEADER,
            theme::FG_DEFAULT,
            header_padded,
            theme::RESET
        )?;

        // File list
        for (i, file) in files.iter().enumerate() {
            if i + 1 >= self.term_height as usize - 1 {
                break;
            }

            execute!(stdout, MoveTo(0, (i + 1) as u16))?;

            let (icon, color) = match file.status.as_str() {
                "added" => ("+", theme::FG_ADDED),
                "deleted" => ("-", theme::FG_REMOVED),
                _ => ("~", theme::FG_HEADER),
            };

            let bg = if i == selected {
                theme::BG_SELECTED
            } else {
                theme::BG_PANEL
            };

            let max_name_len = panel_width.saturating_sub(4);
            let display_name = if file.path.len() > max_name_len {
                format!("…{}", &file.path[file.path.len() - max_name_len + 1..])
            } else {
                file.path.clone()
            };

            let line = format!(" {} {:<width$}", icon, display_name, width = max_name_len);
            write!(stdout, "{}{}{}{}", bg, color, line, theme::RESET)?;
        }

        // Fill remaining space
        for i in files.len() + 1..self.term_height as usize - 1 {
            execute!(stdout, MoveTo(0, i as u16))?;
            write!(
                stdout,
                "{}{:width$}{}",
                theme::BG_PANEL,
                "",
                theme::RESET,
                width = panel_width
            )?;
        }

        Ok(())
    }

    pub fn draw_separator(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        let x = self.left_panel_width;
        for y in 0..self.term_height - 1 {
            execute!(stdout, MoveTo(x, y))?;
            write!(
                stdout,
                "{}{}│{}",
                theme::BG_DARK,
                theme::FG_SEPARATOR,
                theme::RESET
            )?;
        }
        Ok(())
    }

    pub fn draw_diff_panel(
        &self,
        stdout: &mut io::Stdout,
        file_name: &str,
        hunks: &[DiffHunk],
        scroll_offset: usize,
    ) -> io::Result<()> {
        let start_x = self.left_panel_width + 1;
        let diff_width = (self.term_width - start_x) as usize;

        // Header
        execute!(stdout, MoveTo(start_x, 0))?;
        let header = format!(" {} ", file_name);
        let header_padded = format!("{:<width$}", header, width = diff_width);
        write!(
            stdout,
            "{}{}{}{}",
            theme::BG_HEADER,
            theme::FG_HEADER,
            header_padded,
            theme::RESET
        )?;

        if hunks.is_empty() {
            execute!(stdout, MoveTo(start_x, 2))?;
            write!(
                stdout,
                "{}{}  No changes{}",
                theme::BG_DARK,
                theme::FG_DIM,
                theme::RESET
            )?;
            return Ok(());
        }

        let mut row = 1u16;
        let max_rows = self.term_height - 2;
        let mut line_idx = 0usize;

        for (hunk_idx, hunk) in hunks.iter().enumerate() {
            if row >= max_rows {
                break;
            }

            if line_idx + hunk.lines.len() <= scroll_offset {
                line_idx += hunk.lines.len() + 1;
                continue;
            }

            if hunk_idx > 0 && line_idx >= scroll_offset {
                execute!(stdout, MoveTo(start_x, row))?;
                let sep = format!("{:─<width$}", "─", width = diff_width);
                write!(
                    stdout,
                    "{}{}{}{}",
                    theme::BG_HUNK,
                    theme::FG_SEPARATOR,
                    sep,
                    theme::RESET
                )?;
                row += 1;
                if row >= max_rows {
                    break;
                }
            }
            line_idx += 1;

            for line in &hunk.lines {
                if line_idx < scroll_offset {
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

        while row < max_rows {
            execute!(stdout, MoveTo(start_x, row))?;
            write!(
                stdout,
                "{}{:width$}{}",
                theme::BG_DARK,
                "",
                theme::RESET,
                width = diff_width
            )?;
            row += 1;
        }

        Ok(())
    }

    fn draw_diff_line(
        &self,
        stdout: &mut io::Stdout,
        line: &DiffLine,
        width: usize,
    ) -> io::Result<()> {
        let old_str = line
            .old_num
            .map(|n| format!("{:>4}", n))
            .unwrap_or_else(|| "    ".to_string());
        let new_str = line
            .new_num
            .map(|n| format!("{:>4}", n))
            .unwrap_or_else(|| "    ".to_string());

        let content_width = width.saturating_sub(14);

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
                let color_code =
                    theme::rgb_to_256(style.foreground.r, style.foreground.g, style.foreground.b);
                content.push_str(&format!("\x1b[38;5;{}m{}", color_code, display_text));
                chars_written += display_text.len();
            }
            if chars_written < content_width {
                content.push_str(&" ".repeat(content_width - chars_written));
            }
        } else if line.content.len() > content_width {
            content = format!("{}…", &line.content[..content_width.saturating_sub(1)]);
        } else {
            content = format!("{:<width$}", line.content, width = content_width);
        }

        match line.tag {
            ChangeTag::Insert => {
                write!(
                    stdout,
                    "\x1b[48;5;236m\x1b[38;5;243m{} {}\x1b[38;5;240m│\x1b[48;5;22m\x1b[38;5;114m+ {}\x1b[0m",
                    old_str, new_str, content
                )?;
            }
            ChangeTag::Delete => {
                write!(
                    stdout,
                    "\x1b[48;5;236m\x1b[38;5;243m{} {}\x1b[38;5;240m│\x1b[48;5;52m\x1b[38;5;210m- {}\x1b[0m",
                    old_str, new_str, content
                )?;
            }
            ChangeTag::Equal => {
                write!(
                    stdout,
                    "\x1b[48;5;236m\x1b[38;5;243m{} {}\x1b[38;5;240m│\x1b[48;5;236m\x1b[38;5;250m  {}\x1b[0m",
                    old_str, new_str, content
                )?;
            }
        }

        Ok(())
    }

    pub fn draw_status_bar(&self, stdout: &mut io::Stdout, scroll_offset: usize, total_lines: usize, visible_lines: usize) -> io::Result<()> {
        execute!(stdout, MoveTo(0, self.term_height - 1))?;
        
        let scroll_info = if total_lines > visible_lines {
            let percent = if total_lines == 0 {
                100
            } else {
                ((scroll_offset + visible_lines) * 100 / total_lines).min(100)
            };
            format!(" {}% ", percent)
        } else {
            " All ".to_string()
        };
        
        let controls = " ↑↓ Files │ j/k Scroll │ PgUp/PgDn Page │ q Quit ";
        let right_padding = self.term_width as usize - controls.len() - scroll_info.len();
        let status = format!("{}{:>width$}{}", controls, "", scroll_info, width = right_padding);
        
        write!(
            stdout,
            "{}{}{}{}",
            theme::BG_HEADER,
            theme::FG_DIM,
            status,
            theme::RESET
        )
    }
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}
