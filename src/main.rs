use anyhow::Result;
use clap::Parser;
use flaase::cli::{
    AuthCommands, AutodeployCommands, Cli, Commands, DomainCommands, EnvCommands, ServerCommands,
    WebhookCommands,
};
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
                let exit_code = flaase::cli::server_status::status(verbose)?;
                if exit_code != 0 {
                    std::process::exit(exit_code);
                }
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
            AutodeployCommands::Enable { app, branch } => {
                flaase::cli::autodeploy::enable(&app, branch.as_deref())?;
                Ok(())
            }
            AutodeployCommands::Disable { app } => {
                flaase::cli::autodeploy::disable(&app)?;
                Ok(())
            }
            AutodeployCommands::Status { app } => {
                flaase::cli::autodeploy::status(&app)?;
                Ok(())
            }
            AutodeployCommands::Secret { app } => {
                flaase::cli::autodeploy::secret(&app)?;
                Ok(())
            }
            AutodeployCommands::Regenerate { app } => {
                flaase::cli::autodeploy::regenerate(&app)?;
                Ok(())
            }
            AutodeployCommands::Logs { app, limit } => {
                flaase::cli::autodeploy::logs(&app, limit)?;
                Ok(())
            }
        },

        Commands::Auth { command } => match command {
            AuthCommands::List { app } => {
                flaase::cli::auth::list(&app)?;
                Ok(())
            }
            AuthCommands::Add {
                app,
                domain,
                user,
                password,
            } => {
                flaase::cli::auth::add(&app, &domain, user.as_deref(), password.as_deref())?;
                Ok(())
            }
            AuthCommands::Remove { app, domain } => {
                flaase::cli::auth::remove(&app, &domain)?;
                Ok(())
            }
            AuthCommands::Update {
                app,
                domain,
                user,
                password,
            } => {
                flaase::cli::auth::update(&app, &domain, user.as_deref(), password.as_deref())?;
                Ok(())
            }
        },

        Commands::Webhook { command } => match command {
            WebhookCommands::Serve { port, host } => {
                flaase::cli::webhook::serve(&host, port, verbose)?;
                Ok(())
            }
            WebhookCommands::Install => {
                flaase::cli::webhook::install()?;
                Ok(())
            }
            WebhookCommands::Uninstall => {
                flaase::cli::webhook::uninstall()?;
                Ok(())
            }
            WebhookCommands::Status => {
                flaase::cli::webhook::status()?;
                Ok(())
            }
        },
    }
}
