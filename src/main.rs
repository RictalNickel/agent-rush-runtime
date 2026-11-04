use agent_rush::{
    api::{AppState, router},
    config::Settings,
    deploy::VultrClient,
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
    let settings = Settings::from_env()?;
    match cli.command.unwrap_or(Commands::Serve) {
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
            axum::serve(listener, node_router(node))
                .with_graceful_shutdown(async move {
                    shutdown_signal().await;
                    if let Err(e) = shutdown_node.shutdown().await {
                        tracing::error!(error=%e,"worker cleanup failed");
                    }
                })
                .await?;
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
