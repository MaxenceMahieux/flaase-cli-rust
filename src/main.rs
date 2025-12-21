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
            flaase::cli::app::init(verbose)?;
            Ok(())
        }

        Commands::Status => {
            flaase::cli::status::status(verbose)?;
            Ok(())
        }

        Commands::Deploy { app } => {
            flaase::cli::deploy::deploy(&app, verbose)?;
            Ok(())
        }

        Commands::Update { app } => {
            flaase::cli::deploy::update(&app, verbose)?;
            Ok(())
        }

        Commands::Stop { app } => {
            flaase::cli::deploy::stop(&app, verbose)?;
            Ok(())
        }

        Commands::Start { app } => {
            flaase::cli::deploy::start(&app, verbose)?;
            Ok(())
        }

        Commands::Restart { app } => {
            flaase::cli::deploy::restart(&app, verbose)?;
            Ok(())
        }

        Commands::Destroy { app } => {
            flaase::cli::deploy::destroy(&app, verbose)?;
            Ok(())
        }

        Commands::Logs { app, follow, lines } => {
            flaase::cli::logs::logs(&app, follow, lines, verbose)?;
            Ok(())
        }

        Commands::Env { command } => match command {
            EnvCommands::List { app, show } => {
                flaase::cli::env::list(&app, show)?;
                Ok(())
            }
            EnvCommands::Set { app, vars } => {
                flaase::cli::env::set(&app, &vars)?;
                Ok(())
            }
            EnvCommands::Remove { app, key } => {
                flaase::cli::env::remove(&app, &key)?;
                Ok(())
            }
            EnvCommands::Edit { app } => {
                flaase::cli::env::edit(&app)?;
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
