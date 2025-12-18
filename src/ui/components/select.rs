use super::BoxRenderer;
use console::{style, Key, Term};
use std::io;

/// A boxed selection component for choosing from a list of options.
pub struct Select<'a, T: AsRef<str>> {
    prompt: &'a str,
    items: &'a [T],
    default: usize,
}

impl<'a, T: AsRef<str>> Select<'a, T> {
    /// Creates a new select with the given prompt and items.
    pub fn new(prompt: &'a str, items: &'a [T]) -> Self {
        Self {
            prompt,
            items,
            default: 0,
        }
    }

    /// Sets the default selected index.
    pub fn default(mut self, index: usize) -> Self {
        self.default = index.min(self.items.len().saturating_sub(1));
        self
    }

    /// Runs the select prompt and returns the selected index.
    pub fn run(self) -> io::Result<usize> {
        let term = Term::stdout();
        let renderer = BoxRenderer::new(&term);
        let mut selected = self.default;
        let items_count = self.items.len();
        let total_lines = items_count + 2; // top + items + bottom

        renderer.hide_cursor()?;
        self.render_box(&renderer, selected, false)?;

        loop {
            match renderer.read_key()? {
                Key::Enter => break,
                Key::ArrowUp | Key::Char('k') if selected > 0 => {
                    selected -= 1;
                    renderer.move_up(total_lines)?;
                    self.render_box(&renderer, selected, false)?;
                }
                Key::ArrowDown | Key::Char('j') if selected < items_count - 1 => {
                    selected += 1;
                    renderer.move_up(total_lines)?;
                    self.render_box(&renderer, selected, false)?;
                }
                _ => {}
            }
        }

        // Final render: collapse to show only selected item
        renderer.move_up(total_lines)?;
        self.render_box(&renderer, selected, true)?;

        // Clear remaining old lines if we collapsed
        if items_count > 1 {
            for _ in 0..items_count - 1 {
                renderer.clear_line()?;
                renderer.newline()?;
            }
            renderer.move_up(items_count - 1)?;
        }

        renderer.show_cursor()?;

        Ok(selected)
    }

    /// Renders the complete select box.
    fn render_box(
        &self,
        renderer: &BoxRenderer,
        selected: usize,
        collapsed: bool,
    ) -> io::Result<()> {
        renderer.clear_line()?;
        renderer.render_top(self.prompt)?;

        if collapsed {
            self.render_option_line(renderer, self.items[selected].as_ref(), true, true)?;
        } else {
            for (i, item) in self.items.iter().enumerate() {
                renderer.clear_line()?;
                self.render_option_line(renderer, item.as_ref(), false, i == selected)?;
            }
        }

        renderer.clear_line()?;
        renderer.render_bottom()
    }

    /// Renders a single option line.
    fn render_option_line(
        &self,
        renderer: &BoxRenderer,
        label: &str,
        is_selected: bool,
        is_active: bool,
    ) -> io::Result<()> {
        let prefix = if is_active {
            if is_selected {
                style("› ●").cyan().to_string()
            } else {
                style("› ○").cyan().to_string()
            }
        } else if is_selected {
            style("  ●").dim().to_string()
        } else {
            style("  ○").dim().to_string()
        };

        let label_styled = if is_active {
            style(label).cyan().to_string()
        } else {
            style(label).dim().to_string()
        };

        let content = format!("{} {}", prefix, label_styled);
        // "› ● " = 4 visible chars + label
        let visible_len = 4 + label.chars().count();

        renderer.render_line(&content, visible_len)
    }
}
