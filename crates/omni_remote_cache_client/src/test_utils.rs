use std::{
    collections::HashSet,
    env,
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
        let mut port = 3000;
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
        let api_base_url = format!("http://localhost:{}/api", port);

        let child = Command::new(format!(
            "{}/target/release/omni_remote_cache_service",
            ws_dir
        ))
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
