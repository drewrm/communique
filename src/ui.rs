use crate::api::Notification;
use gtk4::{
    Application, Box, Label, ListBox, ListBoxRow, Orientation, Window, gdk,
    glib::{ControlFlow, source},
    prelude::*,
};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use log::debug;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};

use crate::api::NotificationUrgency;

const ICON_SIZE: i32 = 64;
const APP_ID: &str = "org.drewrm.communique.ui";
const APP_NAMESPACE: &str = "org.drewrm.communique";

pub struct NotificationUI {
    pub tx: Sender<Notification>,
    pub close_tx: Sender<u32>,
    pub action_tx: Sender<(u32, String)>,
}

impl NotificationUI {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let (close_tx, close_rx) = mpsc::channel();
        let (action_tx, action_rx) = mpsc::channel();

        std::thread::spawn(move || {
            run_ui(rx, close_rx, action_rx);
        });

        Self {
            tx,
            close_tx,
            action_tx,
        }
    }
}

fn run_ui(rx: Receiver<Notification>, close_rx: Receiver<u32>, action_rx: Receiver<(u32, String)>) {
    let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
    let close_rx = std::sync::Arc::new(std::sync::Mutex::new(close_rx));
    let action_rx = std::sync::Arc::new(std::sync::Mutex::new(action_rx));
    let id_to_row = std::rc::Rc::new(std::sync::Mutex::new(HashMap::new()));
    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(move |app| {
        let window = create_window();
        window.set_application(Some(app));

        setup_layer_shell(&window);

        let list = ListBox::builder()
            .css_classes(["notification-list"])
            .build();

        window.set_child(Some(&list));
        setup_styles();

        window.present();
        poll_notifications(
            &list,
            rx.clone(),
            close_rx.clone(),
            action_rx.clone(),
            id_to_row.clone(),
        );
    });

    app.run();
}

fn create_window() -> Window {
    Window::new()
}

fn setup_layer_shell(window: &Window) {
    LayerShell::init_layer_shell(window);
    window.set_namespace(Some(APP_NAMESPACE));
    window.set_layer(Layer::Top);
    window.set_exclusive_zone(0);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    window.set_margin(Edge::Top, 12);
    window.set_margin(Edge::Right, 12);
}

fn setup_styles() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(include_str!("../resources/style.css"));
    gtk4::style_context_add_provider_for_display(
        &gdk::Display::default().expect("Failed to get display"),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn poll_notifications(
    list: &ListBox,
    rx: std::sync::Arc<std::sync::Mutex<Receiver<Notification>>>,
    close_rx: std::sync::Arc<std::sync::Mutex<Receiver<u32>>>,
    action_rx: std::sync::Arc<std::sync::Mutex<Receiver<(u32, String)>>>,
    id_to_row: std::rc::Rc<std::sync::Mutex<HashMap<u32, ListBoxRow>>>,
) {
    let list_clone = list.clone();
    let rx_clone = rx.clone();
    let close_rx_clone = close_rx.clone();
    let action_rx_clone = action_rx.clone();
    let id_to_row_clone = id_to_row.clone();
    source::timeout_add_local(std::time::Duration::from_millis(100), move || {
        {
            let rx = rx_clone.lock().unwrap();
            while let Ok(notification) = rx.try_recv() {
                debug!(
                    "Received notification: id={:?}, actions={:?}",
                    notification.id, notification.actions
                );
                let row = create_notification_row(&notification);
                list_clone.prepend(&row);

                // Fade in - delay slightly to allow widget to be created
                let row_anim = row.clone();
                source::timeout_add_local(std::time::Duration::from_millis(1), move || {
                    row_anim.add_css_class("visible");
                    row_anim.queue_draw();
                    ControlFlow::Break
                });

                let row_map = row.clone();
                if let Some(id) = notification.id {
                    let mut map = id_to_row_clone.lock().unwrap();
                    map.insert(id, row_map);
                }
                debug!(
                    "Notification added, list size: {}",
                    list_clone.observe_children().n_items()
                );

                // Show window when adding notification
                if let Some(window) = list_clone.root() {
                    window.show();
                }
            }
        }
        {
            let close_rx = close_rx_clone.lock().unwrap();
            while let Ok(id) = close_rx.try_recv() {
                let mut map = id_to_row_clone.lock().unwrap();
                if let Some(row) = map.remove(&id) {
                    dismiss_row_animated(&row);
                }
            }
        }
        {
            let action_rx = action_rx_clone.lock().unwrap();
            while let Ok((id, action)) = action_rx.try_recv() {
                debug!("Action invoked: {} for notification {}", action, id);
                let mut map = id_to_row_clone.lock().unwrap();
                if let Some(row) = map.remove(&id) {
                    dismiss_row_animated(&row);
                }
            }
        }
        ControlFlow::Continue
    });
}

fn create_notification_row(notification: &Notification) -> ListBoxRow {
    let row = ListBoxRow::builder()
        .css_classes(["notification-row", &urgency_class(notification.urgency)])
        .build();

    if notification.actions.is_empty() {
        setup_row_dismiss_handler(&row);
    }

    if notification.expire_timeout > 0 {
        setup_expiry_timer(&row, notification.expire_timeout);
    }

    let content = create_notification_content(notification);

    let content_and_actions = Box::builder()
        .orientation(Orientation::Vertical)
        .hexpand(true)
        .build();

    content_and_actions.append(&content);

    if !notification.actions.is_empty() {
        let actions_box = create_actions_buttons(&notification.actions, notification.id);
        content_and_actions.append(&actions_box);
    }

    let footer = create_notification_footer(notification);

    let hbox = Box::builder()
        .orientation(Orientation::Horizontal)
        .css_classes(["notification-row-content"])
        .spacing(12)
        .build();

    if let Some(icon) = create_notification_icon(notification) {
        hbox.append(&icon);
    }
    hbox.append(&content_and_actions);

    let main_vbox = Box::builder().orientation(Orientation::Vertical).build();
    main_vbox.append(&hbox);
    main_vbox.append(&footer);

    row.set_child(Some(&main_vbox));
    row
}

fn setup_row_dismiss_handler(row: &ListBoxRow) {
    let row_weak = row.downgrade();
    let controller = gtk4::GestureClick::new();
    controller.connect_pressed(move |_controller, _n_press, _x, _y| {
        if let Some(row) = row_weak.upgrade() {
            dismiss_row_animated(&row);
        }
    });
    row.add_controller(controller);
}

fn dismiss_row(row: &ListBoxRow) {
    let list_opt = row.parent().and_then(|p| p.downcast::<ListBox>().ok());

    if let Some(list) = list_opt {
        let count_before = list.observe_children().n_items();
        debug!("Before remove, list size: {}", count_before);
        list.remove(row);
        let count_after = list.observe_children().n_items();
        debug!("After remove, list size: {}", count_after);

        if count_after == 0 {
            debug!("List is empty, hiding window");
            if let Some(window) = list.root() {
                window.hide();
            }
        }
    } else {
        debug!("Row has no list parent");
    }
}

fn dismiss_row_animated(row: &ListBoxRow) {
    row.add_css_class("closing");

    let row_weak = row.downgrade();
    source::timeout_add_local(std::time::Duration::from_millis(500), move || {
        if let Some(row) = row_weak.upgrade() {
            dismiss_row(&row);
        }
        ControlFlow::Break
    });
}

fn setup_expiry_timer(row: &ListBoxRow, expire_timeout: u64) {
    let row_weak = row.downgrade();

    source::timeout_add_local(
        std::time::Duration::from_millis(expire_timeout),
        move || {
            if let Some(row) = row_weak.upgrade() {
                dismiss_row_animated(&row);
            }
            ControlFlow::Break
        },
    );
}

fn create_notification_content(notification: &Notification) -> Box {
    let summary = Label::builder()
        .label(&notification.summary)
        .css_classes(["summary"])
        .hexpand(true)
        .halign(gtk4::Align::Center)
        .build();
    summary.set_markup(&notification.summary);

    let body = Label::builder()
        .label(&notification.body)
        .css_classes(["body"])
        .halign(gtk4::Align::Start)
        .wrap(true)
        .wrap_mode(gtk4::pango::WrapMode::Word)
        .build();
    body.set_markup(&notification.body);

    let content_box = Box::builder()
        .orientation(Orientation::Vertical)
        .css_classes(["notification-content"])
        .build();
    content_box.append(&summary);
    content_box.append(&body);
    content_box
}

fn create_notification_icon(notification: &Notification) -> Option<gtk4::Image> {
    let icon_path = notification.icon.as_ref()?;

    if icon_path.is_empty() {
        return None;
    }

    let icon = gtk4::Image::from_file(icon_path);
    icon.set_pixel_size(ICON_SIZE);
    Some(icon)
}

fn create_notification_footer(notification: &Notification) -> Box {
    let app_name = Label::builder()
        .label(&notification.app_name)
        .css_classes(["app-name"])
        .halign(gtk4::Align::Start)
        .hexpand(true)
        .build();

    let timestamp = Label::builder()
        .label("now")
        .css_classes(["timestamp"])
        .halign(gtk4::Align::End)
        .build();

    start_timestamp_timer(&timestamp, notification.timestamp);

    let footer_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .css_classes(["notification-footer"])
        .hexpand(true)
        .build();
    footer_box.append(&app_name);
    footer_box.append(&timestamp);
    footer_box
}

fn start_timestamp_timer(label: &Label, timestamp: std::time::SystemTime) {
    let ts = timestamp;
    let label_clone = label.clone();
    let label_weak = label.downgrade();

    source::timeout_add_local(std::time::Duration::from_secs(1), move || {
        if label_weak.upgrade().is_none() {
            return ControlFlow::Break;
        }

        let elapsed = std::time::SystemTime::now()
            .duration_since(ts)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if elapsed >= 60 {
            let datetime: chrono::DateTime<chrono::Local> = ts.into();
            label_clone.set_text(&datetime.format("%H:%M").to_string());
            return ControlFlow::Break;
        }

        label_clone.set_text(&format!("{} seconds ago", elapsed));
        ControlFlow::Continue
    });
}

fn urgency_class(urgency: NotificationUrgency) -> String {
    match urgency {
        NotificationUrgency::Low => "urgency-low".to_string(),
        NotificationUrgency::Normal => "urgency-normal".to_string(),
        NotificationUrgency::Critical => "urgency-critical".to_string(),
    }
}

fn create_actions_buttons(actions: &[(String, String)], notification_id: Option<u32>) -> Box {
    let actions_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .css_classes(["notification-actions"])
        .spacing(12)
        .margin_top(8)
        .halign(gtk4::Align::Center)
        .hexpand(true)
        .build();

    for (id, label) in actions {
        debug!("Creating button: id={}, label={}", id, label);

        let button = gtk4::Button::builder()
            .label(label)
            .css_classes(["notification-action-button"])
            .build();

        button.set_size_request(100, 36);

        let gesture = gtk4::GestureClick::new();
        gesture.set_propagation_phase(gtk4::PropagationPhase::Capture);
        gesture.connect_pressed(move |gesture, _n_press, _x, _y| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
        });
        button.add_controller(gesture);

        let action_id = id.clone();
        let notif_id = notification_id.unwrap_or(0);

        button.connect_clicked(move |_button| {
            debug!("Action {} clicked for notification {}", action_id, notif_id);
            super::api::emit_action_invoked(notif_id, &action_id);
            super::api::dismiss_notification_by_id(notif_id);
        });

        actions_box.append(&button);
    }

    actions_box
}
