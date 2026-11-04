use agent_rush::{
    api::{AppState, router},
    config::Settings,
    deploy::VultrClient,
    deploy::lifecycle::{
        DeploymentManifest, ManifestFile, ResourceApi, ResourceKind, ResourceRole, teardown,
    },
    domain::Limits,
    ledger::Ledger,
    node::{NodeState, node_router},
    service,
    worker::{DockerRunner, IMAGE},
};
use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use std::{
    path::PathBuf,
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
    },
    Doctor,
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
            let state = AppState::new(settings, ledger)?;
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
        Commands::Worker { bind } => {
            let image = if settings.sandbox_image.is_empty() {
                IMAGE
            } else {
                &settings.sandbox_image
            };
            let runner = DockerRunner::new(image)?;
            let node = NodeState::new(
                runner,
                settings.data_dir.join("node"),
                settings.worker_token.clone(),
                settings.capacity_mb,
            )?;
            // Bind before recovery so two workers cannot race to destroy each other's tasks.
            let listener = tokio::net::TcpListener::bind(&bind).await?;
            node.recover().await?;
            node.runner.health().await?;
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
