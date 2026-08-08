//! Renders reducer state into the Slint UI properties. Called on the UI thread.
use crate::MainWindow;
use crate::app::AppState;

/// Push reducer state into the Slint properties. Call on the UI thread only.
pub fn push_ui_from_state(ui: &MainWindow, s: &AppState) {
    let selected = ui.get_selected_friend().to_string();

    let messages: Vec<slint::SharedString> = s
        .conversation(selected.as_str())
        .map(|c| {
            c.messages
                .iter()
                .map(|m| {
                    let prefix = if m.mine { "我: ".to_string() } else { format!("{selected}: ") };
                    slint::SharedString::from(format!("{prefix}{}", m.text))
                })
                .collect()
        })
        .unwrap_or_default();

    // Recent conversations: friends that have a conversation (chat history).
    let mut conv_names: Vec<slint::SharedString> = Vec::new();
    let mut conv_previews: Vec<slint::SharedString> = Vec::new();
    let mut conv_unread: Vec<i32> = Vec::new();
    for conv in &s.conversations {
        let last = conv.messages.last().map(|m| m.text.clone()).unwrap_or_default();
        let preview = if conv.messages.last().map_or(false, |m| m.mine) {
            format!("我: {last}")
        } else {
            last
        };
        conv_names.push(conv.friend.clone().into());
        conv_previews.push(preview.into());
        conv_unread.push(if s.is_unread(&conv.friend) { 1 } else { 0 });
    }

    let unread_flags: Vec<i32> =
        s.friends.iter().map(|f| if s.is_unread(&f.name) { 1 } else { 0 }).collect();

    let apply_names: Vec<slint::SharedString> =
        s.applies.iter().map(|a| slint::SharedString::from(a.name.clone())).collect();
    let apply_statuses: Vec<i32> = s.applies.iter().map(|a| a.status as i32).collect();

    let friend_names: Vec<slint::SharedString> =
        s.friends.iter().map(|f| slint::SharedString::from(f.name.clone())).collect();

    ui.set_friend_names(std::rc::Rc::new(slint::VecModel::from(friend_names)).into());
    ui.set_conv_names(std::rc::Rc::new(slint::VecModel::from(conv_names)).into());
    ui.set_conv_previews(std::rc::Rc::new(slint::VecModel::from(conv_previews)).into());
    ui.set_conv_unread(std::rc::Rc::new(slint::VecModel::from(conv_unread)).into());
    ui.set_chat_messages(std::rc::Rc::new(slint::VecModel::from(messages)).into());
    ui.set_unread_flags(std::rc::Rc::new(slint::VecModel::from(unread_flags)).into());
    ui.set_apply_names(std::rc::Rc::new(slint::VecModel::from(apply_names)).into());
    ui.set_apply_statuses(std::rc::Rc::new(slint::VecModel::from(apply_statuses)).into());
}
