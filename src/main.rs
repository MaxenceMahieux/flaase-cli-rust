use anyhow::Result;
use clap::Parser;
use flaase::cli::{AutodeployCommands, Cli, Commands, DomainCommands, EnvCommands, ServerCommands};
use flaase::ui;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(command) => run_command(command, cli.verbose),
        None => {
            ui::header();
            println!(
                "Use {} for available commands",
                console::style("fl --help").cyan()
            );
            Ok(())
        }
    }
}

fn run_command(command: Commands, verbose: bool) -> Result<()> {
    match command {
        Commands::Server { command } => match command {
            ServerCommands::Init { dry_run } => {
                flaase::cli::server::init(dry_run, verbose)?;
                Ok(())
            }
            ServerCommands::Status => {
                ui::info("Server status not yet implemented");
                Ok(())
            }
        },

        Commands::Init => {
            ui::info("App init not yet implemented");
            Ok(())
        }

        Commands::Status => {
            ui::info("Status not yet implemented");
            Ok(())
        }

        Commands::Deploy { app } => {
            ui::info(&format!("Deploy '{}' not yet implemented", app));
            Ok(())
        }

        Commands::Update { app } => {
            ui::info(&format!("Update '{}' not yet implemented", app));
            Ok(())
        }

        Commands::Stop { app } => {
            ui::info(&format!("Stop '{}' not yet implemented", app));
            Ok(())
        }

        Commands::Start { app } => {
            ui::info(&format!("Start '{}' not yet implemented", app));
            Ok(())
        }

        Commands::Restart { app } => {
            ui::info(&format!("Restart '{}' not yet implemented", app));
            Ok(())
        }

        Commands::Destroy { app } => {
            ui::info(&format!("Destroy '{}' not yet implemented", app));
            Ok(())
        }

        Commands::Logs { app, .. } => {
            ui::info(&format!("Logs '{}' not yet implemented", app));
            Ok(())
        }

        Commands::Env { command } => match command {
            EnvCommands::List { app } => {
                ui::info(&format!("Env list '{}' not yet implemented", app));
                Ok(())
            }
            EnvCommands::Set { app, .. } => {
                ui::info(&format!("Env set '{}' not yet implemented", app));
                Ok(())
            }
            EnvCommands::Remove { app, .. } => {
                ui::info(&format!("Env remove '{}' not yet implemented", app));
                Ok(())
            }
        },

        Commands::Domain { command } => match command {
            DomainCommands::List { app } => {
                ui::info(&format!("Domain list '{}' not yet implemented", app));
                Ok(())
            }
            DomainCommands::Add { app, .. } => {
                ui::info(&format!("Domain add '{}' not yet implemented", app));
                Ok(())
            }
            DomainCommands::Remove { app, .. } => {
                ui::info(&format!("Domain remove '{}' not yet implemented", app));
                Ok(())
            }
        },

        Commands::Autodeploy { command } => match command {
            AutodeployCommands::Enable { app } => {
                ui::info(&format!("Autodeploy enable '{}' not yet implemented", app));
                Ok(())
            }
            AutodeployCommands::Disable { app } => {
                ui::info(&format!("Autodeploy disable '{}' not yet implemented", app));
                Ok(())
            }
            AutodeployCommands::Status { app } => {
                ui::info(&format!("Autodeploy status '{}' not yet implemented", app));
                Ok(())
            }
        },
    }
}
