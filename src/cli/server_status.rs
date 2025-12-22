//! Server status command implementation.

use chrono::{DateTime, TimeZone, Utc};
use console::{style, Term};
use serde::Deserialize;
use std::process::Command;

use crate::core::app_config::AppConfig;
use crate::core::config::{ServerConfig, FLAASE_TRAEFIK_PATH};
use crate::core::context::ExecutionContext;
use crate::core::error::AppError;
use crate::providers::container::{ContainerRuntime, DockerRuntime};
use crate::providers::reverse_proxy::TraefikProxy;
use crate::providers::ReverseProxy;
use crate::ui;

/// Service status for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Running,
    Stopped,
    NotInstalled,
}

impl ServiceStatus {
    pub fn display(&self) -> console::StyledObject<&'static str> {
        match self {
            ServiceStatus::Running => style("running").green(),
            ServiceStatus::Stopped => style("stopped").red(),
            ServiceStatus::NotInstalled => style("not installed").dim(),
        }
    }

    pub fn is_critical_failure(&self) -> bool {
        matches!(self, ServiceStatus::Stopped | ServiceStatus::NotInstalled)
    }
}

/// Resource usage level for coloring.
#[derive(Debug, Clone, Copy)]
pub enum UsageLevel {
    Normal,  // < 70%
    Warning, // 70-90%
    Critical, // > 90%
}

impl UsageLevel {
    pub fn from_percentage(pct: f64) -> Self {
        if pct >= 90.0 {
            UsageLevel::Critical
        } else if pct >= 70.0 {
            UsageLevel::Warning
        } else {
            UsageLevel::Normal
        }
    }

    pub fn style_percentage(&self, text: &str) -> String {
        match self {
            UsageLevel::Normal => style(text).green().to_string(),
            UsageLevel::Warning => style(text).yellow().to_string(),
            UsageLevel::Critical => style(text).red().to_string(),
        }
    }
}

/// Service information.
struct ServiceInfo {
    name: String,
    status: ServiceStatus,
    version: String,
}

/// Memory information in bytes.
struct MemoryInfo {
    used: u64,
    total: u64,
}

impl MemoryInfo {
    fn percentage(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.used as f64 / self.total as f64) * 100.0
        }
    }

    fn format(&self) -> String {
        let used_gb = self.used as f64 / 1_073_741_824.0;
        let total_gb = self.total as f64 / 1_073_741_824.0;
        format!("{:.1} GB / {:.1} GB", used_gb, total_gb)
    }
}

/// Disk information in bytes.
struct DiskInfo {
    used: u64,
    total: u64,
}

impl DiskInfo {
    fn percentage(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.used as f64 / self.total as f64) * 100.0
        }
    }

    fn format(&self) -> String {
        let used_gb = self.used as f64 / 1_073_741_824.0;
        let total_gb = self.total as f64 / 1_073_741_824.0;
        format!("{:.0} GB / {:.0} GB", used_gb, total_gb)
    }
}

/// Apps summary.
struct AppsSummary {
    running: usize,
    stopped: usize,
    error: usize,
    not_deployed: usize,
}

impl AppsSummary {
    fn total(&self) -> usize {
        self.running + self.stopped + self.error + self.not_deployed
    }

    fn format(&self) -> String {
        let mut parts = Vec::new();

        if self.running > 0 {
            parts.push(format!("{} running", self.running));
        }
        if self.stopped > 0 {
            parts.push(format!("{} stopped", self.stopped));
        }
        if self.error > 0 {
            parts.push(format!("{} error", self.error));
        }
        if self.not_deployed > 0 {
            parts.push(format!("{} not deployed", self.not_deployed));
        }

        if parts.is_empty() {
            "No apps".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// SSL certificate info.
struct SslInfo {
    domain: String,
    expires_at: Option<DateTime<Utc>>,
}

impl SslInfo {
    fn format_expiry(&self) -> String {
        match self.expires_at {
            Some(dt) => {
                let now = Utc::now();
                let days = (dt - now).num_days();
                let date_str = dt.format("%Y-%m-%d").to_string();

                if days < 0 {
                    format!("{} (expired)", style(date_str).red())
                } else if days < 7 {
                    format!("{} ({} days)", style(date_str).red(), days)
                } else if days < 30 {
                    format!("{} ({} days)", style(date_str).yellow(), days)
                } else {
                    format!("{} ({} days)", style(date_str).green(), days)
                }
            }
            None => style("unknown").dim().to_string(),
        }
    }
}

/// Gets Docker service status and version.
fn get_docker_info(runtime: &DockerRuntime, ctx: &ExecutionContext) -> ServiceInfo {
    let is_running = runtime.is_running(ctx).unwrap_or(false);
    let is_installed = runtime.is_installed(ctx).unwrap_or(false);

    let status = if is_running {
        ServiceStatus::Running
    } else if is_installed {
        ServiceStatus::Stopped
    } else {
        ServiceStatus::NotInstalled
    };

    let version = if is_running {
        runtime
            .get_version(ctx)
            .map(|v| format!("v{}", v))
            .unwrap_or_else(|_| "-".to_string())
    } else {
        "-".to_string()
    };

    ServiceInfo {
        name: "Docker".to_string(),
        status,
        version,
    }
}

/// Gets Traefik service status and version.
fn get_traefik_info(
    proxy: &TraefikProxy,
    runtime: &DockerRuntime,
    ctx: &ExecutionContext,
) -> ServiceInfo {
    let is_running = proxy.is_running(runtime, ctx).unwrap_or(false);
    let is_installed = proxy.is_installed(runtime, ctx).unwrap_or(false);

    let status = if is_running {
        ServiceStatus::Running
    } else if is_installed {
        ServiceStatus::Stopped
    } else {
        ServiceStatus::NotInstalled
    };

    let version = if is_running {
        proxy
            .get_version(runtime, ctx)
            .map(|v| format!("v{}", v))
            .unwrap_or_else(|_| "-".to_string())
    } else {
        "-".to_string()
    };

    ServiceInfo {
        name: "Traefik".to_string(),
        status,
        version,
    }
}

/// Gets CPU usage percentage.
fn get_cpu_usage() -> Option<f64> {
    // Use top command for a quick snapshot
    let output = Command::new("top")
        .args(["-bn1"])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse the %Cpu line: "%Cpu(s):  1.2 us,  0.3 sy,  0.0 ni, 98.4 id, ..."
    for line in stdout.lines() {
        if line.contains("%Cpu") || line.contains("Cpu(s)") {
            // Extract idle percentage and calculate usage
            if let Some(idle_str) = line.split(',').find(|s| s.contains("id")) {
                let idle: f64 = idle_str
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                return Some(100.0 - idle);
            }
        }
    }

    None
}

/// Gets memory usage information.
fn get_memory_info() -> Option<MemoryInfo> {
    // Try /proc/meminfo first (Linux)
    if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
        let mut total: u64 = 0;
        let mut available: u64 = 0;

        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                total = parse_meminfo_value(line);
            } else if line.starts_with("MemAvailable:") {
                available = parse_meminfo_value(line);
            }
        }

        if total > 0 {
            return Some(MemoryInfo {
                used: total.saturating_sub(available),
                total,
            });
        }
    }

    // Fallback to free command
    let output = Command::new("free").args(["-b"]).output().ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        if line.starts_with("Mem:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let total: u64 = parts[1].parse().ok()?;
                let used: u64 = parts[2].parse().ok()?;
                return Some(MemoryInfo { used, total });
            }
        }
    }

    None
}

/// Parses a value from /proc/meminfo (in kB).
fn parse_meminfo_value(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u64>().ok())
        .map(|kb| kb * 1024) // Convert to bytes
        .unwrap_or(0)
}

/// Gets disk usage information for root partition.
fn get_disk_info() -> Option<DiskInfo> {
    let output = Command::new("df")
        .args(["-B1", "/"])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let total: u64 = parts[1].parse().ok()?;
            let used: u64 = parts[2].parse().ok()?;
            return Some(DiskInfo { used, total });
        }
    }

    None
}

/// Gets server uptime.
fn get_uptime() -> Option<String> {
    // Try /proc/uptime first (Linux)
    if let Ok(content) = std::fs::read_to_string("/proc/uptime") {
        if let Some(seconds_str) = content.split_whitespace().next() {
            if let Ok(seconds) = seconds_str.parse::<f64>() {
                return Some(format_uptime(seconds as u64));
            }
        }
    }

    // Fallback to uptime command
    let output = Command::new("uptime").args(["-s"]).output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Parse datetime and calculate uptime
    if let Ok(boot_time) = chrono::NaiveDateTime::parse_from_str(&stdout, "%Y-%m-%d %H:%M:%S") {
        let boot_utc = Utc.from_utc_datetime(&boot_time);
        let duration = Utc::now().signed_duration_since(boot_utc);
        return Some(format_uptime(duration.num_seconds() as u64));
    }

    None
}

/// Formats uptime seconds into human readable string.
fn format_uptime(total_seconds: u64) -> String {
    let days = total_seconds / 86400;
    let hours = (total_seconds % 86400) / 3600;
    let minutes = (total_seconds % 3600) / 60;

    if days > 0 {
        format!("{} days, {} hours, {} minutes", days, hours, minutes)
    } else if hours > 0 {
        format!("{} hours, {} minutes", hours, minutes)
    } else {
        format!("{} minutes", minutes)
    }
}

/// Gets apps summary (running/stopped counts).
fn get_apps_summary(runtime: &DockerRuntime, ctx: &ExecutionContext) -> AppsSummary {
    let app_names = AppConfig::list_all().unwrap_or_default();

    let mut summary = AppsSummary {
        running: 0,
        stopped: 0,
        error: 0,
        not_deployed: 0,
    };

    for name in app_names {
        match AppConfig::load(&name) {
            Ok(config) => {
                if config.deployed_at.is_none() {
                    summary.not_deployed += 1;
                    continue;
                }

                let container_name = format!("flaase-{}-web", name);
                match runtime.container_is_running(&container_name, ctx) {
                    Ok(true) => summary.running += 1,
                    Ok(false) => {
                        match runtime.container_exists(&container_name, ctx) {
                            Ok(true) => summary.stopped += 1,
                            Ok(false) => summary.not_deployed += 1,
                            Err(_) => summary.error += 1,
                        }
                    }
                    Err(_) => summary.error += 1,
                }
            }
            Err(_) => summary.error += 1,
        }
    }

    summary
}

/// ACME certificate structure for parsing acme.json.
#[derive(Debug, Deserialize)]
struct AcmeData {
    #[serde(default)]
    letsencrypt: Option<AcmeResolver>,
}

#[derive(Debug, Deserialize)]
struct AcmeResolver {
    #[serde(rename = "Certificates", default)]
    certificates: Option<Vec<AcmeCertificate>>,
}

#[derive(Debug, Deserialize)]
struct AcmeCertificate {
    domain: AcmeDomain,
    certificate: String,
}

#[derive(Debug, Deserialize)]
struct AcmeDomain {
    main: String,
}

/// Gets SSL certificate information from acme.json.
fn get_ssl_info() -> Vec<SslInfo> {
    let acme_path = format!("{}/acme.json", FLAASE_TRAEFIK_PATH);

    let content = match std::fs::read_to_string(&acme_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let acme_data: AcmeData = match serde_json::from_str(&content) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    let certificates = match acme_data.letsencrypt.and_then(|r| r.certificates) {
        Some(certs) => certs,
        None => return Vec::new(),
    };

    let mut ssl_infos = Vec::new();

    for cert in certificates {
        let expires_at = parse_certificate_expiry(&cert.certificate);
        ssl_infos.push(SslInfo {
            domain: cert.domain.main,
            expires_at,
        });
    }

    ssl_infos
}

/// Parses certificate expiry from base64 encoded certificate.
fn parse_certificate_expiry(cert_base64: &str) -> Option<DateTime<Utc>> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let cert_pem = STANDARD.decode(cert_base64).ok()?;
    let cert_str = String::from_utf8_lossy(&cert_pem);

    // Use openssl to parse the certificate
    let output = Command::new("openssl")
        .args(["x509", "-noout", "-enddate"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;

    use std::io::Write;
    output.stdin.as_ref()?.write_all(cert_str.as_bytes()).ok()?;

    let output = output.wait_with_output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse: notAfter=Dec 21 12:00:00 2025 GMT
    if let Some(date_str) = stdout.strip_prefix("notAfter=") {
        let date_str = date_str.trim();
        // Parse the date format
        if let Ok(dt) = chrono::DateTime::parse_from_str(
            date_str,
            "%b %d %H:%M:%S %Y %Z",
        ) {
            return Some(dt.with_timezone(&Utc));
        }
        // Try alternative format
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(
            &date_str.replace(" GMT", ""),
            "%b %d %H:%M:%S %Y",
        ) {
            return Some(Utc.from_utc_datetime(&dt));
        }
    }

    None
}

/// Prints the services table.
fn print_services_table(term: &Term, services: &[ServiceInfo]) {
    ui::section("Server Status");

    let _ = term.write_line(&format!(
        "  {:<12}  {:<14}  {}",
        style("SERVICE").dim(),
        style("STATUS").dim(),
        style("VERSION").dim()
    ));

    let separator = format!("  {}", "─".repeat(42));
    let _ = term.write_line(&style(separator).dim().to_string());

    for service in services {
        let _ = term.write_line(&format!(
            "  {:<12}  {:<24}  {}",
            service.name,
            service.status.display(),
            service.version
        ));
    }
}

/// Prints the resources section.
fn print_resources(term: &Term, cpu: Option<f64>, memory: Option<&MemoryInfo>, disk: Option<&DiskInfo>, uptime: Option<&str>) {
    ui::section("Resources");

    // Uptime
    if let Some(up) = uptime {
        let _ = term.write_line(&format!("  {:<12}  {}", style("Uptime").dim(), up));
    }

    // CPU
    if let Some(cpu_pct) = cpu {
        let level = UsageLevel::from_percentage(cpu_pct);
        let _ = term.write_line(&format!(
            "  {:<12}  {}",
            style("CPU").dim(),
            level.style_percentage(&format!("{:.0}%", cpu_pct))
        ));
    }

    // Memory
    if let Some(mem) = memory {
        let pct = mem.percentage();
        let level = UsageLevel::from_percentage(pct);
        let _ = term.write_line(&format!(
            "  {:<12}  {} ({})",
            style("Memory").dim(),
            mem.format(),
            level.style_percentage(&format!("{:.0}%", pct))
        ));
    }

    // Disk
    if let Some(dsk) = disk {
        let pct = dsk.percentage();
        let level = UsageLevel::from_percentage(pct);
        let _ = term.write_line(&format!(
            "  {:<12}  {} ({})",
            style("Disk").dim(),
            dsk.format(),
            level.style_percentage(&format!("{:.0}%", pct))
        ));
    }
}

/// Prints the SSL certificates section.
fn print_ssl_info(term: &Term, ssl_infos: &[SslInfo]) {
    if ssl_infos.is_empty() {
        return;
    }

    ui::section("SSL Certificates");

    let _ = term.write_line(&format!(
        "  {:<30}  {}",
        style("DOMAIN").dim(),
        style("EXPIRES").dim()
    ));

    let separator = format!("  {}", "─".repeat(50));
    let _ = term.write_line(&style(separator).dim().to_string());

    for ssl in ssl_infos {
        let _ = term.write_line(&format!(
            "  {:<30}  {}",
            ssl.domain,
            ssl.format_expiry()
        ));
    }
}

/// Prints the apps summary.
fn print_apps_summary(term: &Term, summary: &AppsSummary) {
    ui::section("Apps");

    if summary.total() == 0 {
        let _ = term.write_line(&format!("  {}", style("No apps configured").dim()));
    } else {
        let _ = term.write_line(&format!("  {}", summary.format()));
    }
}

/// Main server status command handler.
pub fn status(_verbose: bool) -> Result<i32, AppError> {
    let term = Term::stdout();

    // Check if server is initialized
    if !ServerConfig::is_initialized() {
        ui::error("Server not initialized");
        ui::info("Run 'fl server init' to set up this server");
        return Ok(1);
    }

    let ctx = ExecutionContext::new(false, false);
    let runtime = DockerRuntime::new();
    let proxy = TraefikProxy::new();

    // Gather service information
    let docker_info = get_docker_info(&runtime, &ctx);
    let traefik_info = get_traefik_info(&proxy, &runtime, &ctx);
    let services = vec![docker_info, traefik_info];

    // Gather resource information
    let cpu = get_cpu_usage();
    let memory = get_memory_info();
    let disk = get_disk_info();
    let uptime = get_uptime();

    // Gather apps summary
    let apps_summary = get_apps_summary(&runtime, &ctx);

    // Gather SSL info
    let ssl_infos = get_ssl_info();

    // Print everything
    print_services_table(&term, &services);
    print_resources(&term, cpu, memory.as_ref(), disk.as_ref(), uptime.as_deref());
    print_ssl_info(&term, &ssl_infos);
    print_apps_summary(&term, &apps_summary);

    println!();

    // Determine exit code
    let critical_service_down = services.iter().any(|s| s.status.is_critical_failure());
    let disk_critical = disk.as_ref().map(|d| d.percentage() >= 90.0).unwrap_or(false);

    if critical_service_down {
        Ok(1)
    } else if disk_critical {
        Ok(2)
    } else {
        Ok(0)
    }
}
