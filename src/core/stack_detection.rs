//! Stack detection from repository files.
//!
//! This module analyzes repository files to detect the runtime,
//! package manager, and framework used by an application.

use std::path::Path;

use crate::core::app_config::{Framework, PackageManager, Stack};

/// Result of stack detection.
#[derive(Debug, Clone, Default)]
pub struct DetectionResult {
    /// Detected stack/runtime
    pub stack: Option<Stack>,
    /// Detected package manager
    pub package_manager: Option<PackageManager>,
    /// Detected framework
    pub framework: Option<Framework>,
    /// Detected runtime version
    pub version: Option<String>,
    /// Whether a Dockerfile exists in the repo
    pub has_dockerfile: bool,
    /// Confidence level
    pub confidence: DetectionConfidence,
    /// Files that were used for detection
    pub detected_files: Vec<String>,
}

/// Confidence level of detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetectionConfidence {
    /// High confidence - explicit files like Cargo.toml, go.mod
    High,
    /// Medium confidence - common patterns detected
    Medium,
    /// Low confidence - heuristics only
    Low,
    /// Nothing detected
    #[default]
    None,
}

impl DetectionConfidence {
    pub fn display(&self) -> &str {
        match self {
            DetectionConfidence::High => "high confidence",
            DetectionConfidence::Medium => "medium confidence",
            DetectionConfidence::Low => "low confidence",
            DetectionConfidence::None => "not detected",
        }
    }
}

/// Detects the stack from repository files.
pub fn detect_stack(repo_path: &Path) -> DetectionResult {
    let mut result = DetectionResult::default();

    // Check for Dockerfile first
    result.has_dockerfile = repo_path.join("Dockerfile").exists();

    // Try to detect stack in order of specificity
    // Most specific first (Rust, Go) then less specific (Node.js, Python)

    // Rust - Cargo.toml is definitive
    if repo_path.join("Cargo.toml").exists() {
        result.stack = Some(Stack::Rust);
        result.package_manager = Some(PackageManager::Cargo);
        result.confidence = DetectionConfidence::High;
        result.detected_files.push("Cargo.toml".to_string());
        detect_rust_details(repo_path, &mut result);
        return result;
    }

    // Go - go.mod is definitive
    if repo_path.join("go.mod").exists() {
        result.stack = Some(Stack::Go);
        result.package_manager = Some(PackageManager::GoMod);
        result.confidence = DetectionConfidence::High;
        result.detected_files.push("go.mod".to_string());
        detect_go_details(repo_path, &mut result);
        return result;
    }

    // Ruby - Gemfile is definitive
    if repo_path.join("Gemfile").exists() {
        result.stack = Some(Stack::Ruby);
        result.package_manager = Some(PackageManager::Bundler);
        result.confidence = DetectionConfidence::High;
        result.detected_files.push("Gemfile".to_string());
        detect_ruby_details(repo_path, &mut result);
        return result;
    }

    // Java - pom.xml or build.gradle
    if repo_path.join("pom.xml").exists() {
        result.stack = Some(Stack::Java);
        result.package_manager = Some(PackageManager::Maven);
        result.confidence = DetectionConfidence::High;
        result.detected_files.push("pom.xml".to_string());
        detect_java_details(repo_path, &mut result);
        return result;
    }
    if repo_path.join("build.gradle").exists() || repo_path.join("build.gradle.kts").exists() {
        result.stack = Some(Stack::Java);
        result.package_manager = Some(PackageManager::Gradle);
        result.confidence = DetectionConfidence::High;
        result.detected_files.push("build.gradle".to_string());
        detect_java_details(repo_path, &mut result);
        return result;
    }

    // PHP/Laravel - composer.json
    if repo_path.join("composer.json").exists() {
        result.package_manager = Some(PackageManager::Composer);
        result.detected_files.push("composer.json".to_string());

        // Check if it's Laravel
        if repo_path.join("artisan").exists() {
            result.stack = Some(Stack::Laravel);
            result.confidence = DetectionConfidence::High;
            result.detected_files.push("artisan".to_string());
        } else {
            result.stack = Some(Stack::Php);
            result.confidence = DetectionConfidence::High;
            detect_php_details(repo_path, &mut result);
        }
        return result;
    }

    // Python - multiple indicators
    if let Some(pm) = detect_python_package_manager(repo_path) {
        result.stack = Some(Stack::Python);
        result.package_manager = Some(pm);
        result.confidence = DetectionConfidence::High;
        match pm {
            PackageManager::Poetry | PackageManager::Uv => {
                result.detected_files.push("pyproject.toml".to_string());
            }
            PackageManager::Pipenv => {
                result.detected_files.push("Pipfile".to_string());
            }
            PackageManager::Pip => {
                result.detected_files.push("requirements.txt".to_string());
            }
            _ => {}
        }
        detect_python_details(repo_path, &mut result);
        return result;
    }

    // Node.js - package.json (check last because it's very common)
    if repo_path.join("package.json").exists() {
        result.detected_files.push("package.json".to_string());
        detect_nodejs_details(repo_path, &mut result);
        return result;
    }

    // Static site - index.html without other indicators
    if repo_path.join("index.html").exists() {
        result.stack = Some(Stack::Static);
        result.confidence = DetectionConfidence::Medium;
        result.detected_files.push("index.html".to_string());
        return result;
    }

    result
}

/// Detects Python package manager from files.
fn detect_python_package_manager(repo_path: &Path) -> Option<PackageManager> {
    // Check in order of preference
    if repo_path.join("pyproject.toml").exists() {
        // Check if it's Poetry or UV
        if let Ok(content) = std::fs::read_to_string(repo_path.join("pyproject.toml")) {
            if content.contains("[tool.poetry]") {
                return Some(PackageManager::Poetry);
            }
            if content.contains("[tool.uv]") || repo_path.join("uv.lock").exists() {
                return Some(PackageManager::Uv);
            }
            // Default pyproject.toml to pip
            return Some(PackageManager::Pip);
        }
    }
    if repo_path.join("Pipfile").exists() {
        return Some(PackageManager::Pipenv);
    }
    if repo_path.join("requirements.txt").exists() {
        return Some(PackageManager::Pip);
    }
    None
}

/// Detects Node.js package manager and framework.
fn detect_nodejs_details(repo_path: &Path, result: &mut DetectionResult) {
    // Detect package manager from lockfiles
    if repo_path.join("pnpm-lock.yaml").exists() {
        result.package_manager = Some(PackageManager::Pnpm);
        result.detected_files.push("pnpm-lock.yaml".to_string());
    } else if repo_path.join("yarn.lock").exists() {
        result.package_manager = Some(PackageManager::Yarn);
        result.detected_files.push("yarn.lock".to_string());
    } else {
        result.package_manager = Some(PackageManager::Npm);
    }

    // Read package.json to detect framework
    if let Ok(content) = std::fs::read_to_string(repo_path.join("package.json")) {
        // Detect Next.js
        if content.contains("\"next\"") {
            result.stack = Some(Stack::NextJs);
            result.confidence = DetectionConfidence::High;
            return;
        }

        // Detect NestJS
        if content.contains("\"@nestjs/core\"") {
            result.stack = Some(Stack::NestJs);
            result.confidence = DetectionConfidence::High;
            return;
        }

        // Detect frameworks for generic Node.js
        if content.contains("\"express\"") {
            result.framework = Some(Framework::Express);
        } else if content.contains("\"fastify\"") {
            result.framework = Some(Framework::Fastify);
        } else if content.contains("\"hono\"") {
            result.framework = Some(Framework::Hono);
        }

        // Detect Node version from engines
        if let Some(version) = extract_node_version(&content) {
            result.version = Some(version);
        }
    }

    // Check for .nvmrc
    if let Ok(version) = std::fs::read_to_string(repo_path.join(".nvmrc")) {
        result.version = Some(version.trim().replace('v', ""));
    }

    result.stack = Some(Stack::NodeJs);
    result.confidence = DetectionConfidence::High;
}

/// Extracts Node.js version from package.json engines field.
fn extract_node_version(package_json: &str) -> Option<String> {
    // Simple extraction - look for "node": ">=18" or similar
    if let Some(start) = package_json.find("\"node\"") {
        let after = &package_json[start..];
        if let Some(colon) = after.find(':') {
            let value_start = &after[colon + 1..];
            if let Some(quote_start) = value_start.find('"') {
                let value = &value_start[quote_start + 1..];
                if let Some(quote_end) = value.find('"') {
                    let version = &value[..quote_end];
                    // Extract just the major version number
                    let clean = version
                        .trim_start_matches(['>', '=', '<', '^', '~', 'v', ' '])
                        .split('.')
                        .next()
                        .unwrap_or("");
                    if !clean.is_empty() {
                        return Some(clean.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Detects Python framework from dependencies.
fn detect_python_details(repo_path: &Path, result: &mut DetectionResult) {
    // Check requirements.txt
    if let Ok(content) = std::fs::read_to_string(repo_path.join("requirements.txt")) {
        detect_python_framework(&content, result);
    }

    // Check pyproject.toml
    if let Ok(content) = std::fs::read_to_string(repo_path.join("pyproject.toml")) {
        detect_python_framework(&content, result);
    }

    // Check for .python-version
    if let Ok(version) = std::fs::read_to_string(repo_path.join(".python-version")) {
        result.version = Some(version.trim().to_string());
    }
}

/// Detects Python framework from content.
fn detect_python_framework(content: &str, result: &mut DetectionResult) {
    let content_lower = content.to_lowercase();

    if content_lower.contains("django") {
        result.framework = Some(Framework::Django);
    } else if content_lower.contains("fastapi") {
        result.framework = Some(Framework::FastApi);
    } else if content_lower.contains("flask") {
        result.framework = Some(Framework::Flask);
    }
}

/// Detects Rust details.
fn detect_rust_details(repo_path: &Path, result: &mut DetectionResult) {
    if let Ok(content) = std::fs::read_to_string(repo_path.join("Cargo.toml")) {
        // Detect web frameworks
        if content.contains("actix-web") {
            result.framework = Some(Framework::Actix);
        } else if content.contains("axum") {
            result.framework = Some(Framework::Axum);
        } else if content.contains("rocket") {
            result.framework = Some(Framework::Rocket);
        }
    }

    // Check rust-toolchain.toml for version
    if let Ok(content) = std::fs::read_to_string(repo_path.join("rust-toolchain.toml")) {
        if let Some(channel) = extract_toml_value(&content, "channel") {
            result.version = Some(channel);
        }
    }
}

/// Detects Go details.
fn detect_go_details(repo_path: &Path, result: &mut DetectionResult) {
    if let Ok(content) = std::fs::read_to_string(repo_path.join("go.mod")) {
        // Extract Go version
        for line in content.lines() {
            if line.starts_with("go ") {
                result.version = Some(line.trim_start_matches("go ").trim().to_string());
                break;
            }
        }

        // Detect frameworks
        if content.contains("github.com/gin-gonic/gin") {
            result.framework = Some(Framework::Gin);
        } else if content.contains("github.com/labstack/echo") {
            result.framework = Some(Framework::Echo);
        } else if content.contains("github.com/gofiber/fiber") {
            result.framework = Some(Framework::Fiber);
        } else if content.contains("github.com/go-chi/chi") {
            result.framework = Some(Framework::Chi);
        }
    }
}

/// Detects Ruby details.
fn detect_ruby_details(repo_path: &Path, result: &mut DetectionResult) {
    // Check for Rails
    if repo_path.join("config").join("application.rb").exists() {
        result.framework = Some(Framework::Rails);
    } else if let Ok(content) = std::fs::read_to_string(repo_path.join("Gemfile")) {
        if content.contains("rails") {
            result.framework = Some(Framework::Rails);
        } else if content.contains("sinatra") {
            result.framework = Some(Framework::Sinatra);
        }
    }

    // Check .ruby-version
    if let Ok(version) = std::fs::read_to_string(repo_path.join(".ruby-version")) {
        result.version = Some(version.trim().to_string());
    }
}

/// Detects Java details.
fn detect_java_details(repo_path: &Path, result: &mut DetectionResult) {
    // Check for Spring Boot
    if let Ok(content) = std::fs::read_to_string(repo_path.join("pom.xml")) {
        if content.contains("spring-boot") {
            result.framework = Some(Framework::SpringBoot);
        }
    }
    if let Ok(content) = std::fs::read_to_string(repo_path.join("build.gradle")) {
        if content.contains("spring-boot") || content.contains("org.springframework.boot") {
            result.framework = Some(Framework::SpringBoot);
        } else if content.contains("quarkus") {
            result.framework = Some(Framework::Quarkus);
        }
    }

    // Check .java-version or .sdkmanrc
    if let Ok(version) = std::fs::read_to_string(repo_path.join(".java-version")) {
        result.version = Some(version.trim().to_string());
    }
}

/// Detects PHP details.
fn detect_php_details(repo_path: &Path, result: &mut DetectionResult) {
    if let Ok(content) = std::fs::read_to_string(repo_path.join("composer.json")) {
        if content.contains("symfony/") {
            result.framework = Some(Framework::Symfony);
        }
    }
}

/// Extracts a value from a TOML-like file.
fn extract_toml_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with(key) && line.contains('=') {
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() == 2 {
                return Some(
                    parts[1]
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string(),
                );
            }
        }
    }
    None
}

/// Validates that a Next.js project has standalone output configured.
/// Returns Ok(()) if valid, or an error message if standalone is not configured.
pub fn validate_nextjs_standalone_config(repo_path: &Path) -> Result<(), String> {
    // List of possible Next.js config file names
    let config_files = [
        "next.config.js",
        "next.config.ts",
        "next.config.mjs",
    ];

    // Find which config file exists
    let config_file = config_files
        .iter()
        .find(|f| repo_path.join(f).exists());

    match config_file {
        Some(filename) => {
            // Read the config file
            let config_path = repo_path.join(filename);
            let content = std::fs::read_to_string(&config_path)
                .map_err(|e| format!("Failed to read {}: {}", filename, e))?;

            // Check if standalone output is configured
            // We look for patterns like:
            // - output: "standalone"
            // - output: 'standalone'
            // - output:"standalone"
            // - output:'standalone'
            let has_standalone = content.contains("output")
                && (content.contains("\"standalone\"") || content.contains("'standalone'"));

            if has_standalone {
                Ok(())
            } else {
                Err(format!(
                    "Next.js standalone output required\n  \
                     Add `output: \"standalone\"` to your {}\n  \
                     Documentation: https://nextjs.org/docs/app/api-reference/config/next-config-js/output",
                    filename
                ))
            }
        }
        None => {
            // No config file found - user needs to create one
            Err(
                "Next.js standalone output required\n  \
                 Create a next.config.js file with `output: \"standalone\"`\n  \
                 Documentation: https://nextjs.org/docs/app/api-reference/config/next-config-js/output"
                    .to_string()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_detect_rust() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[package]
name = "myapp"

[dependencies]
axum = "0.7"
"#,
        )
        .unwrap();

        let result = detect_stack(dir.path());
        assert_eq!(result.stack, Some(Stack::Rust));
        assert_eq!(result.package_manager, Some(PackageManager::Cargo));
        assert_eq!(result.framework, Some(Framework::Axum));
        assert_eq!(result.confidence, DetectionConfidence::High);
    }

    #[test]
    fn test_detect_nextjs() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies": {"next": "14.0.0"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("yarn.lock"), "").unwrap();

        let result = detect_stack(dir.path());
        assert_eq!(result.stack, Some(Stack::NextJs));
        assert_eq!(result.package_manager, Some(PackageManager::Yarn));
    }

    #[test]
    fn test_detect_python_django() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "Django==4.2\n").unwrap();

        let result = detect_stack(dir.path());
        assert_eq!(result.stack, Some(Stack::Python));
        assert_eq!(result.package_manager, Some(PackageManager::Pip));
        assert_eq!(result.framework, Some(Framework::Django));
    }

    #[test]
    fn test_detect_dockerfile() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Dockerfile"), "FROM alpine").unwrap();

        let result = detect_stack(dir.path());
        assert!(result.has_dockerfile);
    }

    #[test]
    fn test_validate_nextjs_standalone_with_standalone() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("next.config.js"),
            r#"
module.exports = {
  output: "standalone",
  reactStrictMode: true,
}
"#,
        )
        .unwrap();

        let result = validate_nextjs_standalone_config(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_nextjs_standalone_with_single_quotes() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("next.config.ts"),
            r#"
const config = {
  output: 'standalone',
}
export default config
"#,
        )
        .unwrap();

        let result = validate_nextjs_standalone_config(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_nextjs_standalone_missing() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("next.config.js"),
            r#"
module.exports = {
  reactStrictMode: true,
}
"#,
        )
        .unwrap();

        let result = validate_nextjs_standalone_config(dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("standalone output required"));
    }

    #[test]
    fn test_validate_nextjs_standalone_no_config_file() {
        let dir = tempdir().unwrap();

        let result = validate_nextjs_standalone_config(dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Create a next.config.js"));
    }

    #[test]
    fn test_validate_nextjs_standalone_mjs() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("next.config.mjs"),
            r#"
export default {
  output: "standalone",
}
"#,
        )
        .unwrap();

        let result = validate_nextjs_standalone_config(dir.path());
        assert!(result.is_ok());
    }
}
