use super::Select;
use std::io;

/// A boxed confirmation component for yes/no questions.
pub struct Confirm<'a> {
    prompt: &'a str,
    default: bool,
}

impl<'a> Confirm<'a> {
    /// Creates a new confirm with the given prompt.
    pub fn new(prompt: &'a str) -> Self {
        Self {
            prompt,
            default: true,
        }
    }

    /// Sets the default value (true = Yes, false = No).
    pub fn default(mut self, value: bool) -> Self {
        self.default = value;
        self
    }

    /// Runs the confirm prompt and returns the boolean result.
    pub fn run(self) -> io::Result<bool> {
        let options = if self.default {
            vec!["Yes", "No"]
        } else {
            vec!["No", "Yes"]
        };

        let selected = Select::new(self.prompt, &options).run()?;

        // default=true: ["Yes", "No"] -> selected=0 means Yes
        // default=false: ["No", "Yes"] -> selected=1 means Yes
        Ok((selected == 0 && self.default) || (selected == 1 && !self.default))
    }
}
