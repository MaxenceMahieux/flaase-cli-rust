# Flaase

A CLI tool for simplified VPS deployment with Docker. Deploy your apps with a single command, manage environments, and set up complete CI/CD pipelines.

## Installation

```bash
curl -fsSL https://get.flaase.com | sh
```

## Quick Start

```bash
# 1. Initialize your server (once)
fl server init

# 2. Create an app configuration
cd /path/to/your/project
fl init

# 3. Deploy
fl deploy myapp
```

## Features

- **One-command deployment** - Deploy Docker apps with `fl deploy`
- **Zero-downtime updates** - Blue-green deployment support
- **Auto-deploy from GitHub** - Push-to-deploy via webhooks
- **Multi-environment** - Manage staging, production, etc.
- **Environment variables** - Per-environment secrets management
- **SSL/TLS** - Automatic HTTPS via Let's Encrypt
- **Custom domains** - Multiple domains per app
- **Rollback** - Instant rollback to previous versions
- **Notifications** - Slack, Discord, and Email alerts

---

## Commands Reference

### Server Management

```bash
fl server init          # Initialize server for deployments
fl server status        # Show server health status
```

### App Lifecycle

```bash
fl init                 # Initialize app configuration (interactive)
fl deploy <app>         # Deploy an app
fl update <app>         # Update a deployed app
fl stop <app>           # Stop an app
fl start <app>          # Start a stopped app
fl restart <app>        # Restart an app
fl status               # Show status of all apps
```

### Update App (Zero-Downtime)

```bash
fl update <app>         # Pull latest and redeploy
```

The update command:
- Pulls latest changes from repository
- Shows git commit before → after
- Performs zero-downtime deployment (if blue-green enabled)
- Runs health check on new container before switching traffic
- If health check fails, keeps old version running
- Suggests rollback command on failure

### Destroy App

```bash
fl destroy <app>              # Interactive confirmation (type app name)
fl destroy <app> --keep-data  # Keep database/cache volumes
fl destroy <app> --force      # Skip confirmation (for scripting)
fl destroy <app> -y           # Same as --force
```

The destroy command:
- Requires typing the app name to confirm (safety)
- Asks whether to delete data volumes
- Removes containers, network, Traefik config, images
- Warns if app is currently running

### Logs

```bash
fl logs <app>                        # Stream logs (follow by default)
fl logs <app> --no-follow            # Show recent logs and exit
fl logs <app> -n 200                 # Show last 200 lines
fl logs <app> --service database     # Show database logs
fl logs <app> --service cache        # Show Redis cache logs
fl logs <app> --service all          # Show all services
fl logs <app> --since 1h             # Logs from last hour
fl logs <app> --since 30m            # Logs from last 30 minutes
fl logs <app> --since 2024-01-15     # Logs since date
```

Logs are colorized:
- **Red**: Errors, fatal, panic, exceptions
- **Yellow**: Warnings, deprecated
- **Green**: Success, started, connected, ready
- **Dim**: Debug, trace

### Rollback

```bash
fl rollback <app>                # Rollback to previous version
fl rollback <app> --list         # List available versions
fl rollback <app> --to <sha>     # Rollback to specific commit
```

### Environment Variables

```bash
fl env list <app>                      # List variables (production)
fl env list <app> --env staging        # List variables (staging)
fl env set <app> KEY=value             # Set variable
fl env set <app> KEY=value --env staging
fl env remove <app> KEY                # Remove variable
fl env edit <app>                      # Edit in $EDITOR
fl env copy <app> production staging   # Copy between environments
fl env envs <app>                      # List all environments
```

### Custom Domains

```bash
fl domain list <app>                   # List domains
fl domain add <app> api.example.com    # Add domain
fl domain remove <app> api.example.com # Remove domain
```

### HTTP Basic Auth

```bash
fl auth list <app>                     # List protected domains
fl auth add <app> staging.example.com  # Add protection
fl auth remove <app> staging.example.com
fl auth update <app> staging.example.com
```

---

## CI/CD Configuration

### Enable Auto-Deploy

```bash
fl autodeploy enable <app>             # Enable GitHub webhook
fl autodeploy disable <app>            # Disable
fl autodeploy status <app>             # Show status
fl autodeploy secret <app>             # Show webhook secret
fl autodeploy logs <app>               # View deployment logs
```

### Multi-Environment Deployments

```bash
# Add environments with branch mapping
fl autodeploy env add <app> staging develop --auto-deploy
fl autodeploy env add <app> production main
fl autodeploy env list <app>
fl autodeploy env remove <app> staging
```

### Test Execution

```bash
fl autodeploy test <app> --enable --command "npm test"
fl autodeploy test <app> --timeout 600
fl autodeploy test <app> --disable
```

### Deployment Hooks

```bash
# Add hooks (phases: pre_build, pre_deploy, post_deploy, on_failure)
fl autodeploy hooks add <app> post_deploy migrate "npm run db:migrate" --required
fl autodeploy hooks add <app> pre_deploy backup "./backup.sh" --timeout 120
fl autodeploy hooks list <app>
fl autodeploy hooks remove <app> post_deploy migrate
```

### Blue-Green Deployment (Zero-Downtime)

```bash
fl autodeploy blue-green <app> --enable
fl autodeploy blue-green <app> --keep-old 300   # Keep old container 5min
fl autodeploy blue-green <app> --disable
```

### Rollback Configuration

```bash
fl autodeploy rollback-config <app> --enable
fl autodeploy rollback-config <app> --auto-rollback true
fl autodeploy rollback-config <app> --keep-versions 5
```

### Approval Gates

```bash
fl autodeploy approval config <app> --enable
fl autodeploy approval config <app> --timeout 120  # Minutes
fl autodeploy approval pending <app>
fl autodeploy approval approve <app>
fl autodeploy approval reject <app>
```

### Docker Build Optimization

```bash
fl autodeploy build <app> --cache true
fl autodeploy build <app> --buildkit true
fl autodeploy build <app> --cache-from registry.example.com/myapp
```

### Rate Limiting

```bash
fl autodeploy rate-limit <app> --enable --max-deploys 5 --window 3600
fl autodeploy rate-limit <app> --disable
```

---

## Notifications

### Slack

```bash
fl autodeploy notify slack <app> --webhook-url "https://hooks.slack.com/..."
fl autodeploy notify slack <app> --channel "#deployments"
fl autodeploy notify slack <app> --remove
```

### Discord

```bash
fl autodeploy notify discord <app> --webhook-url "https://discord.com/api/webhooks/..."
fl autodeploy notify discord <app> --remove
```

### Email (SMTP)

```bash
fl autodeploy notify email <app> \
  --smtp-host smtp.gmail.com \
  --smtp-port 587 \
  --smtp-user user@gmail.com \
  --smtp-password "app-password" \
  --from-email noreply@example.com \
  --from-name "Flaase Deploy" \
  --to-emails "dev@example.com,ops@example.com"

fl autodeploy notify email <app> --remove
```

### Notification Events

```bash
fl autodeploy notify events <app> --on-start true
fl autodeploy notify events <app> --on-success true
fl autodeploy notify events <app> --on-failure true
fl autodeploy notify test <app>   # Send test notification
```

---

## Webhook Server

The webhook server receives GitHub events for auto-deployment.

```bash
fl webhook install      # Install as systemd service
fl webhook uninstall    # Remove service
fl webhook status       # Show status
fl webhook serve        # Run manually (for testing)
```

---

## Example: Complete CI/CD Setup

```bash
# 1. Initialize and deploy
fl init
fl deploy myapp

# 2. Enable auto-deploy
fl autodeploy enable myapp --branch main

# 3. Configure environments
fl autodeploy env add myapp staging develop --auto-deploy
fl autodeploy env add myapp production main

# 4. Set environment variables
fl env set myapp DATABASE_URL=postgres://...
fl env set myapp API_KEY=staging_key --env staging

# 5. Configure tests and hooks
fl autodeploy test myapp --enable --command "npm test"
fl autodeploy hooks add myapp post_deploy migrate "npm run db:migrate" --required

# 6. Enable zero-downtime deployment
fl autodeploy blue-green myapp --enable

# 7. Configure auto-rollback
fl autodeploy rollback-config myapp --enable --auto-rollback true

# 8. Set up notifications
fl autodeploy notify slack myapp --webhook-url "https://hooks.slack.com/..."
fl autodeploy notify events myapp --on-failure true --on-success true

# 9. Require approval for production
fl autodeploy approval config myapp --enable

# 10. Install webhook server
fl webhook install
```

---

## App Configuration

Apps are configured via `flaase.yaml`:

```yaml
name: myapp
port: 3000
domains:
  - myapp.example.com
  - api.example.com

# Optional: Database
database:
  type: postgresql  # postgresql, mysql, mongodb

# Optional: Cache
cache:
  type: redis

# Optional: Health check
healthcheck:
  path: /health
  interval: 30
  timeout: 10
```

---

## Requirements

- Linux VPS (Ubuntu 20.04+ recommended)
- Docker installed
- Root access (for initial setup)

---

## License

MIT
