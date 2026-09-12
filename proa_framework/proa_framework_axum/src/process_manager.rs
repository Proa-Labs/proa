// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Process management for dev server processes
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;

/// Manages dev server processes for external apps
#[derive(Clone)]
pub struct ProcessManager {
    processes: Arc<Mutex<HashMap<String, ManagedProcess>>>,
}

struct ManagedProcess {
    child: Child,
}

impl ProcessManager {
    /// Create a new process manager
    pub fn new() -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn a dev server process
    ///
    /// # Arguments
    /// * `mount_path` - The mount path for the external app (used as process ID)
    /// * `command` - Shell command to execute (e.g., "cd apps/admin && pnpm dev")
    ///
    /// # Returns
    /// Result indicating success or failure
    pub fn spawn(&self, mount_path: String, command: String) -> anyhow::Result<()> {
        tracing::info!("Spawning dev server for {}: {}", mount_path, command);

        // Use sh -c to execute the command in a shell
        let child = Command::new("sh")
            .arg("-c")
            .arg(&command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn dev server: {}", e))?;

        tracing::info!("Dev server process started with PID: {:?}", child.id());

        let process = ManagedProcess { child };

        let mut processes = self.processes.lock().unwrap();
        processes.insert(mount_path.clone(), process);

        Ok(())
    }

    /// Check if a process is still running
    pub fn is_running(&self, mount_path: &str) -> bool {
        let mut processes = self.processes.lock().unwrap();
        if let Some(process) = processes.get_mut(mount_path) {
            match process.child.try_wait() {
                Ok(Some(_status)) => {
                    tracing::warn!("Dev server for {} has exited", mount_path);
                    false
                }
                Ok(None) => true, // Still running
                Err(e) => {
                    tracing::error!("Error checking process status: {}", e);
                    false
                }
            }
        } else {
            false
        }
    }

    /// Kill a specific dev server process
    pub fn kill(&self, mount_path: &str) -> anyhow::Result<()> {
        let mut processes = self.processes.lock().unwrap();
        if let Some(mut process) = processes.remove(mount_path) {
            tracing::info!("Killing dev server for {}", mount_path);
            process
                .child
                .kill()
                .map_err(|e| anyhow::anyhow!("Failed to kill process: {}", e))?;
            process
                .child
                .wait()
                .map_err(|e| anyhow::anyhow!("Failed to wait for process: {}", e))?;
        }
        Ok(())
    }

    /// Kill all managed processes
    pub fn kill_all(&self) -> anyhow::Result<()> {
        let mut processes = self.processes.lock().unwrap();
        let mount_paths: Vec<String> = processes.keys().cloned().collect();

        for mount_path in mount_paths {
            if let Some(mut process) = processes.remove(&mount_path) {
                tracing::info!("Killing dev server for {}", mount_path);
                if let Err(e) = process.child.kill() {
                    tracing::error!("Failed to kill process for {}: {}", mount_path, e);
                }
                let _ = process.child.wait();
            }
        }

        Ok(())
    }

    /// Get the number of managed processes
    pub fn count(&self) -> usize {
        self.processes.lock().unwrap().len()
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        // Clean up all processes when the manager is dropped
        if let Err(e) = self.kill_all() {
            tracing::error!("Error cleaning up processes: {}", e);
        }
    }
}

/// Wait for a service to be ready by polling a health check URL
///
/// # Arguments
/// * `url` - The health check URL to poll
/// * `timeout` - Maximum duration to wait
/// * `interval` - Time between poll attempts
///
/// # Returns
/// Result indicating whether the service became ready
pub async fn wait_for_ready(
    url: &str,
    timeout: Duration,
    interval: Duration,
) -> anyhow::Result<()> {
    tracing::info!("Waiting for service at {} to be ready...", url);

    let client = reqwest::Client::new();
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            return Err(anyhow::anyhow!(
                "Timeout waiting for service at {} to be ready",
                url
            ));
        }

        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                tracing::info!("Service at {} is ready!", url);
                return Ok(());
            }
            Ok(response) => {
                tracing::debug!("Service at {} returned status: {}", url, response.status());
            }
            Err(e) => {
                tracing::debug!("Health check failed: {}", e);
            }
        }

        sleep(interval).await;
    }
}
