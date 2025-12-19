use crate::core::config::ContainerRuntimeInfo;
use crate::core::context::ExecutionContext;
use crate::core::error::AppError;
use crate::providers::package_manager::PackageManager;

/// Trait for container runtime operations.
/// Designed to support Docker now and Kubernetes in the future.
pub trait ContainerRuntime {
    /// Returns the name of the runtime (e.g., "docker", "kubernetes").
    fn name(&self) -> &str;

    /// Returns the runtime type identifier for config.
    fn runtime_type(&self) -> &str;

    /// Checks if the runtime is installed.
    fn is_installed(&self, ctx: &ExecutionContext) -> Result<bool, AppError>;

    /// Gets the installed version.
    fn get_version(&self, ctx: &ExecutionContext) -> Result<String, AppError>;

    /// Installs the container runtime.
    fn install(
        &self,
        pkg_manager: &dyn PackageManager,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError>;

    /// Starts the runtime service.
    fn start_service(&self, ctx: &ExecutionContext) -> Result<(), AppError>;

    /// Enables the runtime service to start on boot.
    fn enable_service(&self, ctx: &ExecutionContext) -> Result<(), AppError>;

    /// Checks if the runtime service is running.
    fn is_running(&self, ctx: &ExecutionContext) -> Result<bool, AppError>;

    /// Gets runtime info for the server config.
    fn get_info(&self, ctx: &ExecutionContext) -> Result<ContainerRuntimeInfo, AppError>;

    /// Runs a container with the specified configuration.
    fn run_container(
        &self,
        config: &ContainerConfig,
        ctx: &ExecutionContext,
    ) -> Result<String, AppError>;

    /// Stops a container by name or ID.
    fn stop_container(&self, name: &str, ctx: &ExecutionContext) -> Result<(), AppError>;

    /// Removes a container by name or ID.
    fn remove_container(&self, name: &str, ctx: &ExecutionContext) -> Result<(), AppError>;

    /// Checks if a container exists.
    fn container_exists(&self, name: &str, ctx: &ExecutionContext) -> Result<bool, AppError>;

    /// Checks if a container is running.
    fn container_is_running(&self, name: &str, ctx: &ExecutionContext) -> Result<bool, AppError>;

    /// Creates a network.
    fn create_network(&self, name: &str, ctx: &ExecutionContext) -> Result<(), AppError>;

    /// Checks if a network exists.
    fn network_exists(&self, name: &str, ctx: &ExecutionContext) -> Result<bool, AppError>;

    /// Builds a Docker image from a Dockerfile.
    fn build_image(
        &self,
        tag: &str,
        context_dir: &str,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError>;

    /// Checks if a port is available on the host.
    fn is_port_available(&self, port: u16, ctx: &ExecutionContext) -> Result<bool, AppError>;

    /// Gets logs from a container.
    fn get_logs(&self, name: &str, lines: u32, ctx: &ExecutionContext) -> Result<String, AppError>;

    /// Pulls a Docker image.
    fn pull_image(&self, image: &str, ctx: &ExecutionContext) -> Result<(), AppError>;

    /// Finds an available port starting from the given port.
    fn find_available_port(&self, start: u16, ctx: &ExecutionContext) -> Result<u16, AppError>;

    /// Connects a container to an additional network.
    fn connect_network(
        &self,
        container: &str,
        network: &str,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError>;

    /// Executes a command inside a running container.
    fn exec_in_container(
        &self,
        container: &str,
        command: &[&str],
        ctx: &ExecutionContext,
    ) -> Result<String, AppError>;
}

/// Configuration for running a container.
#[derive(Debug, Clone)]
pub struct ContainerConfig {
    pub name: String,
    pub image: String,
    pub ports: Vec<PortMapping>,
    pub volumes: Vec<VolumeMapping>,
    pub environment: Vec<(String, String)>,
    pub env_files: Vec<String>,
    pub network: Option<String>,
    pub restart_policy: RestartPolicy,
    pub labels: Vec<(String, String)>,
    pub command: Option<Vec<String>>,
}

impl ContainerConfig {
    pub fn new(name: &str, image: &str) -> Self {
        Self {
            name: name.to_string(),
            image: image.to_string(),
            ports: Vec::new(),
            volumes: Vec::new(),
            environment: Vec::new(),
            env_files: Vec::new(),
            network: None,
            restart_policy: RestartPolicy::UnlessStopped,
            labels: Vec::new(),
            command: None,
        }
    }

    pub fn port(mut self, host: u16, container: u16) -> Self {
        self.ports.push(PortMapping { host, container });
        self
    }

    pub fn volume(mut self, host: &str, container: &str) -> Self {
        self.volumes.push(VolumeMapping {
            host: host.to_string(),
            container: container.to_string(),
            readonly: false,
        });
        self
    }

    pub fn volume_readonly(mut self, host: &str, container: &str) -> Self {
        self.volumes.push(VolumeMapping {
            host: host.to_string(),
            container: container.to_string(),
            readonly: true,
        });
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.environment.push((key.to_string(), value.to_string()));
        self
    }

    pub fn env_file(mut self, path: &str) -> Self {
        self.env_files.push(path.to_string());
        self
    }

    pub fn network(mut self, network: &str) -> Self {
        self.network = Some(network.to_string());
        self
    }

    pub fn restart(mut self, policy: RestartPolicy) -> Self {
        self.restart_policy = policy;
        self
    }

    pub fn label(mut self, key: &str, value: &str) -> Self {
        self.labels.push((key.to_string(), value.to_string()));
        self
    }

    pub fn command(mut self, cmd: Vec<String>) -> Self {
        self.command = Some(cmd);
        self
    }
}

#[derive(Debug, Clone)]
pub struct PortMapping {
    pub host: u16,
    pub container: u16,
}

#[derive(Debug, Clone)]
pub struct VolumeMapping {
    pub host: String,
    pub container: String,
    pub readonly: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum RestartPolicy {
    No,
    Always,
    OnFailure,
    UnlessStopped,
}

impl RestartPolicy {
    pub fn as_str(&self) -> &str {
        match self {
            Self::No => "no",
            Self::Always => "always",
            Self::OnFailure => "on-failure",
            Self::UnlessStopped => "unless-stopped",
        }
    }
}

/// Docker implementation of ContainerRuntime.
pub struct DockerRuntime;

impl DockerRuntime {
    pub fn new() -> Self {
        Self
    }

    /// Required packages for Docker installation.
    fn required_packages() -> &'static [&'static str] {
        &["docker.io", "docker-compose-v2", "containerd"]
    }
}

impl Default for DockerRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ContainerRuntime for DockerRuntime {
    fn name(&self) -> &str {
        "Docker"
    }

    fn runtime_type(&self) -> &str {
        "docker"
    }

    fn is_installed(&self, ctx: &ExecutionContext) -> Result<bool, AppError> {
        match ctx.run_command("which", &["docker"]) {
            Ok(output) => Ok(output.success),
            Err(_) => Ok(false),
        }
    }

    fn get_version(&self, ctx: &ExecutionContext) -> Result<String, AppError> {
        let output = ctx.run_command("docker", &["--version"])?;
        output.ensure_success("Failed to get Docker version")?;

        // Parse version from "Docker version 24.0.7, build ..."
        let version = output
            .stdout
            .split_whitespace()
            .nth(2)
            .map(|v| v.trim_end_matches(',').to_string())
            .unwrap_or_else(|| "unknown".to_string());

        Ok(version)
    }

    fn install(
        &self,
        pkg_manager: &dyn PackageManager,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError> {
        pkg_manager.update(ctx)?;
        pkg_manager.install(Self::required_packages(), ctx)?;
        Ok(())
    }

    fn start_service(&self, ctx: &ExecutionContext) -> Result<(), AppError> {
        ctx.run_command("systemctl", &["start", "docker"])?
            .ensure_success("Failed to start Docker service")?;
        Ok(())
    }

    fn enable_service(&self, ctx: &ExecutionContext) -> Result<(), AppError> {
        ctx.run_command("systemctl", &["enable", "docker"])?
            .ensure_success("Failed to enable Docker service")?;
        Ok(())
    }

    fn is_running(&self, ctx: &ExecutionContext) -> Result<bool, AppError> {
        let output = ctx.run_command("systemctl", &["is-active", "docker"])?;
        Ok(output.success && output.stdout.trim() == "active")
    }

    fn get_info(&self, ctx: &ExecutionContext) -> Result<ContainerRuntimeInfo, AppError> {
        let version = self.get_version(ctx)?;
        Ok(ContainerRuntimeInfo {
            runtime_type: self.runtime_type().to_string(),
            version,
        })
    }

    fn run_container(
        &self,
        config: &ContainerConfig,
        ctx: &ExecutionContext,
    ) -> Result<String, AppError> {
        let mut args = vec!["run", "-d", "--name", &config.name];

        // Restart policy
        args.push("--restart");
        args.push(config.restart_policy.as_str());

        // Network
        if let Some(ref network) = config.network {
            args.push("--network");
            args.push(network);
        }

        // Collect formatted strings that need to live long enough
        let port_mappings: Vec<String> = config
            .ports
            .iter()
            .map(|p| format!("{}:{}", p.host, p.container))
            .collect();

        let volume_mappings: Vec<String> = config
            .volumes
            .iter()
            .map(|v| {
                if v.readonly {
                    format!("{}:{}:ro", v.host, v.container)
                } else {
                    format!("{}:{}", v.host, v.container)
                }
            })
            .collect();

        let env_mappings: Vec<String> = config
            .environment
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        let label_mappings: Vec<String> = config
            .labels
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        // Ports
        for port in &port_mappings {
            args.push("-p");
            args.push(port);
        }

        // Volumes
        for vol in &volume_mappings {
            args.push("-v");
            args.push(vol);
        }

        // Environment
        for env in &env_mappings {
            args.push("-e");
            args.push(env);
        }

        // Environment files
        for env_file in &config.env_files {
            args.push("--env-file");
            args.push(env_file);
        }

        // Labels
        for label in &label_mappings {
            args.push("-l");
            args.push(label);
        }

        // Image
        args.push(&config.image);

        // Command
        let cmd_args: Vec<&str>;
        if let Some(ref cmd) = config.command {
            cmd_args = cmd.iter().map(|s| s.as_str()).collect();
            args.extend(&cmd_args);
        }

        let output = ctx.run_command("docker", &args)?;
        output.ensure_success(&format!("Failed to run container '{}'", config.name))?;

        Ok(output.stdout.trim().to_string())
    }

    fn stop_container(&self, name: &str, ctx: &ExecutionContext) -> Result<(), AppError> {
        ctx.run_command("docker", &["stop", name])?
            .ensure_success(&format!("Failed to stop container '{}'", name))?;
        Ok(())
    }

    fn remove_container(&self, name: &str, ctx: &ExecutionContext) -> Result<(), AppError> {
        ctx.run_command("docker", &["rm", "-f", name])?
            .ensure_success(&format!("Failed to remove container '{}'", name))?;
        Ok(())
    }

    fn container_exists(&self, name: &str, ctx: &ExecutionContext) -> Result<bool, AppError> {
        let output = ctx.run_command(
            "docker",
            &["ps", "-a", "--filter", &format!("name=^{}$", name), "-q"],
        )?;
        Ok(!output.stdout.trim().is_empty())
    }

    fn container_is_running(&self, name: &str, ctx: &ExecutionContext) -> Result<bool, AppError> {
        let output = ctx.run_command(
            "docker",
            &["ps", "--filter", &format!("name=^{}$", name), "-q"],
        )?;
        Ok(!output.stdout.trim().is_empty())
    }

    fn create_network(&self, name: &str, ctx: &ExecutionContext) -> Result<(), AppError> {
        if self.network_exists(name, ctx)? {
            return Ok(());
        }

        ctx.run_command("docker", &["network", "create", name])?
            .ensure_success(&format!("Failed to create network '{}'", name))?;
        Ok(())
    }

    fn network_exists(&self, name: &str, ctx: &ExecutionContext) -> Result<bool, AppError> {
        let output = ctx.run_command(
            "docker",
            &[
                "network",
                "ls",
                "--filter",
                &format!("name=^{}$", name),
                "-q",
            ],
        )?;
        Ok(!output.stdout.trim().is_empty())
    }

    fn build_image(
        &self,
        tag: &str,
        context_dir: &str,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError> {
        ctx.run_command_streaming("docker", &["build", "-t", tag, context_dir])?
            .ensure_success(&format!("Failed to build image '{}'", tag))?;
        Ok(())
    }

    fn is_port_available(&self, port: u16, ctx: &ExecutionContext) -> Result<bool, AppError> {
        // Check if any container is using this port
        let output = ctx.run_command(
            "docker",
            &[
                "ps",
                "--format",
                "{{.Ports}}",
            ],
        )?;

        let port_str = format!(":{}", port);
        let port_mapping = format!("0.0.0.0:{}", port);

        for line in output.stdout.lines() {
            if line.contains(&port_str) || line.contains(&port_mapping) {
                return Ok(false);
            }
        }

        // Also check if the port is bound by a non-Docker process
        let ss_output = ctx.run_command("ss", &["-tuln"]);
        if let Ok(ss) = ss_output {
            let port_pattern = format!(":{} ", port);
            let port_pattern2 = format!(":{}\n", port);
            if ss.stdout.contains(&port_pattern) || ss.stdout.contains(&port_pattern2) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    fn get_logs(&self, name: &str, lines: u32, ctx: &ExecutionContext) -> Result<String, AppError> {
        let lines_str = lines.to_string();
        let output = ctx.run_command("docker", &["logs", "--tail", &lines_str, name])?;
        Ok(format!("{}\n{}", output.stdout, output.stderr))
    }

    fn pull_image(&self, image: &str, ctx: &ExecutionContext) -> Result<(), AppError> {
        ctx.run_command_streaming("docker", &["pull", image])?
            .ensure_success(&format!("Failed to pull image '{}'", image))?;
        Ok(())
    }

    fn find_available_port(&self, start: u16, ctx: &ExecutionContext) -> Result<u16, AppError> {
        let mut port = start;
        let max_attempts = 100;

        for _ in 0..max_attempts {
            if self.is_port_available(port, ctx)? {
                return Ok(port);
            }
            port += 1;
        }

        Err(AppError::Config(format!(
            "Could not find available port starting from {}",
            start
        )))
    }

    fn connect_network(
        &self,
        container: &str,
        network: &str,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError> {
        // Check if already connected
        let output = ctx.run_command(
            "docker",
            &["inspect", container, "--format", "{{range .NetworkSettings.Networks}}{{.NetworkID}}{{end}}"],
        )?;

        // Check if network exists, create if not
        if !self.network_exists(network, ctx)? {
            self.create_network(network, ctx)?;
        }

        // Get network ID to check if already connected
        let network_id = ctx.run_command("docker", &["network", "inspect", network, "-f", "{{.Id}}"])?;

        if output.stdout.contains(&network_id.stdout[..12]) {
            return Ok(()); // Already connected
        }

        ctx.run_command("docker", &["network", "connect", network, container])?
            .ensure_success(&format!(
                "Failed to connect container '{}' to network '{}'",
                container, network
            ))?;

        Ok(())
    }

    fn exec_in_container(
        &self,
        container: &str,
        command: &[&str],
        ctx: &ExecutionContext,
    ) -> Result<String, AppError> {
        let mut args = vec!["exec", container];
        args.extend(command);

        let output = ctx.run_command("docker", &args)?;

        if output.success {
            Ok(output.stdout)
        } else {
            Err(AppError::Docker(format!(
                "Command failed in container: {}",
                output.stderr
            )))
        }
    }
}

/// Creates the appropriate container runtime.
/// Currently only Docker is supported.
pub fn create_container_runtime() -> Box<dyn ContainerRuntime> {
    Box::new(DockerRuntime::new())
}
