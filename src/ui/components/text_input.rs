use super::{BoxRenderer, INNER_WIDTH};
use console::{style, Key, Term};
use std::io;

/// A boxed text input component with optional placeholder and password masking.
pub struct TextInput<'a> {
    prompt: &'a str,
    placeholder: Option<&'a str>,
    masked: bool,
}

impl<'a> TextInput<'a> {
    /// Creates a new text input with the given prompt.
    pub fn new(prompt: &'a str) -> Self {
        Self {
            prompt,
            placeholder: None,
            masked: false,
        }
    }

    /// Sets a placeholder text shown when input is empty.
    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = Some(placeholder);
        self
    }

    /// Enables password masking (shows * instead of characters).
    pub fn masked(mut self) -> Self {
        self.masked = true;
        self
    }

    /// Runs the input prompt and returns the entered text.
    pub fn run(self) -> io::Result<String> {
        let term = Term::stdout();
        let renderer = BoxRenderer::new(&term);
        let mut input = String::new();
        let mut cursor_pos: usize = 0;

        renderer.hide_cursor()?;
        renderer.render_top(self.prompt)?;
        self.render_input_line(&renderer, &input, cursor_pos, true);
        renderer.newline()?;
        renderer.render_bottom()?;

        loop {
            renderer.move_up(2)?;
            renderer.clear_line()?;
            self.render_input_line(&renderer, &input, cursor_pos, true);
            renderer.newline()?;
            renderer.move_down(1)?;

            match renderer.read_key()? {
                Key::Enter => break,
                Key::Char(c) => {
                    input.insert(cursor_pos, c);
                    cursor_pos += 1;
                }
                Key::Backspace if cursor_pos > 0 => {
                    cursor_pos -= 1;
                    input.remove(cursor_pos);
                }
                Key::ArrowLeft if cursor_pos > 0 => {
                    cursor_pos -= 1;
                }
                Key::ArrowRight if cursor_pos < input.len() => {
                    cursor_pos += 1;
                }
                Key::Home => {
                    cursor_pos = 0;
                }
                Key::End => {
                    cursor_pos = input.len();
                }
                _ => {}
            }
        }

        // Final render without cursor
        renderer.move_up(2)?;
        renderer.clear_line()?;
        self.render_input_line(&renderer, &input, 0, false);
        renderer.newline()?;
        renderer.move_down(1)?;
        renderer.show_cursor()?;

        Ok(input)
    }

    /// Renders the input line with scrolling and placeholder support.
    fn render_input_line(
        &self,
        renderer: &BoxRenderer,
        content: &str,
        cursor_pos: usize,
        show_cursor: bool,
    ) {
        let display_content = if self.masked {
            "*".repeat(content.len())
        } else {
            content.to_string()
        };

        let text_width = INNER_WIDTH - 2; // -1 leading space, -1 cursor space
        let mut display = String::from(" "); // Leading space for alignment
        let mut visible_len = 1;

        if display_content.is_empty() && !show_cursor {
            // Show placeholder when empty and no cursor
            if let Some(ph) = self.placeholder {
                let ph_display: String = ph.chars().take(text_width).collect();
                display.push_str(&style(&ph_display).dim().to_string());
                visible_len += ph_display.chars().count();
            }
        } else if display_content.is_empty() && show_cursor {
            // Empty with cursor
            display.push_str(&style(" ").reverse().to_string());
            visible_len += 1;

            // Show rest of placeholder dimmed
            if let Some(ph) = self.placeholder {
                let ph_rest: String = ph.chars().skip(1).take(text_width - 1).collect();
                if !ph_rest.is_empty() {
                    display.push_str(&style(&ph_rest).dim().to_string());
                    visible_len += ph_rest.chars().count();
                }
            }
        } else {
            // Has content - render with scrolling
            let chars: Vec<char> = display_content.chars().collect();
            let content_len = chars.len();

            let scroll_offset = if cursor_pos >= text_width {
                cursor_pos - text_width + 1
            } else {
                0
            };

            let visible_end = (scroll_offset + text_width).min(content_len);
            let visible_chars: Vec<char> = chars
                .iter()
                .skip(scroll_offset)
                .take(visible_end - scroll_offset)
                .copied()
                .collect();

            for (i, ch) in visible_chars.iter().enumerate() {
                let actual_pos = scroll_offset + i;
                if show_cursor && actual_pos == cursor_pos {
                    display.push_str(&style(ch.to_string()).reverse().to_string());
                } else {
                    display.push(*ch);
                }
                visible_len += 1;
            }

            // Show cursor at end if needed
            if show_cursor && cursor_pos >= content_len && cursor_pos >= scroll_offset {
                display.push_str(&style(" ").reverse().to_string());
                visible_len += 1;
            }
        }

        renderer.render_line_no_newline(&display, visible_len);
    }
}
