use console::{style, Term};

const ASCII_HEADER: &str = r#"
   __ _
  / _| | __ _  __ _ ___  ___
 | |_| |/ _` |/ _` / __|/ _ \
 |  _| | (_| | (_| \__ \  __/
 |_| |_|\__,_|\__,_|___/\___|
"#;

/// Prints the Flaase ASCII art header.
pub fn header() {
    let term = Term::stdout();
    let _ = term.write_line(&style(ASCII_HEADER).cyan().to_string());
}

/// Prints a success message with a green checkmark.
pub fn success(message: &str) {
    println!("{} {}", style("✓").green(), message);
}

/// Prints an error message with a red cross.
pub fn error(message: &str) {
    eprintln!("{} {}", style("✗").red(), message);
}

/// Prints a warning message in yellow.
pub fn warning(message: &str) {
    println!("{} {}", style("!").yellow(), message);
}

/// Prints an info message with an arrow.
pub fn info(message: &str) {
    println!("{} {}", style("→").cyan(), message);
}

/// Prints a URL in cyan and bold.
pub fn url(url: &str) {
    println!("{} {}", style("→").cyan(), style(url).cyan().bold());
}

/// Prints an error with a hint for resolution.
pub fn error_with_hint(message: &str, hint: &str) {
    eprintln!("{} {}", style("✗").red(), message);
    eprintln!("  {} {}", style("→").dim(), hint);
}

/// Prints a step in progress (spinner style).
pub fn step(message: &str) {
    print!("{} {}... ", style("◦").cyan(), style(message).dim());
    use std::io::Write;
    std::io::stdout().flush().ok();
}

/// Marks the current step as done.
pub fn step_done() {
    println!("{}", style("done").green());
}

/// Marks the current step as failed.
pub fn step_failed() {
    println!("{}", style("failed").red());
}

/// Prints a section header.
pub fn section(title: &str) {
    println!();
    println!("{}", style(title).bold());
    println!();
}
