use agent_rush::{
    api::AppState, config::Settings, domain::JobRequest, ledger::Ledger, service::run_once,
};

#[tokio::test]
async fn unconfigured_controller_leaves_jobs_queued_without_reserving_money_or_workers() {
    let mut settings = Settings::local();
    settings.worker_url = Some("http://127.0.0.1:1".into());
    let state = AppState::new(settings, Ledger::memory(1024).unwrap()).unwrap();
    let request = serde_json::from_value::<JobRequest>(
        serde_json::json!({"prompt":"Compute revenue totals"}),
    )
    .unwrap();
    let job = state.ledger.create_job("operator", request, "one").unwrap();
    assert_eq!(run_once(&state).await.unwrap(), 0);
    assert_eq!(state.ledger.reserved_memory().unwrap(), 0);
    assert_eq!(state.ledger.spend_total().unwrap(), 0.0);
    assert_eq!(
        state
            .ledger
            .get_job("operator", &job.id)
            .unwrap()
            .unwrap()
            .state,
        "queued"
    );
}
