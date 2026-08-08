// App-state reducer — Seam 2. Pure state transitions, no I/O.
// Ticket 1 only needs the login-status slice; friend/chat states come in later tickets.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginStatus {
    Idle,
    Connecting,
}

impl LoginStatus {
    pub fn label(self) -> &'static str {
        match self {
            LoginStatus::Idle => "",
            LoginStatus::Connecting => "connecting…",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AppState {
    pub login_status: LoginStatus,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            login_status: LoginStatus::Idle,
        }
    }

    pub fn begin_login(&mut self) {
        self.login_status = LoginStatus::Connecting;
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
}
