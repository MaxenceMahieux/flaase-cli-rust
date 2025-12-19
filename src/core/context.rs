use std::process::{Command, Output, Stdio};

use crate::core::error::AppError;
use crate::ui;

/// Execution context that controls how commands are run.
/// Supports dry-run mode and verbose output.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    dry_run: bool,
    verbose: bool,
}

impl ExecutionContext {
    /// Creates a new execution context.
    pub fn new(dry_run: bool, verbose: bool) -> Self {
        Self { dry_run, verbose }
    }

    /// Returns true if in dry-run mode.
    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    /// Returns true if verbose output is enabled.
    pub fn is_verbose(&self) -> bool {
        self.verbose
    }

    /// Executes a shell command and returns the output.
    /// In dry-run mode, prints the command without executing it.
    pub fn run_command(&self, cmd: &str, args: &[&str]) -> Result<CommandOutput, AppError> {
        let full_cmd = format!("{} {}", cmd, args.join(" "));

        if self.dry_run {
            ui::info(&format!("[DRY-RUN] {}", full_cmd));
            return Ok(CommandOutput::dry_run());
        }

        if self.verbose {
            ui::info(&format!("Running: {}", full_cmd));
        }

        let output = Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::Command(format!("Failed to execute '{}': {}", cmd, e)))?;

        let cmd_output = CommandOutput::from_output(output);

        if self.verbose && !cmd_output.stdout.is_empty() {
            println!("{}", cmd_output.stdout);
        }

        Ok(cmd_output)
    }

    /// Executes a shell command with sudo prefix.
    pub fn run_sudo(&self, cmd: &str, args: &[&str]) -> Result<CommandOutput, AppError> {
        let mut sudo_args = vec![cmd];
        sudo_args.extend(args);
        self.run_command("sudo", &sudo_args)
    }

    /// Executes a shell command and streams output in real-time.
    /// Useful for long-running commands like package installation.
    pub fn run_command_streaming(
        &self,
        cmd: &str,
        args: &[&str],
    ) -> Result<CommandOutput, AppError> {
        let full_cmd = format!("{} {}", cmd, args.join(" "));

        if self.dry_run {
            ui::info(&format!("[DRY-RUN] {}", full_cmd));
            return Ok(CommandOutput::dry_run());
        }

        if self.verbose {
            ui::info(&format!("Running: {}", full_cmd));
        }

        let status = Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(if self.verbose {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .stderr(if self.verbose {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .status()
            .map_err(|e| AppError::Command(format!("Failed to execute '{}': {}", cmd, e)))?;

        Ok(CommandOutput {
            success: status.success(),
            code: status.code().unwrap_or(-1),
            stdout: String::new(),
            stderr: String::new(),
            dry_run: false,
        })
    }

    /// Executes a sudo command with streaming output.
    pub fn run_sudo_streaming(&self, cmd: &str, args: &[&str]) -> Result<CommandOutput, AppError> {
        let mut sudo_args = vec![cmd];
        sudo_args.extend(args);
        self.run_command_streaming("sudo", &sudo_args)
    }

    /// Writes content to a file.
    /// In dry-run mode, prints what would be written.
    pub fn write_file(&self, path: &str, content: &str) -> Result<(), AppError> {
        if self.dry_run {
            ui::info(&format!("[DRY-RUN] Write to {}", path));
            if self.verbose {
                println!("Content:\n{}", content);
            }
            return Ok(());
        }

        if self.verbose {
            ui::info(&format!("Writing to {}", path));
        }

        std::fs::write(path, content).map_err(AppError::Io)
    }

    /// Creates a directory with all parent directories.
    /// In dry-run mode, prints what would be created.
    pub fn create_dir(&self, path: &str) -> Result<(), AppError> {
        if self.dry_run {
            ui::info(&format!("[DRY-RUN] Create directory {}", path));
            return Ok(());
        }

        if self.verbose {
            ui::info(&format!("Creating directory {}", path));
        }

        std::fs::create_dir_all(path).map_err(AppError::Io)
    }
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self::new(false, false)
    }
}

/// Result of a command execution.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub success: bool,
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    pub dry_run: bool,
}

impl CommandOutput {
    /// Creates a mock output for dry-run mode.
    pub fn dry_run() -> Self {
        Self {
            success: true,
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
            dry_run: true,
        }
    }

    /// Creates output from a std::process::Output.
    pub fn from_output(output: Output) -> Self {
        Self {
            success: output.status.success(),
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            dry_run: false,
        }
    }

    /// Returns an error if the command failed.
    pub fn ensure_success(&self, context: &str) -> Result<(), AppError> {
        if self.dry_run || self.success {
            Ok(())
        } else {
            Err(AppError::Command(format!(
                "{}: {}",
                context,
                if self.stderr.is_empty() {
                    "Command failed"
                } else {
                    &self.stderr
                }
            )))
        }
    }
}
