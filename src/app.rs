// App-state reducer — Seam 2. Pure state transitions, no I/O.
// Login lifecycle, friend list, conversations (1:1 text chat) + unread markers.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginStatus {
    Idle,
    Connecting,
    Connected,
    Failed(String),
}

impl LoginStatus {
    pub fn label(&self) -> &'static str {
        match self {
            LoginStatus::Idle => "",
            LoginStatus::Connecting => "connecting…",
            LoginStatus::Connected => "",
            LoginStatus::Failed(_) => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Friend {
    pub id: i64,
    pub name: String,
}

impl Friend {
    /// Create a friend from a login-list entry (no uid known server-side).
    pub fn from_name(name: String) -> Self {
        Friend { id: 0, name }
    }
}

/// A single chat message in a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    /// True when sent by me, false when received.
    pub mine: bool,
    pub text: String,
}

/// The conversation with one friend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub friend: String,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub login_status: LoginStatus,
    /// My own uid from the login response.
    pub my_uid: i64,
    pub friends: Vec<Friend>,
    pub conversations: Vec<Conversation>,
    /// Friend names whose conversation is currently open.
    pub open: Vec<String>,
    /// Friend names with unread messages (not currently open).
    pub unread: Vec<String>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            login_status: LoginStatus::Idle,
            my_uid: 0,
            friends: Vec::new(),
            conversations: Vec::new(),
            open: Vec::new(),
            unread: Vec::new(),
        }
    }

    pub fn begin_login(&mut self) {
        self.login_status = LoginStatus::Connecting;
    }

    pub fn login_succeeded(&mut self, my_uid: i64, friends: Vec<Friend>) {
        self.my_uid = my_uid;
        self.friends = friends;
        self.login_status = LoginStatus::Connected;
    }

    pub fn login_failed(&mut self, message: String) {
        self.login_status = LoginStatus::Failed(message);
    }

    /// Resolve a friend uid to its name, if known.
    pub fn friend_name(&self, uid: i64) -> Option<&str> {
        self.friends.iter().find(|f| f.id == uid).map(|f| f.name.as_str())
    }

    /// Attribute an incoming push to a friend name. The login friend list
    /// carries no uid and the search endpoint is broken server-side, so an
    /// unmapped uid degrades to a readable `uid:N` label rather than dropping.
    pub fn friend_for_uid(&self, uid: i64) -> String {
        self.friend_name(uid).map(str::to_string).unwrap_or_else(|| format!("uid:{uid}"))
    }

    /// Open the conversation with `friend`; clears any unread marker.
    pub fn open_conversation(&mut self, friend: &str) {
        if !self.conversations.iter().any(|c| c.friend == friend) {
            self.conversations.push(Conversation {
                friend: friend.to_string(),
                messages: Vec::new(),
            });
        }
        if !self.open.iter().any(|f| f == friend) {
            self.open.push(friend.to_string());
        }
        self.unread.retain(|f| f != friend);
    }

    /// Append a message I sent to the conversation with `friend`.
    pub fn sent_message(&mut self, friend: &str, text: String) {
        self.push_to_conversation(friend, ChatMessage { mine: true, text });
    }

    /// Append a received message. Marks the friend unread unless their
    /// conversation is currently open.
    pub fn received_message(&mut self, friend: &str, text: String) {
        self.push_to_conversation(friend, ChatMessage { mine: false, text });
        if !self.open.iter().any(|f| f == friend) && !self.unread.iter().any(|f| f == friend) {
            self.unread.push(friend.to_string());
        }
    }

    fn push_to_conversation(&mut self, friend: &str, msg: ChatMessage) {
        match self.conversations.iter_mut().find(|c| c.friend == friend) {
            Some(conv) => conv.messages.push(msg),
            None => self.conversations.push(Conversation {
                friend: friend.to_string(),
                messages: vec![msg],
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_is_idle() {
        let state = AppState::new();
        assert_eq!(state.login_status, LoginStatus::Idle);
    }

    #[test]
    fn begin_login_transitions_to_connecting() {
        let mut state = AppState::new();
        state.begin_login();
        assert_eq!(state.login_status, LoginStatus::Connecting);
    }

    #[test]
    fn login_status_label_is_connecting() {
        assert_eq!(LoginStatus::Connecting.label(), "connecting…");
    }

    #[test]
    fn login_success_stores_friends_and_connects() {
        let mut state = AppState::new();
        state.begin_login();
        state.login_succeeded(4, vec![Friend { id: 1, name: "aaa".into() }]);

        assert_eq!(state.login_status, LoginStatus::Connected);
        assert_eq!(state.my_uid, 4);
        assert_eq!(state.friends.len(), 1);
        assert_eq!(state.friends[0].name, "aaa");
    }

    #[test]
    fn login_failure_shows_error_and_stays_on_login() {
        let mut state = AppState::new();
        state.begin_login();
        state.login_failed("密码错误".to_string());

        assert!(matches!(state.login_status, LoginStatus::Failed(_)));
        assert!(state.friends.is_empty());
    }

    #[test]
    fn idle_login_status_has_no_error() {
        let state = AppState::new();
        assert!(matches!(state.login_status, LoginStatus::Idle));
    }

    #[test]
    fn friend_name_resolves_uid() {
        let mut state = AppState::new();
        state.login_succeeded(0, vec![Friend { id: 3, name: "aaa".into() }]);
        assert_eq!(state.friend_name(3), Some("aaa"));
        assert_eq!(state.friend_name(99), None);
    }

    #[test]
    fn friend_for_uid_degrades_to_uid_label_when_unmapped() {
        let mut state = AppState::new();
        state.login_succeeded(0, vec![Friend::from_name("aaa".into())]);
        // login list has no uid -> push from uid 1 maps to a uid:1 label
        assert_eq!(state.friend_for_uid(1), "uid:1");
        // known uid maps to the name
        state.friends[0].id = 1;
        assert_eq!(state.friend_for_uid(1), "aaa");
    }

    #[test]
    fn sent_message_appends_to_open_conversation() {
        let mut state = AppState::new();
        state.open_conversation("aaa");
        state.sent_message("aaa", "你好".into());

        let conv = state.conversations.iter().find(|c| c.friend == "aaa").unwrap();
        assert_eq!(conv.messages.len(), 1);
        assert!(conv.messages[0].mine);
        assert_eq!(conv.messages[0].text, "你好");
    }

    #[test]
    fn received_message_marks_unread_when_not_open() {
        let mut state = AppState::new();
        state.received_message("aaa", "在吗".into());

        assert!(state.unread.contains(&"aaa".to_string()));
        let conv = state.conversations.iter().find(|c| c.friend == "aaa").unwrap();
        assert_eq!(conv.messages.len(), 1);
        assert!(!conv.messages[0].mine);
    }

    #[test]
    fn received_message_does_not_mark_unread_when_open() {
        let mut state = AppState::new();
        state.open_conversation("aaa");
        state.received_message("aaa", "在吗".into());

        assert!(!state.unread.contains(&"aaa".to_string()));
    }

    #[test]
    fn open_conversation_clears_unread_marker() {
        let mut state = AppState::new();
        state.received_message("aaa", "在吗".into());
        assert!(state.unread.contains(&"aaa".to_string()));

        state.open_conversation("aaa");
        assert!(!state.unread.contains(&"aaa".to_string()));
    }
}
