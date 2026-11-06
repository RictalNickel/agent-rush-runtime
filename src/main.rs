use agent_rush::{
    api::{AppState, router},
    box_client::HttpBoxClient,
    boxes::{BoxConfig, BoxManager},
    config::Settings,
    deploy::VultrClient,
    deploy::lifecycle::{
        DeploymentManifest, ManifestFile, ResourceApi, ResourceKind, ResourceRole, teardown,
    },
    domain::Limits,
    inference::{GatewayModel, InferenceGateway},
    ledger::Ledger,
    mcp_client::McpRegistry,
    node::{NodeState, node_router},
    service,
    session::{SessionLimits, Sessions},
    webhooks,
    worker::{DockerRunner, IMAGE},
};
use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Parser)]
#[command(
    name = "agent-rush",
    version,
    about = "Bounded Rust agents, disposable gVisor execution and verifiable receipts"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}
#[derive(Subcommand)]
enum Commands {
    Serve,
    Worker {
        #[arg(long, default_value = "127.0.0.1:8090")]
        bind: String,
        #[command(flatten)]
        boxes: BoxArgs,
    },
    Doctor,
    /// Run fixed real containment/resource probes. Refuses hosts without required enforcement.
    Qualify {
        #[arg(long)]
        output: PathBuf,
    },
    /// Start a session box, prove its nine containment checks, destroy it, and write the
    /// report that `POST /v1/boxes` requires. Refuses to write a failing report as passing.
    /// The report is written to a temporary file beside the output and renamed over it.
    QualifyBox {
        #[command(flatten)]
        boxes: BoxArgs,
        #[arg(long)]
        output: PathBuf,
        /// A private endpoint (ip:port) the host can reach and a box must not; repeatable.
        #[arg(long)]
        private_probe: Vec<String>,
        /// Worker listener ports, probed on every host address.
        #[arg(long, default_values_t = [8090u16, 8443])]
        listener_port: Vec<u16>,
    },
    Sandbox {
        source: PathBuf,
        #[arg(long, default_value_t = 512)]
        memory_mb: u32,
        #[arg(long, default_value_t = 30)]
        timeout: u32,
        #[arg(long)]
        receipt: Option<PathBuf>,
    },
    Cleanup,
    VultrPlan {
        #[arg(long, default_value = "ord")]
        region: String,
        #[arg(long)]
        credential_file: Option<PathBuf>,
    },
    VultrInferenceKey {
        #[arg(long)]
        credential_file: Option<PathBuf>,
        #[arg(long)]
        subscription_id: Option<String>,
        #[arg(long)]
        output_file: PathBuf,
    },
    /// Discover the exact requested model from Vultr; export no guessed tariff or paid request.
    VultrInferenceModel {
        #[arg(long)]
        output_file: PathBuf,
    },
    /// Initialize a private tracking manifest using a provider-verified estimate; creates no resources.
    VultrManifest {
        #[arg(long)]
        credential_file: Option<PathBuf>,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long, default_value = "ord")]
        region: String,
        #[arg(long)]
        hourly_estimate: f64,
        #[arg(long, default_value_t = 2.0)]
        lifetime_hours: f64,
        #[arg(long, default_value_t = 0.0)]
        spent_before_usd: f64,
    },
    /// Track only an acknowledged newly created resource, never an unrelated existing resource.
    VultrTrack {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        kind: ResourceKind,
        #[arg(long)]
        id: String,
        #[arg(long)]
        role: ResourceRole,
    },
    /// Preview by default. --execute verifies ownership, deletes exact tracked resources and checks absence.
    VultrTeardown {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        execute: bool,
        #[arg(long)]
        credential_file: Option<PathBuf>,
    },
    /// Preview by default; --execute runs independent deadline cleanup on a trusted runner.
    VultrWatch {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        heartbeat: Option<PathBuf>,
        #[arg(long)]
        credential_file: Option<PathBuf>,
        #[arg(long)]
        execute: bool,
        #[arg(long)]
        once: bool,
        #[arg(long,default_value_t=15,value_parser=clap::value_parser!(u64).range(5..=60))]
        poll_seconds: u64,
    },
}

/// Session boxes. A worker without `--box-image` serves one-shot runs only.
#[derive(clap::Args)]
struct BoxArgs {
    /// Digest-pinned workspace image (deploy/workspace.Dockerfile).
    #[arg(long, env = "AR_BOX_IMAGE")]
    box_image: Option<String>,
    #[arg(long, env = "AR_BOX_NETWORK", default_value = agent_rush::boxes::DEFAULT_NETWORK)]
    box_network: String,
    /// Box qualification report written by `qualify-box`.
    #[arg(
        long,
        env = "AR_BOX_QUALIFICATION_REPORT",
        default_value = "/var/lib/agent-rush/box-qualification.json"
    )]
    box_report: PathBuf,
    #[arg(long, env = "AR_BOX_IDLE_SECONDS", default_value_t = agent_rush::boxes::DEFAULT_IDLE_SECONDS)]
    box_idle_seconds: u64,
    #[arg(long, env = "AR_BOX_CEILING_MB", default_value_t = agent_rush::boxes::DEFAULT_CEILING_MB)]
    box_ceiling_mb: u32,
    /// Hard size of each box's /workspace (a sparse ext4 image), in MiB.
    #[arg(long, env = "AR_BOX_DISK_MB", default_value_t = agent_rush::boxes::DEFAULT_DISK_MB)]
    box_disk_mb: u32,
    /// A parked workspace (its box idle past AR_BOX_IDLE_SECONDS) is deleted after this
    /// long without a call.
    #[arg(long, env = "AR_BOX_RETENTION_SECONDS", default_value_t = agent_rush::boxes::DEFAULT_RETENTION_SECONDS)]
    box_retention_seconds: u64,
    /// Marker written by deploy/box-network.sh once the box network filter is in place.
    #[arg(long, env = "AR_BOX_GUARD_MARKER", default_value = agent_rush::boxes::DEFAULT_GUARD_MARKER)]
    box_guard_marker: PathBuf,
}
impl BoxArgs {
    fn config(&self, settings: &Settings) -> Option<BoxConfig> {
        Some(BoxConfig {
            image: self.box_image.clone().filter(|v| !v.is_empty())?,
            network: self.box_network.clone(),
            report: self.box_report.clone(),
            state_dir: settings.data_dir.join("boxes"),
            idle_seconds: self.box_idle_seconds,
            ceiling_mb: self.box_ceiling_mb,
            disk_mb: self.box_disk_mb,
            retention_seconds: self.box_retention_seconds,
            guard_marker: self.box_guard_marker.clone(),
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agent_rush=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Commands::Serve);
    let needs_app_settings = matches!(
        &command,
        Commands::Serve
            | Commands::Worker { .. }
            | Commands::QualifyBox { .. }
            | Commands::Doctor
            | Commands::Sandbox { .. }
            | Commands::Cleanup
    );
    let settings = if needs_app_settings {
        Settings::from_env()?
    } else {
        Settings::local()
    };
    match command {
        Commands::Serve => {
            std::fs::create_dir_all(&settings.data_dir)?;
            let ledger = Ledger::open(&settings.data_dir.join("control.db"), settings.capacity_mb)?;
            let bind = settings.bind.clone();
            let mut state = AppState::new(settings, ledger)?;
            if let Some(path) = &state.settings.mcp_servers_file {
                state.mcp = Arc::new(McpRegistry::from_file(path)?);
            }
            if let Some(worker_url) = state.settings.worker_url.clone() {
                let boxes = HttpBoxClient::new(
                    state.http.clone(),
                    worker_url,
                    state.settings.worker_token.clone(),
                );
                let model = GatewayModel {
                    gateway: InferenceGateway::new(state.settings.clone(), state.ledger.clone())?,
                };
                let sessions = Sessions::new(
                    state.ledger.clone(),
                    Arc::new(model),
                    Arc::new(boxes),
                    state.mcp.clone(),
                    SessionLimits::default(),
                );
                webhooks::spawn(sessions.clone(), state.ledger.clone(), state.webhook_guard);
                state.sessions = Some(sessions);
            } else {
                tracing::warn!("no worker configured; /v1/sessions answers 503");
            }
            let listener = tokio::net::TcpListener::bind(&bind).await?;
            let stop = Arc::new(AtomicBool::new(false));
            let scheduler = tokio::spawn(service::scheduler(state.clone(), stop.clone()));
            tracing::info!(address=%bind,"control server listening");
            axum::serve(listener, router(state))
                .with_graceful_shutdown(async move {
                    shutdown_signal().await;
                    stop.store(true, Ordering::SeqCst);
                })
                .await?;
            scheduler.await??;
        }
        Commands::Worker { bind, boxes } => {
            let image = if settings.sandbox_image.is_empty() {
                IMAGE
            } else {
                &settings.sandbox_image
            };
            let runner = DockerRunner::new(image)?;
            let report_path=settings.qualification_report.clone().context("Run agent-rush qualify and configure AR_QUALIFICATION_REPORT before starting a worker")?;
            let expires_at = qualified_until(&report_path, &runner).await?;
            let node = NodeState::new(
                runner,
                settings.data_dir.join("node"),
                settings.worker_token.clone(),
                settings.capacity_mb,
            )?
            .with_qualification_expiry(expires_at);
            let node = match boxes.config(&settings) {
                Some(config) => node.with_boxes(config)?,
                None => node,
            };
            // Bind before recovery so two workers cannot race to destroy each other's tasks.
            let listener = tokio::net::TcpListener::bind(&bind).await?;
            node.recover().await?;
            node.runner.health().await?;
            if let Some(boxes) = node.boxes() {
                tracing::info!(recovered=%boxes.recover().await?, "session boxes enabled");
                boxes.start_reaper();
            }
            // A requalification timer replaces the report; adopt it in place every minute.
            let refresh_node = node.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    match qualified_until(&report_path, &refresh_node.runner).await {
                        Ok(at) if at > refresh_node.qualification_expiry() => {
                            refresh_node.extend_qualification(at);
                            tracing::info!(expires_at = at, "adopted newer qualification report");
                        }
                        Ok(_) => {}
                        Err(e) => tracing::warn!(error=%e, "qualification report not adopted"),
                    }
                }
            });
            tracing::info!(address=%bind,"private worker listening; use mTLS ingress for remote access");
            let shutdown_node = node.clone();
            axum::serve(listener, node_router(node.clone()))
                .with_graceful_shutdown(async move {
                    shutdown_signal().await;
                    shutdown_node.begin_drain();
                })
                .await?;
            node.shutdown().await?;
        }
        Commands::Doctor => {
            let runner = DockerRunner::new(if settings.sandbox_image.is_empty() {
                IMAGE
            } else {
                &settings.sandbox_image
            })?;
            println!("{}", serde_json::to_string_pretty(&runner.health().await?)?);
        }
        Commands::Sandbox {
            source,
            memory_mb,
            timeout,
            receipt,
        } => {
            ensure!(
                std::fs::metadata(&source)?.len() <= 32768,
                "source exceeds policy"
            );
            let code = std::fs::read_to_string(source)?;
            let runner = DockerRunner::new(if settings.sandbox_image.is_empty() {
                IMAGE
            } else {
                &settings.sandbox_image
            })?;
            let id = uuid::Uuid::new_v4().to_string();
            let cancelled = Arc::new(AtomicBool::new(false));
            let flag = cancelled.clone();
            let signal = tokio::spawn(async move {
                shutdown_signal().await;
                flag.store(true, Ordering::SeqCst);
            });
            let result = runner
                .run(
                    &id,
                    &code,
                    Limits {
                        memory_mb,
                        timeout_seconds: timeout,
                        cpu_millis: 1000,
                        ..Limits::default()
                    },
                    cancelled,
                )
                .await?;
            signal.abort();
            let json = serde_json::to_string_pretty(&result)?;
            if let Some(path) = receipt {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(path, &json)?;
            }
            println!("{json}");
            ensure!(
                result["checks_passed"] == true && result["cleanup_state"] == "verified",
                "sandbox verification failed"
            );
        }
        Commands::Cleanup => {
            let runner = DockerRunner::new(if settings.sandbox_image.is_empty() {
                IMAGE
            } else {
                &settings.sandbox_image
            })?;
            println!("{}", serde_json::json!({"removed":runner.recover().await?}));
        }
        Commands::QualifyBox {
            boxes,
            output,
            private_probe,
            listener_port,
        } => {
            let config = boxes
                .config(&settings)
                .context("set --box-image or AR_BOX_IMAGE to the pinned workspace image")?;
            let capacity = agent_rush::resources::ResourcePolicy::detect(settings.capacity_mb)?
                .usable_memory_mb;
            let image = if settings.sandbox_image.is_empty() {
                IMAGE
            } else {
                &settings.sandbox_image
            };
            let manager = BoxManager::new(DockerRunner::new(image)?, config, capacity)?;
            let probes = agent_rush::qualification::BoxProbes {
                private: private_probe,
                listener_ports: listener_port,
                worker_token: Some(settings.worker_token.clone()),
            };
            let report = agent_rush::qualification::run_box(&manager, &probes).await?;
            agent_rush::qualification::write_report_atomically(
                &output,
                &serde_json::to_vec_pretty(&report)?,
            )
            .context("writing the box qualification report")?;
            println!(
                "{}",
                serde_json::json!({"qualified":report.qualified,"output":output,"checks":report.checks.iter().map(|c|serde_json::json!({"name":c.name,"passed":c.passed})).collect::<Vec<_>>()})
            );
            ensure!(
                report.qualified,
                "Box qualification failed; see report. Boxes stay refused on this worker"
            );
        }
        Commands::Qualify { output } => {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&output)
                .context("Qualification output must be a new private file")?;
            let image = std::env::var("AR_SANDBOX_IMAGE")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| IMAGE.into());
            let runner = DockerRunner::new(&image)?;
            let cancel = Arc::new(AtomicBool::new(false));
            let signal = cancel.clone();
            let watcher = tokio::spawn(async move {
                shutdown_signal().await;
                signal.store(true, Ordering::SeqCst);
            });
            let report = agent_rush::qualification::run(&runner, cancel).await;
            watcher.abort();
            file.write_all(&serde_json::to_vec_pretty(&report)?)?;
            file.sync_all()?;
            println!(
                "{}",
                serde_json::json!({"qualified":report["qualified"],"output":output,"checks":report["checks"].as_array().map_or(0,|v|v.len())})
            );
            ensure!(
                report["qualified"] == true,
                "Worker qualification failed or was unavailable; see report. No weaker runtime is permitted"
            );
        }
        Commands::VultrPlan {
            region,
            credential_file,
        } => {
            let key = match credential_file {
                Some(path) => std::fs::read_to_string(path)?,
                None => std::env::var("VULTR_API_KEY")
                    .context("set VULTR_API_KEY or pass an owner-only --credential-file")?,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &VultrClient::new(key.trim().into())?
                        .inspect(&region)
                        .await?
                )?
            );
        }
        Commands::VultrInferenceKey {
            credential_file,
            subscription_id,
            output_file,
        } => {
            let key = match credential_file {
                Some(path) => std::fs::read_to_string(path)?,
                None => std::env::var("VULTR_API_KEY")
                    .context("set VULTR_API_KEY or pass an owner-only --credential-file")?,
            };
            let summary = VultrClient::new(key.trim().into())?
                .export_existing_inference_key(subscription_id.as_deref(), &output_file)
                .await?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
        Commands::VultrInferenceModel { output_file } => {
            let key = std::env::var("VULTR_INFERENCE_API_KEY")
                .context("Inference credential is not configured")?;
            let summary =
                agent_rush::deploy::credentials::discover_model_configuration(&key, &output_file)
                    .await?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
        Commands::VultrManifest {
            credential_file,
            manifest,
            region,
            hourly_estimate,
            lifetime_hours,
            spent_before_usd,
        } => {
            let key = match credential_file {
                Some(path) => std::fs::read_to_string(path)?,
                None => std::env::var("VULTR_API_KEY")
                    .context("set VULTR_API_KEY or pass an owner-only --credential-file")?,
            };
            let scope = VultrClient::new(key.trim().into())?
                .credential_scope()
                .await?;
            let record = DeploymentManifest::new(
                &region,
                chrono::Utc::now(),
                hourly_estimate,
                lifetime_hours,
                spent_before_usd,
                scope,
            )?;
            let _store = ManifestFile::create(&manifest, &record)?;
            println!("{}", serde_json::to_string_pretty(&record.preview()?)?);
        }
        Commands::VultrTrack {
            manifest,
            kind,
            id,
            role,
        } => {
            let store = ManifestFile::open(&manifest)?;
            let mut record = store.load()?;
            record.track(kind, &id, role)?;
            store.save(&record)?;
            println!("{}", serde_json::to_string_pretty(&record.preview()?)?);
        }
        Commands::VultrWatch {
            manifest,
            heartbeat,
            credential_file,
            execute,
            once,
            poll_seconds,
        } => {
            if !execute {
                let store = ManifestFile::open(&manifest)?;
                let record = store.load()?;
                println!(
                    "{}",
                    serde_json::json!({"phase":"preview","watchdog_armed":false,"expires_at":record.expires_at,"deployment_id":record.deployment_id,"tracked_resources":record.resources.len(),"note":"No watchdog armed and no API operation performed. --execute starts authenticated deadline cleanup."})
                );
            } else {
                let key = match credential_file {
                    Some(path) => std::fs::read_to_string(path)?,
                    None => std::env::var("VULTR_API_KEY")
                        .context("set VULTR_API_KEY or pass an owner-only --credential-file")?,
                };
                let client = VultrClient::new(key.trim().into())?;
                let heartbeat =
                    heartbeat.unwrap_or_else(|| manifest.with_extension("watchdog.json"));
                run_cloud_watchdog(&client, &manifest, &heartbeat, once, poll_seconds).await?;
            }
        }
        Commands::VultrTeardown {
            manifest,
            execute,
            credential_file,
        } => {
            let store = ManifestFile::open(&manifest)?;
            let mut record = store.load()?;
            if execute {
                let key = match credential_file {
                    Some(path) => std::fs::read_to_string(path)?,
                    None => std::env::var("VULTR_API_KEY")
                        .context("set VULTR_API_KEY or pass an owner-only --credential-file")?,
                };
                let client = VultrClient::new(key.trim().into())?;
                record.closed = true;
                store.save(&record)?;
                let result = teardown(&client, &mut record, chrono::Utc::now()).await;
                // Preserve confirmed removals even if a later API operation failed.
                store.save(&record)?;
                let report = result?;
                println!("{}", serde_json::to_string_pretty(&report)?);
                ensure!(
                    report.complete,
                    "Cloud deletion remains pending; retry and reconcile provider billing"
                );
            } else {
                println!("{}", serde_json::to_string_pretty(&record.preview()?)?);
            }
        }
    }
    Ok(())
}
async fn run_cloud_watchdog(
    client: &VultrClient,
    manifest_path: &Path,
    heartbeat_path: &Path,
    once: bool,
    poll_seconds: u64,
) -> Result<()> {
    use agent_rush::deploy::watchdog::{HeartbeatFile, WatchReport, tick_file};
    let initial = {
        let store = ManifestFile::open(manifest_path)?;
        store.load()?
    };
    let identity = initial.identity_sha256()?;
    let heartbeat = HeartbeatFile::open(heartbeat_path, manifest_path, &initial)?;
    let interrupted = Arc::new(AtomicBool::new(false));
    let stop = interrupted.clone();
    let signal = tokio::spawn(async move {
        shutdown_signal().await;
        stop.store(true, Ordering::SeqCst);
    });
    let result:Result<()>=async {
        loop {
            let force_cleanup=interrupted.load(Ordering::SeqCst);
            let at=chrono::Utc::now();
            let report=match tick_file(client,manifest_path,&identity,at,force_cleanup,!once).await {
                Ok(report)=>report,
                Err(error)=>WatchReport::error(&initial,chrono::Utc::now(),!once,&error.to_string())?,
            };
            heartbeat.write(&report)?;
            println!("{}",serde_json::json!({"phase":report.phase,"watchdog_armed":report.ensure_armed(&initial,chrono::Utc::now()).is_ok(),"confirmed_absent":report.confirmed_absent,"pending_count":report.pending.len(),"heartbeat":heartbeat_path}));
            if report.phase=="complete" {return Ok(());}
            if once {ensure!(report.phase!="error","Cloud check failed; private heartbeat records the error");return Ok(());}
            if force_cleanup {anyhow::bail!("Watchdog interrupted; cleanup remains unresolved. Preserve heartbeat/manifest and retry teardown.");}
            if interrupted.load(Ordering::SeqCst) {continue;}
            tokio::time::sleep(std::time::Duration::from_secs(poll_seconds)).await;
        }
    }.await;
    signal.abort();
    result
}
/// Validate the qualification report against this host and image; return when it expires.
async fn qualified_until(report_path: &Path, runner: &DockerRunner) -> Result<i64> {
    let metadata = std::fs::symlink_metadata(report_path)?;
    ensure!(
        metadata.file_type().is_file() && metadata.len() <= 5 * 1024 * 1024,
        "invalid qualification file"
    );
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(report_path)?)?;
    agent_rush::qualification::validate_report(
        &report,
        &runner.health().await?,
        chrono::Utc::now(),
    )?;
    Ok(chrono::DateTime::parse_from_rfc3339(
        report["finished_at"]
            .as_str()
            .context("qualification timestamp missing")?,
    )?
    .timestamp()
        + 7200)
}
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {_ = tokio::signal::ctrl_c()=>{},_ = term.recv()=>{}}
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
