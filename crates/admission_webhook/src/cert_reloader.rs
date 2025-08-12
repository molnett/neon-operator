use anyhow::Result;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

pub struct CertificateReloader {
    should_restart: Arc<AtomicBool>,
}

impl CertificateReloader {
    pub fn new() -> Self {
        Self {
            should_restart: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn should_restart(&self) -> bool {
        self.should_restart.load(Ordering::Relaxed)
    }

    pub async fn start_watching(&self, cert_dir: &str) -> Result<()> {
        let should_restart = self.should_restart.clone();
        let cert_dir = cert_dir.to_string();

        tokio::spawn(async move {
            if let Err(e) = watch_certificates(&cert_dir, should_restart).await {
                error!("Certificate watcher failed: {}", e);
            }
        });

        Ok(())
    }
}

async fn watch_certificates(cert_dir: &str, should_restart: Arc<AtomicBool>) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(100);

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Err(e) = tx.blocking_send(res) {
                error!("Failed to send file watcher event: {}", e);
            }
        },
        Config::default(),
    )?;

    watcher.watch(Path::new(cert_dir), RecursiveMode::Recursive)?;
    info!("Started watching certificate directory: {}", cert_dir);

    while let Some(event_result) = rx.recv().await {
        match event_result {
            Ok(event) => {
                // Check if the event involves our certificate files
                let involves_cert_files = event.paths.iter().any(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .map(|name| name == "tls.crt" || name == "tls.key")
                        .unwrap_or(false)
                });

                if involves_cert_files {
                    info!("Certificate files changed - signaling server restart");
                    should_restart.store(true, Ordering::Relaxed);
                }
            }
            Err(e) => {
                warn!("File watcher error: {}", e);
            }
        }
    }

    Ok(())
}
