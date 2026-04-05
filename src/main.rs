use log::{debug, error};

mod api;
mod types;
mod ui;

#[tokio::main]
async fn main() {
    env_logger::init();
    debug!("Starting Server...");

    let notification_ui = ui::NotificationUI::new();

    api::set_action_tx(notification_ui.action_tx);
    api::set_action_close_tx(notification_ui.close_tx.clone());

    let daemon = api::NotificationDaemon::with_tx(notification_ui.tx, notification_ui.close_tx);
    if let Err(e) = daemon.run().await {
        error!("Error running daemon: {e}");
        std::process::exit(1);
    }
}
