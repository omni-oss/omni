use std::{
    collections::HashSet,
    env,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{LazyLock, Mutex},
    time::Duration,
};

use tokio::time::sleep;

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
    pub async fn new() -> Self {
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
        let default_path = format!(
            "{}/target/release/omni_remote_cache_service{}",
            ws_dir, ext
        );
        let lookup_paths = if !target.is_empty() {
            vec![
                format!(
                    "{}/target/{}/release/omni_remote_cache_service{}",
                    ws_dir, target, ext
                ),
                default_path,
            ]
        } else {
            vec![default_path]
        };

        for target_path in lookup_paths {
            if Path::new(&target_path).exists() {
                path = target_path;
                break;
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

        // check if the server is up
        let client = reqwest::Client::new();
        let mut did_connect = false;
        let mut current_try = 0;
        const MAX_TRIES: u32 = 10;
        // we're not trying to get a valid response, just to make sure the server is up and can respond
        while current_try < MAX_TRIES {
            match client.get(&api_base_url).send().await {
                Ok(_) => {
                    did_connect = true;
                    break;
                }
                Err(e) => {
                    if e.is_connect() {
                        eprintln!("Failed to connect to server: {}", e);
                    } else {
                        did_connect = true;
                        break;
                    }
                }
            }
            current_try += 1;
            sleep(Duration::from_millis(100)).await;
        }

        if !did_connect {
            panic!("Failed to connect to server: {}", api_base_url);
        } else {
            trace::trace!("Connected to server at {}", api_base_url);
        }

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
