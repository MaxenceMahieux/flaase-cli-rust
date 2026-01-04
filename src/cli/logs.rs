//! Logs command handler.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use console::Style;

use crate::core::app_config::AppConfig;
use crate::core::error::AppError;

/// Shows logs for an app.
pub fn logs(
    app_name: &str,
    follow: bool,
    no_follow: bool,
    lines: u32,
    service: &str,
    since: Option<&str>,
    verbose: bool,
) -> Result<(), AppError> {
    let config = AppConfig::load(app_name)?;

    // Determine which containers to show
    let containers = get_service_containers(app_name, service, &config)?;

    if containers.is_empty() {
        return Err(AppError::Deploy(format!(
            "No containers found for service '{}'. Is the app deployed?",
            service
        )));
    }

    // Follow by default unless --no-follow is specified
    let should_follow = !no_follow || follow;

    // Validate --since format if provided
    if let Some(since_val) = since {
        validate_since(since_val)?;
    }

    if verbose {
        println!(
            "Showing logs for: {}",
            containers.join(", ")
        );
    }

    if containers.len() == 1 {
        // Single container - stream directly
        stream_container_logs(&containers[0], lines, since, should_follow)?;
    } else {
        // Multiple containers - show header for each
        if should_follow {
            // For follow mode with multiple containers, we need to merge streams
            stream_multi_container_logs(&containers, lines, since)?;
        } else {
            // Show each container's logs sequentially
            for container in &containers {
                let service_name = extract_service_name(container);
                println!(
                    "\n{} {}",
                    Style::new().bold().cyan().apply_to("==="),
                    Style::new().bold().apply_to(&service_name)
                );
                println!("{}", Style::new().dim().apply_to("=".repeat(40)));
                stream_container_logs(container, lines, since, false)?;
            }
        }
    }

    Ok(())
}

/// Gets the container names for a given service.
fn get_service_containers(
    app_name: &str,
    service: &str,
    config: &AppConfig,
) -> Result<Vec<String>, AppError> {
    let prefix = format!("flaase-{}", app_name);

    match service.to_lowercase().as_str() {
        "app" | "web" => Ok(vec![format!("{}-web", prefix)]),
        "database" | "db" => {
            if config.database.is_some() {
                Ok(vec![format!("{}-db", prefix)])
            } else {
                Err(AppError::Validation(
                    "No database configured for this app".into(),
                ))
            }
        }
        "cache" | "redis" => {
            if config.cache.is_some() {
                Ok(vec![format!("{}-cache", prefix)])
            } else {
                Err(AppError::Validation(
                    "No cache configured for this app".into(),
                ))
            }
        }
        "all" => {
            let mut containers = vec![format!("{}-web", prefix)];
            if config.database.is_some() {
                containers.push(format!("{}-db", prefix));
            }
            if config.cache.is_some() {
                containers.push(format!("{}-cache", prefix));
            }
            Ok(containers)
        }
        _ => Err(AppError::Validation(format!(
            "Unknown service '{}'. Use: app, database, cache, or all",
            service
        ))),
    }
}

/// Validates the --since format.
fn validate_since(since: &str) -> Result<(), AppError> {
    // Duration format: 1h, 30m, 2s, 1d
    if since.chars().last().map(|c| "hmsд".contains(c)).unwrap_or(false) {
        let num_part = &since[..since.len() - 1];
        if num_part.parse::<u64>().is_ok() {
            return Ok(());
        }
    }

    // Handle "Xh" "Xm" patterns
    if since.ends_with('h') || since.ends_with('m') || since.ends_with('s') || since.ends_with('d') {
        let num_part = &since[..since.len() - 1];
        if num_part.parse::<u64>().is_ok() {
            return Ok(());
        }
    }

    // ISO date format: 2024-01-15 or 2024-01-15T10:30:00
    if since.contains('-') && since.len() >= 10 {
        return Ok(());
    }

    // Relative format: "1 hour ago"
    if since.contains("ago") {
        return Ok(());
    }

    Err(AppError::Validation(format!(
        "Invalid --since format '{}'. Examples: 1h, 30m, 1d, 2024-01-15",
        since
    )))
}

/// Streams logs from a single container.
fn stream_container_logs(
    container: &str,
    lines: u32,
    since: Option<&str>,
    follow: bool,
) -> Result<(), AppError> {
    let mut args = vec!["logs".to_string()];

    if follow {
        args.push("-f".to_string());
    }

    args.push("--tail".to_string());
    args.push(lines.to_string());

    // Add timestamps
    args.push("-t".to_string());

    if let Some(since_val) = since {
        args.push("--since".to_string());
        args.push(since_val.to_string());
    }

    args.push(container.to_string());

    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    if follow {
        // Stream with colorization
        stream_with_colorization("docker", &args_ref)?;
    } else {
        // Get output and colorize
        let output = Command::new("docker")
            .args(&args_ref)
            .output()
            .map_err(|e| AppError::Command(format!("Failed to get logs: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Combine stdout and stderr (docker logs outputs to both)
        let combined = if stderr.is_empty() {
            stdout.to_string()
        } else if stdout.is_empty() {
            stderr.to_string()
        } else {
            format!("{}{}", stdout, stderr)
        };

        print_colorized_logs(&combined);
    }

    Ok(())
}

/// Streams logs from multiple containers (merged).
fn stream_multi_container_logs(
    containers: &[String],
    lines: u32,
    since: Option<&str>,
) -> Result<(), AppError> {
    // For multiple containers in follow mode, we use a simple approach:
    // spawn docker logs for each and prefix output with container name

    use std::thread;
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel();

    for container in containers {
        let container = container.clone();
        let since = since.map(|s| s.to_string());
        let tx = tx.clone();

        thread::spawn(move || {
            let mut args = vec!["logs", "-f", "--tail"];
            let lines_str = lines.to_string();
            args.push(&lines_str);
            args.push("-t");

            let since_owned;
            if let Some(ref s) = since {
                since_owned = s.clone();
                args.push("--since");
                args.push(&since_owned);
            }

            args.push(&container);

            let child = Command::new("docker")
                .args(&args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            if let Ok(mut child) = child {
                let service_name = extract_service_name(&container);
                let color = get_service_color(&service_name);

                if let Some(stdout) = child.stdout.take() {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines().map_while(Result::ok) {
                        let _ = tx.send((service_name.clone(), color.clone(), line));
                    }
                }
            }
        });
    }

    // Drop original sender so rx knows when all threads are done
    drop(tx);

    // Print received lines with colorization
    for (service, color, line) in rx {
        let prefix = color.apply_to(format!("[{}]", service));
        let colored_line = colorize_log_line(&line);
        println!("{} {}", prefix, colored_line);
    }

    Ok(())
}

/// Streams command output with colorization.
fn stream_with_colorization(cmd: &str, args: &[&str]) -> Result<(), AppError> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Command(format!("Failed to execute {}: {}", cmd, e)))?;

    // Handle stdout
    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            let colored = colorize_log_line(&line);
            println!("{}", colored);
        }
    }

    // Wait for process
    let status = child.wait()
        .map_err(|e| AppError::Command(format!("Failed to wait for process: {}", e)))?;

    if !status.success() {
        // Check if it was interrupted (Ctrl+C)
        if status.code() == Some(130) || status.code() == Some(137) {
            return Ok(());
        }
    }

    Ok(())
}

/// Prints logs with colorization.
fn print_colorized_logs(logs: &str) {
    for line in logs.lines() {
        let colored = colorize_log_line(line);
        println!("{}", colored);
    }
}

/// Colorizes a single log line based on content.
fn colorize_log_line(line: &str) -> String {
    let line_lower = line.to_lowercase();

    let error_style = Style::new().red();
    let warn_style = Style::new().yellow();
    let success_style = Style::new().green();
    let debug_style = Style::new().dim();

    // Error patterns
    if line_lower.contains("error")
        || line_lower.contains("fatal")
        || line_lower.contains("panic")
        || line_lower.contains("exception")
        || line_lower.contains("failed")
        || line_lower.contains("err:")
    {
        return error_style.apply_to(line).to_string();
    }

    // Warning patterns
    if line_lower.contains("warn")
        || line_lower.contains("warning")
        || line_lower.contains("deprecated")
    {
        return warn_style.apply_to(line).to_string();
    }

    // Success patterns
    if line_lower.contains("success")
        || line_lower.contains("started")
        || line_lower.contains("listening")
        || line_lower.contains("connected")
        || line_lower.contains("ready")
    {
        return success_style.apply_to(line).to_string();
    }

    // Debug patterns
    if line_lower.contains("debug") || line_lower.contains("trace") {
        return debug_style.apply_to(line).to_string();
    }

    // HTTP status codes
    if let Some(colored) = colorize_http_status(line) {
        return colored;
    }

    line.to_string()
}

/// Colorizes HTTP status codes in log lines.
fn colorize_http_status(line: &str) -> Option<String> {
    // Look for common HTTP log patterns like "GET /path 200" or "POST /api 500"
    let patterns = [" 2", " 3", " 4", " 5"];

    for pattern in patterns {
        if let Some(pos) = line.find(pattern) {
            // Check if followed by 2 more digits
            let rest = &line[pos + 2..];
            if rest.len() >= 2 {
                let status_chars: String = rest.chars().take(2).collect();
                if status_chars.chars().all(|c| c.is_ascii_digit()) {
                    let status_code: u16 = format!("{}{}", &pattern[1..], status_chars)
                        .parse()
                        .unwrap_or(0);

                    let style = match status_code {
                        200..=299 => Style::new().green(),
                        300..=399 => Style::new().cyan(),
                        400..=499 => Style::new().yellow(),
                        500..=599 => Style::new().red(),
                        _ => return None,
                    };

                    // Colorize just the status code portion
                    let before = &line[..pos + 1];
                    let code = &line[pos + 1..pos + 4];
                    let after = &line[pos + 4..];

                    return Some(format!(
                        "{}{}{}",
                        before,
                        style.apply_to(code),
                        after
                    ));
                }
            }
        }
    }

    None
}

/// Extracts service name from container name.
fn extract_service_name(container: &str) -> String {
    if container.ends_with("-web") {
        "app".to_string()
    } else if container.ends_with("-db") {
        "database".to_string()
    } else if container.ends_with("-cache") {
        "cache".to_string()
    } else {
        container.to_string()
    }
}

/// Gets a color style for a service.
fn get_service_color(service: &str) -> Style {
    match service {
        "app" => Style::new().cyan().bold(),
        "database" => Style::new().magenta().bold(),
        "cache" => Style::new().yellow().bold(),
        _ => Style::new().white().bold(),
    }
}
