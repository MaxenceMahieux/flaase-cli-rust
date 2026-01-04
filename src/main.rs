use anyhow::Result;
use clap::Parser;
use flaase::cli::{
    ApprovalCommands, AuthCommands, AutodeployCommands, Cli, Commands, DomainCommands,
    EnvCommands, EnvDeployCommands, HooksCommands, NotifyCommands, ServerCommands, WebhookCommands,
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

        Commands::Destroy { app, force, keep_data } => {
            flaase::cli::deploy::destroy(&app, force, keep_data, verbose)?;
            Ok(())
        }

        Commands::Rollback { app, to, list } => {
            flaase::cli::deploy::rollback(&app, to.as_deref(), list, verbose)?;
            Ok(())
        }

        Commands::Logs {
            app,
            follow,
            no_follow,
            lines,
            service,
            since,
        } => {
            flaase::cli::logs::logs(&app, follow, no_follow, lines, &service, since.as_deref(), verbose)?;
            Ok(())
        }

        Commands::Env { command } => match command {
            EnvCommands::List { app, show, env } => {
                flaase::cli::env::list(&app, show, env.as_deref())?;
                Ok(())
            }
            EnvCommands::Set { app, vars, env } => {
                flaase::cli::env::set(&app, &vars, env.as_deref())?;
                Ok(())
            }
            EnvCommands::Remove { app, key, env } => {
                flaase::cli::env::remove(&app, &key, env.as_deref())?;
                Ok(())
            }
            EnvCommands::Edit { app, env } => {
                flaase::cli::env::edit(&app, env.as_deref())?;
                Ok(())
            }
            EnvCommands::Copy { app, from, to } => {
                flaase::cli::env::copy(&app, &from, &to)?;
                Ok(())
            }
            EnvCommands::Envs { app } => {
                flaase::cli::env::envs(&app)?;
                Ok(())
            }
        },

        Commands::Domain { command } => match command {
            DomainCommands::List { app } => {
                flaase::cli::domain::list(&app)?;
                Ok(())
            }
            DomainCommands::Add {
                app,
                domain,
                skip_dns_check,
            } => {
                flaase::cli::domain::add(&app, &domain, skip_dns_check)?;
                Ok(())
            }
            DomainCommands::Remove { app, domain } => {
                flaase::cli::domain::remove(&app, &domain)?;
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
            AutodeployCommands::Notify(notify_cmd) => match notify_cmd {
                NotifyCommands::Status { app } => {
                    flaase::cli::autodeploy::notify_status(&app)?;
                    Ok(())
                }
                NotifyCommands::Enable { app } => {
                    flaase::cli::autodeploy::notify_enable(&app)?;
                    Ok(())
                }
                NotifyCommands::Disable { app } => {
                    flaase::cli::autodeploy::notify_disable(&app)?;
                    Ok(())
                }
                NotifyCommands::Slack {
                    app,
                    webhook_url,
                    channel,
                    username,
                    remove,
                } => {
                    flaase::cli::autodeploy::notify_slack(
                        &app,
                        webhook_url.as_deref(),
                        channel.as_deref(),
                        username.as_deref(),
                        remove,
                    )?;
                    Ok(())
                }
                NotifyCommands::Discord {
                    app,
                    webhook_url,
                    username,
                    remove,
                } => {
                    flaase::cli::autodeploy::notify_discord(
                        &app,
                        webhook_url.as_deref(),
                        username.as_deref(),
                        remove,
                    )?;
                    Ok(())
                }
                NotifyCommands::Email {
                    app,
                    smtp_host,
                    smtp_port,
                    smtp_user,
                    smtp_password,
                    from_email,
                    from_name,
                    to_emails,
                    starttls,
                    remove,
                } => {
                    flaase::cli::autodeploy::notify_email(
                        &app,
                        smtp_host.as_deref(),
                        smtp_port,
                        smtp_user.as_deref(),
                        smtp_password.as_deref(),
                        from_email.as_deref(),
                        from_name.as_deref(),
                        to_emails.as_deref(),
                        starttls,
                        remove,
                    )?;
                    Ok(())
                }
                NotifyCommands::Events {
                    app,
                    on_start,
                    on_success,
                    on_failure,
                } => {
                    flaase::cli::autodeploy::notify_events(&app, on_start, on_success, on_failure)?;
                    Ok(())
                }
                NotifyCommands::Test { app } => {
                    flaase::cli::autodeploy::notify_test(&app)?;
                    Ok(())
                }
            },
            AutodeployCommands::RateLimit {
                app,
                enable,
                disable,
                max_deploys,
                window,
            } => {
                flaase::cli::autodeploy::rate_limit(&app, enable, disable, max_deploys, window)?;
                Ok(())
            }
            AutodeployCommands::Test {
                app,
                enable,
                disable,
                command,
                timeout,
                fail_on_error,
            } => {
                flaase::cli::autodeploy::test_config(
                    &app,
                    enable,
                    disable,
                    command.as_deref(),
                    timeout,
                    fail_on_error,
                )?;
                Ok(())
            }
            AutodeployCommands::Hooks(hooks_cmd) => match hooks_cmd {
                HooksCommands::List { app } => {
                    flaase::cli::autodeploy::hooks_list(&app)?;
                    Ok(())
                }
                HooksCommands::Add {
                    app,
                    phase,
                    name,
                    command,
                    timeout,
                    required,
                    in_container,
                } => {
                    flaase::cli::autodeploy::hooks_add(
                        &app,
                        &phase,
                        &name,
                        &command,
                        Some(timeout),
                        required,
                        in_container,
                    )?;
                    Ok(())
                }
                HooksCommands::Remove { app, phase, name } => {
                    flaase::cli::autodeploy::hooks_remove(&app, &phase, &name)?;
                    Ok(())
                }
            },
            AutodeployCommands::RollbackConfig {
                app,
                enable,
                disable,
                keep_versions,
                auto_rollback,
            } => {
                flaase::cli::autodeploy::rollback_config(
                    &app,
                    enable,
                    disable,
                    keep_versions,
                    auto_rollback,
                )?;
                Ok(())
            }
            AutodeployCommands::Env(env_cmd) => match env_cmd {
                EnvDeployCommands::List { app } => {
                    flaase::cli::autodeploy::env_list(&app)?;
                    Ok(())
                }
                EnvDeployCommands::Add {
                    app,
                    name,
                    branch,
                    auto_deploy,
                    domain,
                } => {
                    flaase::cli::autodeploy::env_add(&app, &name, &branch, auto_deploy, domain)?;
                    Ok(())
                }
                EnvDeployCommands::Remove { app, name } => {
                    flaase::cli::autodeploy::env_remove(&app, &name)?;
                    Ok(())
                }
            },
            AutodeployCommands::Approval(approval_cmd) => match approval_cmd {
                ApprovalCommands::Config {
                    app,
                    enable,
                    disable,
                    timeout,
                } => {
                    flaase::cli::autodeploy::approval_config(&app, enable, disable, timeout)?;
                    Ok(())
                }
                ApprovalCommands::Pending { app } => {
                    flaase::cli::autodeploy::approval_pending(&app)?;
                    Ok(())
                }
                ApprovalCommands::Approve { app, approval_id } => {
                    flaase::cli::autodeploy::approval_approve(&app, approval_id.as_deref())?;
                    Ok(())
                }
                ApprovalCommands::Reject { app, approval_id } => {
                    flaase::cli::autodeploy::approval_reject(&app, approval_id.as_deref())?;
                    Ok(())
                }
            },
            AutodeployCommands::Build {
                app,
                cache,
                buildkit,
                cache_from,
            } => {
                flaase::cli::autodeploy::build_config(&app, cache, buildkit, cache_from.as_deref())?;
                Ok(())
            }
            AutodeployCommands::BlueGreen {
                app,
                enable,
                disable,
                keep_old,
                no_auto_cleanup,
            } => {
                flaase::cli::autodeploy::blue_green_config(&app, enable, disable, keep_old, no_auto_cleanup)?;
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
