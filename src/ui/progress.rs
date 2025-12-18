use console::style;
use indicatif::{ProgressBar as IndicatifBar, ProgressStyle};
use std::time::Duration;

/// A pnpm-style progress bar for deployment operations.
pub struct ProgressBar {
    bar: IndicatifBar,
}

impl ProgressBar {
    /// Creates a new progress bar with a label.
    pub fn new(label: &str, total: u64) -> Self {
        let bar = IndicatifBar::new(total);
        bar.set_style(
            ProgressStyle::default_bar()
                .template(&format!(
                    "{{spinner:.cyan}} {} {{bar:20.cyan/dim}} {{percent:>3}}%",
                    style(format!("{:<20}", label)).dim()
                ))
                .expect("Invalid progress bar template")
                .progress_chars("█░"),
        );
        bar.enable_steady_tick(Duration::from_millis(100));
        Self { bar }
    }

    /// Creates a spinner for indeterminate progress.
    pub fn spinner(label: &str) -> Self {
        let bar = IndicatifBar::new_spinner();
        bar.set_style(
            ProgressStyle::default_spinner()
                .template(&format!("{{spinner:.cyan}} {}", style(label).dim()))
                .expect("Invalid spinner template"),
        );
        bar.enable_steady_tick(Duration::from_millis(100));
        Self { bar }
    }

    /// Updates the progress bar position.
    pub fn set(&self, value: u64) {
        self.bar.set_position(value);
    }

    /// Increments the progress bar by a given amount.
    pub fn inc(&self, delta: u64) {
        self.bar.inc(delta);
    }

    /// Finishes the progress bar with a success message.
    pub fn finish(&self, message: &str) {
        self.bar
            .finish_with_message(format!("{} {}", style("✓").green(), message));
    }

    /// Finishes the progress bar with an error message.
    pub fn finish_error(&self, message: &str) {
        self.bar
            .finish_with_message(format!("{} {}", style("✗").red(), message));
    }

    /// Abandons the progress bar (clears it).
    pub fn abandon(&self) {
        self.bar.abandon();
    }
}

/// Creates a multi-progress display for parallel operations.
pub struct MultiProgress {
    multi: indicatif::MultiProgress,
}

impl MultiProgress {
    pub fn new() -> Self {
        Self {
            multi: indicatif::MultiProgress::new(),
        }
    }

    /// Adds a progress bar to the multi-progress display.
    pub fn add(&self, label: &str, total: u64) -> ProgressBar {
        let bar = IndicatifBar::new(total);
        bar.set_style(
            ProgressStyle::default_bar()
                .template(&format!(
                    "{{spinner:.cyan}} {} {{bar:20.cyan/dim}} {{percent:>3}}%",
                    style(format!("{:<20}", label)).dim()
                ))
                .expect("Invalid progress bar template")
                .progress_chars("█░"),
        );
        bar.enable_steady_tick(Duration::from_millis(100));
        let bar = self.multi.add(bar);
        ProgressBar { bar }
    }
}

impl Default for MultiProgress {
    fn default() -> Self {
        Self::new()
    }
}
