use std::{
    collections::HashSet,
    env,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{LazyLock, Mutex},
    thread::sleep,
    time::Duration,
};

// Track used ports globally
static PORTS: LazyLock<Mutex<HashSet<u16>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// RAII helper that reserves and releases a unique port.
pub struct PortGuard {
    pub port: u16,
}

impl PortGuard {
    pub fn new() -> Self {
        let mut ports = PORTS.lock().unwrap();
        let mut port = 3010;
        while ports.contains(&port) {
            port += 1;
        }
        ports.insert(port);
        Self { port }
    }
}

impl Drop for PortGuard {
    fn drop(&mut self) {
        PORTS.lock().unwrap().remove(&self.port);
    }
}

/// RAII helper that spawns the child process and ensures teardown.
pub struct ChildProcessGuard {
    pub child: Child,
    #[allow(unused)]
    pub port_guard: PortGuard,
    pub api_base_url: String,
}

impl ChildProcessGuard {
    pub fn new() -> Self {
        let port_guard = PortGuard::new();
        let port = port_guard.port;

        let ws_dir =
            env::var("WORKSPACE_DIR").expect("WORKSPACE_DIR is not set");
        let target =
            env::var("RUST_TARGET").unwrap_or_else(|_| String::default());
        let api_base_url = format!("http://localhost:{}/api", port);

        let ext = if target.contains("windows") {
            ".exe"
        } else {
            ""
        };

        let mut path = String::new();
        if !target.is_empty() {
            let target_path = format!(
                "{}/target/{}/release/omni_remote_cache_service{}",
                ws_dir, target, ext
            );

            if Path::new(&target_path).exists() {
                path = target_path;
            }
        }
        if path.is_empty() {
            let default_path = format!(
                "{}/target/release/omni_remote_cache_service{}",
                ws_dir, ext
            );
            if Path::new(&default_path).exists() {
                path = default_path;
            }
        }

        if path.is_empty() {
            panic!("Could not find omni_remote_cache_service binary");
        }

        trace::trace!("Starting omni_remote_cache_service at {}", path);

        let child = Command::new(path)
            .args([
                "serve",
                "--listen",
                &format!("0.0.0.0:{}", port),
                "-b",
                "in-memory",
                "--routes.api-prefix",
                "/api",
                "--config",
                "orcs.config.json",
                "--config-type",
                "file",
            ])
            .envs(env::vars())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn child process");

        // Give the server time to start
        sleep(Duration::from_millis(25));

        Self {
            child,
            port_guard,
            api_base_url,
        }
    }
}

impl Drop for ChildProcessGuard {
    fn drop(&mut self) {
        if let Err(e) = self.child.kill() {
            eprintln!("Failed to kill child process: {}", e);
        }
    }
}
