use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::OnceLock;
use zbus::{connection::Builder, fdo};
use log::debug;
type Result<T> = std::result::Result<T, fdo::Error>;

static ACTION_TX: OnceLock<mpsc::Sender<(u32, String)>> = OnceLock::new();
static CONNECTION: OnceLock<zbus::Connection> = OnceLock::new();
static ACTION_CLOSE_TX: OnceLock<mpsc::Sender<u32>> = OnceLock::new();

pub fn set_action_tx(tx: mpsc::Sender<(u32, String)>) {
    let _ = ACTION_TX.set(tx);
}

pub fn set_action_close_tx(tx: mpsc::Sender<u32>) {
    let _ = ACTION_CLOSE_TX.set(tx);
}

pub fn set_connection(conn: zbus::Connection) {
    let _ = CONNECTION.set(conn);
}

pub fn dismiss_notification_by_id(id: u32) {
    if let Some(tx) = ACTION_CLOSE_TX.get() {
        let _ = tx.send(id);
    }
}

pub fn emit_action_invoked(id: u32, action: &str) {
    debug!("ActionInvoked signal: {} {}", id, action);
    
    let conn_opt = CONNECTION.get();
    if conn_opt.is_none() {
        debug!("No connection available for signal emission");
        return;
    }
    
    let conn = conn_opt.unwrap().clone();
    let action = action.to_string();
    
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        
        rt.block_on(async {
            let path = zbus::zvariant::ObjectPath::try_from("/org/freedesktop/Notifications").unwrap();
            
            let msg_result = zbus::message::Message::signal(&path, "org.freedesktop.Notifications", "ActionInvoked")
                .unwrap()
                .build(&(id, action.as_str()));
            
            if let Ok(msg) = msg_result {
                debug!("Sending ActionInvoked signal");
                match conn.send(&msg).await {
                    Ok(_) => debug!("ActionInvoked signal sent successfully"),
                    Err(e) => debug!("Failed to send ActionInvoked signal: {}", e),
                }
            }
            
            // Send NotificationClosed signal with reason 3 (ClosedByOwner)
            let close_msg_result = zbus::message::Message::signal(&path, "org.freedesktop.Notifications", "NotificationClosed")
                .unwrap()
                .build(&(id, 3u32));
            
            if let Ok(close_msg) = close_msg_result {
                debug!("Sending NotificationClosed signal");
                match conn.send(&close_msg).await {
                    Ok(_) => debug!("NotificationClosed signal sent successfully"),
                    Err(e) => debug!("Failed to send NotificationClosed signal: {}", e),
                }
            }
        });
    });
}

#[derive(Copy, Clone, Debug)]
pub enum NotificationUrgency {
    Low = 0,
    Normal = 1,
    Critical = 2, 
}

impl From<u32> for NotificationUrgency {
    fn from(value: u32) -> Self {
        match value {
            0 => NotificationUrgency::Low,
            1 => NotificationUrgency::Normal,
            2 => NotificationUrgency::Critical,
            _ => NotificationUrgency::Normal,
        }
    }
}

impl From<NotificationUrgency> for String {
    fn from(urgency: NotificationUrgency) -> Self {
        match urgency {
            NotificationUrgency::Low => "Low".to_string(),
            NotificationUrgency::Normal => "Normal".to_string(),
            NotificationUrgency::Critical => "Critical".to_string(),
        }
    }
}

impl std::fmt::Display for NotificationUrgency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let urgency_str: String = (*self).into();
        write!(f, "{}", urgency_str)
    }
}

#[derive(Debug)]
pub struct NotificationDaemon {
    pub tx: Option<mpsc::Sender<super::ui::Notification>>,
    pub close_tx: Option<mpsc::Sender<u32>>,
    next_id: AtomicU32,
}

impl NotificationDaemon {

    pub fn with_tx(tx: mpsc::Sender<super::ui::Notification>, close_tx: mpsc::Sender<u32>) -> Self {
        Self {
            tx: Some(tx),
            close_tx: Some(close_tx),
            next_id: AtomicU32::new(1),
        }
    }

    pub async fn run(self) -> Result<()> {
        debug!("Creating Session Bus connection");
        let connection = Builder::session()?
            .name("org.freedesktop.Notifications")?
            .serve_at("/org/freedesktop/Notifications", self)?
            .build()
            .await?;

        set_connection(connection);

        debug!("NotificationDaemon is now listening on the session bus");
        std::future::pending::<()>().await;
        Ok(())
    }
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl NotificationDaemon {
    async fn notify(
        &self,
        app_name: String,
        _replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, zbus::zvariant::OwnedValue>,
        expire_timeout: i32,
    ) -> Result<u32> {

        let urgency = if let Some(hint) = hints.get("urgency") {
            if let Ok(v) = <zbus::zvariant::OwnedValue as TryInto<u8>>::try_into(hint.clone()) {
                NotificationUrgency::from(v as u32)
            } else {
                NotificationUrgency::Normal
            }
        } else {
            NotificationUrgency::Normal
        };

        let actions_pairs: Vec<(String, String)> = actions
            .chunks(2)
            .filter_map(|chunk| {
                if chunk.len() == 2 {
                    Some((chunk[0].clone(), chunk[1].clone()))
                } else {
                    None
                }
            })
            .collect();

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        
        debug!("Notification Received: {id} {app_name} {urgency} {summary} {body} {:?}", _app_icon);

        if let Some(tx) = &self.tx {
            let _ = tx.send(super::ui::Notification {
                app_name: app_name.clone(),
                summary: summary.clone(),
                body: body.clone(),
                urgency,
                timestamp: std::time::SystemTime::now(),
                icon: Some(_app_icon),
                expire_timeout: expire_timeout as u64,
                id: Some(id),
                actions: actions_pairs,
            });
        }

        Ok(id)
    }

    async fn close_notification(&self, id: u32) -> Result<()> {
        debug!("Close notification requested: {}", id);
        if let Some(tx) = &self.close_tx {
            let _ = tx.send(id);
        }
        Ok(())
    }

    #[zbus(signal)]
    async fn action_invoked(
        emitter: zbus::object_server::SignalEmitter<'_>,
        id: u32,
        action: &str,
    ) -> zbus::Result<()>;

    async fn get_capabilities(&self) -> Result<Vec<String>> {
        Ok(vec![
            "actions".to_string(),
            "body".to_string(),
            "body-markup".to_string(),
            "body-hyperlinks".to_string(),
        ])
    }

    async fn get_server_information(
        &self,
    ) -> Result<(String, String, String, String)> {
        Ok((
            "Notifications-RS".to_string(),
            "N/A".to_string(),
            "0.1".to_string(),
            "1.3".to_string(),
        ))
    }
}
