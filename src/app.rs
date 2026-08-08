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
    pub fn new(id: i64, name: String) -> Self {
        Friend { id, name }
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

/// A pending friend-apply received from another user (not yet approved).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendApply {
    pub from_uid: i64,
    pub name: String,
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
    /// Pending friend-applies awaiting my approval.
    pub applies: Vec<FriendApply>,
    /// The last search result (uid, name), if any.
    pub search_result: Option<Friend>,
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
            applies: Vec::new(),
            search_result: None,
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

    /// Record an incoming friend-apply push.
    pub fn apply_received(&mut self, from_uid: i64, name: String) {
        if self.applies.iter().any(|a| a.name == name) {
            return;
        }
        self.applies.push(FriendApply { from_uid, name });
    }

    /// Seed pending applies from the login response, which now carries the
    /// applicant's uid. uid is 0 if the server didn't supply it.
    pub fn seed_applies(&mut self, applies: Vec<FriendApply>) {
        for apply in applies {
            self.apply_received(apply.from_uid, apply.name);
        }
    }

    /// Approve a pending apply: remove it and add the user as a friend.
    /// Returns the name of the approved user.
    pub fn approve_apply(&mut self, from_uid: i64) -> Option<String> {
        let idx = self.applies.iter().position(|a| a.from_uid == from_uid)?;
        let apply = self.applies.remove(idx);
        if !self.friends.iter().any(|f| f.id == from_uid) {
            self.friends.push(Friend { id: from_uid, name: apply.name.clone() });
        }
        Some(apply.name)
    }

    /// Find the uid of a pending apply by its display name.
    pub fn approve_apply_uid(&self, name: &str) -> Option<i64> {
        self.applies.iter().find(|a| a.name == name).map(|a| a.from_uid)
    }

    /// Drop a pending apply by display name without adding a friend.
    pub fn reject_apply(&mut self, name: &str) {
        self.applies.retain(|a| a.name != name);
    }

    /// Store the result of a user search.
    pub fn set_search_result(&mut self, result: Option<Friend>) {
        self.search_result = result;
    }

    pub fn login_failed(&mut self, message: String) {
        self.login_status = LoginStatus::Failed(message);
    }

    /// Resolve a friend uid to its name, if known.
    pub fn friend_name(&self, uid: i64) -> Option<&str> {
        self.friends.iter().find(|f| f.id == uid).map(|f| f.name.as_str())
    }

    /// Attribute an incoming push to a friend name. The login friend list
    /// carries uids, so an unmapped uid means a sender we haven't met yet;
    /// degrade to a readable `uid:N` label rather than dropping.
    pub fn friend_for_uid(&self, uid: i64) -> String {
        self.friend_name(uid).map(str::to_string).unwrap_or_else(|| format!("uid:{uid}"))
    }

    /// Route an incoming text push: learn the sender's uid if there is exactly
    /// one friend with an unknown uid (a compat fallback for servers that omit
    /// uids in the login list), then deliver. Returns the friend name routed to.
    pub fn receive_push(&mut self, from_uid: i64, text: String) -> String {
        // Learn uid -> name when unambiguous: one friend whose uid is unknown.
        if self.friend_name(from_uid).is_none() {
            let unknown: Vec<usize> = self
                .friends
                .iter()
                .enumerate()
                .filter(|(_, f)| f.id == 0)
                .map(|(i, _)| i)
                .collect();
            if unknown.len() == 1 {
                self.friends[unknown[0]].id = from_uid;
            }
        }
        let friend = self.friend_for_uid(from_uid);
        self.received_message(&friend, text);
        friend
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
        state.login_succeeded(0, vec![Friend::new(0, "aaa".into())]);
        // login list has no uid -> push from uid 1 maps to a uid:1 label
        assert_eq!(state.friend_for_uid(1), "uid:1");
        // known uid maps to the name
        state.friends[0].id = 1;
        assert_eq!(state.friend_for_uid(1), "aaa");
    }

    #[test]
    fn receive_push_learns_uid_and_routes_to_friend() {
        let mut state = AppState::new();
        state.login_succeeded(4, vec![Friend::new(0, "aaa".into())]);

        let friend = state.receive_push(1, "你好".into());
        assert_eq!(friend, "aaa"); // learned uid 1 -> aaa
        assert_eq!(state.friends[0].id, 1);

        let conv = state.conversations.iter().find(|c| c.friend == "aaa").unwrap();
        assert_eq!(conv.messages[0].text, "你好");
        assert!(state.unread.contains(&"aaa".to_string()));
    }

    #[test]
    fn receive_push_degrades_to_uid_label_when_ambiguous() {
        let mut state = AppState::new();
        state.login_succeeded(4, vec![Friend::new(0, "aaa".into()), Friend::new(0, "bbb".into())]);
        // two unknown-uid friends -> can't learn, degrade to uid:N
        let friend = state.receive_push(1, "hi".into());
        assert_eq!(friend, "uid:1");
    }

    #[test]
    fn reject_apply_removes_without_adding_friend() {
        let mut state = AppState::new();
        state.apply_received(3, "bbb".into());
        state.reject_apply("bbb");
        assert!(state.applies.is_empty());
        assert!(!state.friends.iter().any(|f| f.id == 3));
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

    #[test]
    fn apply_received_stores_pending_apply_once() {
        let mut state = AppState::new();
        state.apply_received(3, "bbb".into());
        state.apply_received(3, "bbb".into()); // duplicate ignored
        assert_eq!(state.applies.len(), 1);
        assert_eq!(state.applies[0].from_uid, 3);
        assert_eq!(state.applies[0].name, "bbb");
    }

    #[test]
    fn seed_applies_loads_login_apply_list_with_uids() {
        let mut state = AppState::new();
        state.seed_applies(vec![
            FriendApply { from_uid: 1, name: "aaa".into() },
            FriendApply { from_uid: 3, name: "bbb".into() },
        ]);
        assert_eq!(state.applies.len(), 2);
        assert_eq!(state.applies[0].from_uid, 1);
        assert_eq!(state.applies[0].name, "aaa");
        assert_eq!(state.applies[1].from_uid, 3);
    }

    #[test]
    fn login_with_real_uids_sends_and_approves_directly() {
        // Primary path: server supplies uids in the login lists. A friend with a
        // real uid sends immediately, and an apply with a real uid approves
        // immediately — no push-learning needed.
        let mut state = AppState::new();
        state.login_succeeded(4, vec![Friend::new(1, "aaa".into())]);
        state.seed_applies(vec![FriendApply { from_uid: 3, name: "bbb".into() }]);

        // send to the friend whose uid came from login
        assert_eq!(state.friends[0].id, 1);
        state.sent_message("aaa", "hi".into());

        // approve the apply using its server-supplied uid
        let approved = state.approve_apply(3);
        assert_eq!(approved, Some("bbb".to_string()));
        assert!(state.friends.iter().any(|f| f.id == 3 && f.name == "bbb"));
    }

    #[test]
    fn approve_apply_removes_and_adds_friend() {
        let mut state = AppState::new();
        state.apply_received(3, "bbb".into());
        let name = state.approve_apply(3);
        assert_eq!(name, Some("bbb".to_string()));
        assert!(state.applies.is_empty());
        assert!(state.friends.iter().any(|f| f.id == 3 && f.name == "bbb"));
    }

    #[test]
    fn approve_apply_for_unknown_uid_returns_none() {
        let mut state = AppState::new();
        assert_eq!(state.approve_apply(99), None);
    }
}
