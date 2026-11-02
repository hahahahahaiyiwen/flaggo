use std::{
    fs::File,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub struct Server {
    child: Child,
    pub base: String,
    log: PathBuf,
}

impl Server {
    pub async fn dotnet(service: &str, database: &Path, log: &Path) -> Self {
        let address = unused_address();
        let repository =
            PathBuf::from(std::env::var_os("FLAGGO_DOTNET_REFERENCE_ROOT").expect(
                "Set FLAGGO_DOTNET_REFERENCE_ROOT to the explicit historical oracle checkout",
            ));
        let project = match service {
            "contract" => "Flaggo.ContractService",
            "decision" => "Flaggo.DecisionService",
            _ => panic!("unknown service"),
        };
        let assembly = repository
            .join("apps")
            .join(format!("{service}-service"))
            .join("src")
            .join(project)
            .join("bin")
            .join("Release")
            .join("net10.0")
            .join(format!("{project}.dll"));
        assert!(
            assembly.is_file(),
            "Build the .NET reference first: {}",
            assembly.display()
        );
        let mut command = Command::new("dotnet");
        clear_telemetry(&mut command);
        command
            .arg(assembly)
            .env("ASPNETCORE_URLS", format!("http://{address}"))
            .env(
                "ConnectionStrings__Flaggo",
                format!("Data Source={}", database.display()),
            )
            .env("ASPNETCORE_ENVIRONMENT", "Production");
        command.env("FLAGGO_CONTRACT_ACTIVATION_POLL_INTERVAL_MS", "300000");
        Self::start(command, address, log).await
    }

    pub(crate) async fn start(
        mut command: Command,
        address: std::net::SocketAddr,
        log: &Path,
    ) -> Self {
        let output = File::create(log).unwrap();
        command
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output));
        let mut server = Self {
            child: command.spawn().expect("start service"),
            base: format!("http://{address}"),
            log: log.to_path_buf(),
        };
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = server.child.try_wait().unwrap() {
                panic!(
                    "Service exited {status}: {}",
                    std::fs::read_to_string(&server.log).unwrap()
                );
            }
            if let Ok(response) = client
                .get(format!("{}/health/ready", server.base))
                .send()
                .await
                && response.status().is_success()
            {
                return server;
            }
            assert!(
                Instant::now() < deadline,
                "Service not ready: {}",
                std::fs::read_to_string(&server.log).unwrap()
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if self
            .child
            .try_wait()
            .expect("inspect owned child")
            .is_none()
        {
            self.child.kill().expect("stop owned child");
        }
        self.child.wait().expect("reap owned child");
    }
}

pub fn unused_address() -> std::net::SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

pub fn assert_legacy_configuration_rejected(
    command: impl Fn() -> Command,
    directory: &Path,
    database_setting: &str,
    listen_setting: &str,
) {
    let database = directory.join("must-not-exist.db");
    for (legacy, replacement) in [
        ("ConnectionStrings__Flaggo", database_setting),
        ("ASPNETCORE_URLS", listen_setting),
    ] {
        let output = command()
            .current_dir(directory)
            .env_remove("ConnectionStrings__Flaggo")
            .env_remove("ASPNETCORE_URLS")
            .env(database_setting, &database)
            .env(listen_setting, "127.0.0.1:0")
            .env_remove(replacement)
            .env(legacy, "obsolete-value-must-not-be-logged")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!database.exists());
        assert!(!directory.join("flaggo.db").exists());
        let log = String::from_utf8_lossy(&output.stdout);
        assert!(log.contains("flaggo.service.failed"), "{log}");
        assert!(log.contains("configuration"), "{log}");
        assert!(log.contains(replacement), "{log}");
        assert!(!log.contains("obsolete-value-must-not-be-logged"));
    }
}

pub fn clear_telemetry(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("OTEL_") {
            command.env_remove(key);
        }
    }
}
