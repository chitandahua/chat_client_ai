//! Renders reducer state into the Slint UI properties. Called on the UI thread.
use crate::app::AppState;
use crate::{ApplyRow, ConversationRow, MainWindow, MessageRow};
use slint::{ModelRc, SharedString, VecModel};

/// Build a Slint struct model of conversation rows.
fn conv_rows(s: &AppState) -> ModelRc<ConversationRow> {
    let rows: Vec<ConversationRow> = s
        .conversations
        .iter()
        .map(|c| {
            let last = c.messages.last().map(|m| m.text.clone()).unwrap_or_default();
            let preview = if c.messages.last().map_or(false, |m| m.mine) {
                format!("我: {last}")
            } else {
                last
            };
            ConversationRow {
                name: c.friend.clone().into(),
                preview: preview.into(),
                unread: i32::from(s.is_unread(&c.friend)),
            }
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

/// Build a Slint struct model of friend-apply rows.
fn apply_rows(s: &AppState) -> ModelRc<ApplyRow> {
    let rows: Vec<ApplyRow> = s
        .applies
        .iter()
        .map(|a| ApplyRow { name: a.name.clone().into(), status: a.status as i32 })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

/// Build a Slint struct model of chat message rows.
fn chat_rows(s: &AppState, selected: &str) -> ModelRc<MessageRow> {
    let rows: Vec<MessageRow> = s
        .conversation(selected)
        .map(|c| {
            c.messages
                .iter()
                .map(|m| MessageRow { mine: m.mine, text: m.text.clone().into() })
                .collect()
        })
        .unwrap_or_default();
    ModelRc::new(VecModel::from(rows))
}

/// Push reducer state into the Slint properties. Call on the UI thread only.
pub fn push_ui_from_state(ui: &MainWindow, s: &AppState) {
    let selected = ui.get_selected_friend().to_string();

    let unread_flags: Vec<i32> =
        s.friends.iter().map(|f| i32::from(s.is_unread(&f.name))).collect();

    let friend_names: Vec<SharedString> =
        s.friends.iter().map(|f| f.name.clone().into()).collect();

    ui.set_friend_names(std::rc::Rc::new(VecModel::from(friend_names)).into());
    ui.set_conv_rows(conv_rows(s));
    ui.set_apply_rows(apply_rows(s));
    ui.set_chat_rows(chat_rows(s, &selected));
    ui.set_unread_flags(std::rc::Rc::new(VecModel::from(unread_flags)).into());
}
