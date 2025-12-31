//! Traefik dynamic configuration templates for applications.
//! Used when deploying apps to generate routing rules.

/// Generates a Traefik dynamic configuration for an app.
pub fn generate_app_config(app_name: &str, domains: &[AppDomain], container_port: u16) -> String {
    let mut routers = String::new();
    let mut services = String::new();
    let mut auth_middlewares = String::new();

    // Generate router for each domain
    for (i, domain) in domains.iter().enumerate() {
        let router_name = if i == 0 {
            app_name.to_string()
        } else {
            format!("{}-{}", app_name, i)
        };

        // Sanitize domain for middleware name (replace dots with dashes)
        let domain_safe = domain.domain.replace('.', "-");
        let auth_middleware_name = format!("{}-auth-{}", app_name, domain_safe);

        // Build middleware list for this domain
        let mut https_middlewares = Vec::new();
        if domain.auth.is_some() {
            https_middlewares.push(auth_middleware_name.clone());
        }

        // Generate auth middleware if needed
        if let Some(auth) = &domain.auth {
            // Use single quotes to avoid YAML escaping issues with $ in bcrypt hashes
            auth_middlewares.push_str(&format!(
                r#"    {middleware_name}:
      basicAuth:
        users:
          - '{htpasswd_line}'
"#,
                middleware_name = auth_middleware_name,
                htpasswd_line = auth.htpasswd_line
            ));
        }

        // HTTP router (for ACME challenge and redirect)
        routers.push_str(&format!(
            r#"    {router_name}-http:
      rule: "Host(`{domain}`)"
      entryPoints:
        - web
      service: {app_name}
      middlewares:
        - {app_name}-redirect-https
"#,
            router_name = router_name,
            domain = domain.domain,
            app_name = app_name
        ));

        // HTTPS router with optional auth middleware
        if https_middlewares.is_empty() {
            routers.push_str(&format!(
                r#"    {router_name}:
      rule: "Host(`{domain}`)"
      entryPoints:
        - websecure
      service: {app_name}
      tls:
        certResolver: letsencrypt
"#,
                router_name = router_name,
                domain = domain.domain,
                app_name = app_name
            ));
        } else {
            let middlewares_list = https_middlewares
                .iter()
                .map(|m| format!("        - {}", m))
                .collect::<Vec<_>>()
                .join("\n");
            routers.push_str(&format!(
                r#"    {router_name}:
      rule: "Host(`{domain}`)"
      entryPoints:
        - websecure
      service: {app_name}
      middlewares:
{middlewares_list}
      tls:
        certResolver: letsencrypt
"#,
                router_name = router_name,
                domain = domain.domain,
                app_name = app_name,
                middlewares_list = middlewares_list
            ));
        }

        // Add www routers if primary domain
        if domain.primary && !domain.domain.starts_with("www.") {
            // HTTP www router
            routers.push_str(&format!(
                r#"    {app_name}-www-http:
      rule: "Host(`www.{domain}`)"
      entryPoints:
        - web
      service: {app_name}
      middlewares:
        - {app_name}-redirect-https
"#,
                app_name = app_name,
                domain = domain.domain
            ));

            // HTTPS www router (inherits auth from primary domain)
            if https_middlewares.is_empty() {
                routers.push_str(&format!(
                    r#"    {app_name}-www:
      rule: "Host(`www.{domain}`)"
      entryPoints:
        - websecure
      service: {app_name}
      tls:
        certResolver: letsencrypt
"#,
                    app_name = app_name,
                    domain = domain.domain
                ));
            } else {
                let middlewares_list = https_middlewares
                    .iter()
                    .map(|m| format!("        - {}", m))
                    .collect::<Vec<_>>()
                    .join("\n");
                routers.push_str(&format!(
                    r#"    {app_name}-www:
      rule: "Host(`www.{domain}`)"
      entryPoints:
        - websecure
      service: {app_name}
      middlewares:
{middlewares_list}
      tls:
        certResolver: letsencrypt
"#,
                    app_name = app_name,
                    domain = domain.domain,
                    middlewares_list = middlewares_list
                ));
            }
        }
    }

    // Generate service
    services.push_str(&format!(
        r#"    {app_name}:
      loadBalancer:
        servers:
          - url: "http://flaase-{app_name}-web:{port}"
"#,
        app_name = app_name,
        port = container_port
    ));

    // Generate middlewares (redirect + auth)
    let middlewares = format!(
        r#"  middlewares:
    {app_name}-redirect-https:
      redirectScheme:
        scheme: https
        permanent: true
{auth_middlewares}"#,
        app_name = app_name,
        auth_middlewares = auth_middlewares
    );

    format!(
        r#"# Traefik dynamic configuration for {app_name}
# Generated by Flaase

http:
  routers:
{routers}
  services:
{services}
{middlewares}"#,
        app_name = app_name,
        routers = routers,
        services = services,
        middlewares = middlewares
    )
}

/// Generates a Traefik maintenance configuration (503 page) for an app.
pub fn generate_maintenance_config(app_name: &str) -> String {
    format!(
        r#"# Traefik maintenance configuration for {app_name}
# Generated by Flaase - App is stopped

http:
  routers:
    {app_name}-maintenance:
      rule: "PathPrefix(`/`)"
      entryPoints:
        - websecure
      service: {app_name}-maintenance
      priority: 1
      tls:
        certResolver: letsencrypt

  services:
    {app_name}-maintenance:
      loadBalancer:
        servers: []

  middlewares:
    {app_name}-maintenance-error:
      errors:
        status:
          - "503"
        service: {app_name}-maintenance
        query: "/"
"#,
        app_name = app_name
    )
}

/// Domain configuration for an app.
#[derive(Debug, Clone)]
pub struct AppDomain {
    pub domain: String,
    pub primary: bool,
    /// Optional authentication (htpasswd format: "username:hash")
    pub auth: Option<DomainAuthConfig>,
}

/// Authentication configuration for a domain.
#[derive(Debug, Clone)]
pub struct DomainAuthConfig {
    /// Htpasswd-compatible credential string: "username:bcrypt_hash"
    pub htpasswd_line: String,
}

impl AppDomain {
    pub fn new(domain: &str, primary: bool) -> Self {
        Self {
            domain: domain.to_string(),
            primary,
            auth: None,
        }
    }

    pub fn with_auth(mut self, htpasswd_line: &str) -> Self {
        self.auth = Some(DomainAuthConfig {
            htpasswd_line: htpasswd_line.to_string(),
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_app_config() {
        let domains = vec![AppDomain::new("example.com", true)];
        let config = generate_app_config("my-app", &domains, 3000);

        // Check HTTPS router
        assert!(config.contains("my-app:"));
        assert!(config.contains("Host(`example.com`)"));
        assert!(config.contains("Host(`www.example.com`)"));
        assert!(config.contains("http://flaase-my-app-web:3000"));

        // Check HTTP router for ACME
        assert!(config.contains("my-app-http:"));
        assert!(config.contains("entryPoints:\n        - web"));

        // Check middleware
        assert!(config.contains("my-app-redirect-https:"));
        assert!(config.contains("redirectScheme:"));
    }

    #[test]
    fn test_generate_app_config_with_auth() {
        let domains = vec![AppDomain::new("example.com", true)
            .with_auth("admin:$2y$10$abcdefghijklmnopqrstuvwxyz")];
        let config = generate_app_config("my-app", &domains, 3000);

        // Check auth middleware is generated
        assert!(config.contains("my-app-auth-example-com:"));
        assert!(config.contains("basicAuth:"));
        assert!(config.contains("users:"));
        // Check single quotes are used (no escaping needed)
        assert!(config.contains("- 'admin:$2y$10$"));

        // Check router uses auth middleware
        assert!(config.contains("- my-app-auth-example-com"));
    }

    #[test]
    fn test_generate_app_config_mixed_auth() {
        let domains = vec![
            AppDomain::new("secure.example.com", false)
                .with_auth("admin:$2y$10$hash"),
            AppDomain::new("public.example.com", true),
        ];
        let config = generate_app_config("my-app", &domains, 3000);

        // Check auth middleware only for secure domain
        assert!(config.contains("my-app-auth-secure-example-com:"));
        assert!(!config.contains("my-app-auth-public-example-com:"));
    }
}
