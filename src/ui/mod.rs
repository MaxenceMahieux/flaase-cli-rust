pub mod components;
pub mod output;
pub mod progress;

// Re-export components for direct access
pub use components::{Confirm, Select, TextInput};

// Re-export output utilities
pub use output::{error, error_with_hint, header, info, success, url, warning};

// Re-export progress utilities
pub use progress::{MultiProgress, ProgressBar};

// Convenience functions that wrap the components for simpler usage

use std::io;

/// Prompts for text input.
pub fn input(prompt: &str) -> io::Result<String> {
    TextInput::new(prompt).run()
}

/// Prompts for text input with a placeholder.
pub fn input_with_placeholder(prompt: &str, placeholder: Option<&str>) -> io::Result<String> {
    let mut input = TextInput::new(prompt);
    if let Some(ph) = placeholder {
        input = input.placeholder(ph);
    }
    input.run()
}

/// Prompts for text input with a default value shown as placeholder.
pub fn input_with_default(prompt: &str, default: &str) -> io::Result<String> {
    let result = if default.is_empty() {
        TextInput::new(prompt).run()?
    } else {
        TextInput::new(prompt).placeholder(default).run()?
    };

    if result.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(result)
    }
}

/// Prompts for password input (masked).
pub fn password(prompt: &str) -> io::Result<String> {
    TextInput::new(prompt).masked().run()
}

/// Prompts for selection from a list of options.
pub fn select<T: AsRef<str>>(prompt: &str, items: &[T]) -> io::Result<usize> {
    Select::new(prompt, items).run()
}

/// Prompts for a yes/no confirmation.
pub fn confirm(prompt: &str, default: bool) -> io::Result<bool> {
    Confirm::new(prompt).default(default).run()
}
