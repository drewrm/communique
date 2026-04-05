use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct Notification {
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub urgency: NotificationUrgency,
    pub timestamp: SystemTime,
    pub icon: Option<String>,
    pub expire_timeout: u64,
    pub id: Option<u32>,
    pub actions: Vec<(String, String)>,
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
