use neon_cluster::{controllers, util::telemetry};

mod compute;
mod handlers;
mod server;
mod services;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telemetry::init().await;

    // Initialize Kubernetes controller state
    let state = controllers::cluster_controller::State::default();
    let project_state = controllers::project_controller::State::default();
    let branch_state = controllers::branch_controller::State::default();

    // Start controllers
    let neon_cluster_controller = controllers::cluster_controller::run(state.clone());
    let neon_project_controller = controllers::project_controller::run(project_state.clone());
    let neon_branch_controller = controllers::branch_controller::run(branch_state.clone());

    // Start web server
    let web_server = server::start_server(state);

    // Both runtimes implement graceful shutdown, so poll until both are done
    tokio::join!(
        neon_cluster_controller,
        neon_project_controller,
        neon_branch_controller,
        web_server
    )
    .3?;
    Ok(())
}
