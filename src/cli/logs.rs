//! Logs command handler.

use crate::core::app_config::AppConfig;
use crate::core::context::ExecutionContext;
use crate::core::error::AppError;
use crate::providers::create_container_runtime;

/// Shows logs for an app.
pub fn logs(app_name: &str, follow: bool, lines: u32, verbose: bool) -> Result<(), AppError> {
    let _config = AppConfig::load(app_name)?;
    let ctx = ExecutionContext::new(false, verbose);

    let container_name = format!("flaase-{}-web", app_name);

    let runtime = create_container_runtime();

    // Check if container exists
    if !runtime.container_exists(&container_name, &ctx)? {
        return Err(AppError::Deploy(format!(
            "Container '{}' not found. Is the app deployed?",
            container_name
        )));
    }

    if follow {
        // Use streaming for follow mode
        let mut args = vec!["logs", "-f", "--tail"];
        let lines_str = lines.to_string();
        args.push(&lines_str);
        args.push(&container_name);

        ctx.run_command_streaming("docker", &args)?;
    } else {
        // Get logs and print them
        let logs = runtime.get_logs(&container_name, lines, &ctx)?;
        println!("{}", logs);
    }

    Ok(())
}
