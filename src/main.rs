slint::include_modules!();

mod app;

use app::AppState;

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    let state = std::cell::RefCell::new(AppState::new());

    ui.on_do_login({
        let ui = ui.as_weak();
        move || {
            state.borrow_mut().begin_login();
            let label = state.borrow().login_status.label();
            if let Some(ui) = ui.upgrade() {
                ui.set_login_status(label.into());
            }
        }
    });

    ui.run()
}
