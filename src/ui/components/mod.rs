mod confirm;
mod select;
mod text_input;

pub use confirm::Confirm;
pub use select::Select;
pub use text_input::TextInput;

use console::{style, Term};
use std::io;

/// Inner width of all boxed components (between border characters).
pub const INNER_WIDTH: usize = 54;

/// Shared rendering utilities for boxed components.
pub(crate) struct BoxRenderer<'a> {
    term: &'a Term,
}

impl<'a> BoxRenderer<'a> {
    pub fn new(term: &'a Term) -> Self {
        Self { term }
    }

    /// Renders the top border with a label.
    pub fn render_top(&self, label: &str) -> io::Result<()> {
        let label_display = format!(" {} ", label);
        let label_len = label_display.chars().count();
        let padding_len = INNER_WIDTH.saturating_sub(label_len);
        let padding = "─".repeat(padding_len);

        self.term.write_line(&format!(
            "{}{}{}{}",
            style("┌").dim(),
            style(&label_display).cyan(),
            style(&padding).dim(),
            style("┐").dim()
        ))
    }

    /// Renders the bottom border.
    pub fn render_bottom(&self) -> io::Result<()> {
        let padding = "─".repeat(INNER_WIDTH);
        self.term.write_line(&format!(
            "{}{}{}",
            style("└").dim(),
            style(&padding).dim(),
            style("┘").dim()
        ))
    }

    /// Renders a content line with side borders.
    pub fn render_line(&self, content: &str, visible_len: usize) -> io::Result<()> {
        let padding = INNER_WIDTH.saturating_sub(visible_len);
        self.term.write_line(&format!(
            "{}{}{}{}",
            style("│").dim(),
            content,
            " ".repeat(padding),
            style("│").dim()
        ))
    }

    /// Renders a content line without newline (for input fields).
    pub fn render_line_no_newline(&self, content: &str, visible_len: usize) {
        let padding = INNER_WIDTH.saturating_sub(visible_len);
        let _ = self.term.write_str(&format!(
            "{}{}{}{}",
            style("│").dim(),
            content,
            " ".repeat(padding),
            style("│").dim()
        ));
    }

    /// Clears the current line.
    pub fn clear_line(&self) -> io::Result<()> {
        self.term.clear_line()
    }

    /// Moves cursor up by n lines.
    pub fn move_up(&self, n: usize) -> io::Result<()> {
        self.term.move_cursor_up(n)
    }

    /// Moves cursor down by n lines.
    pub fn move_down(&self, n: usize) -> io::Result<()> {
        self.term.move_cursor_down(n)
    }

    /// Writes a newline.
    pub fn newline(&self) -> io::Result<()> {
        self.term.write_line("")
    }

    /// Hides the terminal cursor.
    pub fn hide_cursor(&self) -> io::Result<()> {
        self.term.hide_cursor()
    }

    /// Shows the terminal cursor.
    pub fn show_cursor(&self) -> io::Result<()> {
        self.term.show_cursor()
    }

    /// Reads a key from the terminal.
    pub fn read_key(&self) -> io::Result<console::Key> {
        self.term.read_key()
    }
}
