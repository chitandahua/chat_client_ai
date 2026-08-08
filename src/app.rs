// App-state reducer — Seam 2. Pure state transitions, no I/O.
// Login lifecycle + friend list for tickets 1-2; chat/apply states come in later tickets.

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
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub login_status: LoginStatus,
    pub friends: Vec<Friend>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            login_status: LoginStatus::Idle,
            friends: Vec::new(),
        }
    }

    pub fn begin_login(&mut self) {
        self.login_status = LoginStatus::Connecting;
    }

    pub fn login_succeeded(&mut self, friends: Vec<Friend>) {
        self.friends = friends;
        self.login_status = LoginStatus::Connected;
    }

    pub fn login_failed(&mut self, message: String) {
        self.login_status = LoginStatus::Failed(message);
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
        state.login_succeeded(vec![Friend { name: "aaa".into() }]);

        assert_eq!(state.login_status, LoginStatus::Connected);
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
}
