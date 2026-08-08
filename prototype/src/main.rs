// THROWAWAY UI prototype glue — wires the switcher + mock-state callbacks.
slint::include_modules!();
use slint::Model;

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;

    ui.on_do_login({
        let ui = ui.as_weak();
        move || {
            let ui = ui.unwrap();
            ui.set_login_status("已登录(模拟): ssss @ 127.0.0.1:10086 → chat 18080".into());
            ui.set_variant(1);
        }
    });

    ui.on_next_variant({
        let ui = ui.as_weak();
        move || {
            let ui = ui.unwrap();
            let v = ui.get_variant();
            if v >= 3 {
                ui.set_variant(1);
            } else if v >= 1 {
                ui.set_variant(v + 1);
            } else {
                ui.set_variant(1);
            }
        }
    });

    ui.on_prev_variant({
        let ui = ui.as_weak();
        move || {
            let ui = ui.unwrap();
            let v = ui.get_variant();
            if v <= 1 {
                ui.set_variant(0);
            } else {
                ui.set_variant(v - 1);
            }
        }
    });

    ui.on_toggle_add_friend({
        let ui = ui.as_weak();
        move || {
            let ui = ui.unwrap();
            ui.set_add_friend_open(!ui.get_add_friend_open());
        }
    });

    ui.on_send_message({
        let ui = ui.as_weak();
        move || {
            let ui = ui.unwrap();
            let text = ui.get_input_text();
            if text.trim().is_empty() {
                return;
            }
            let msgs = ui.get_messages();
            let model = slint::VecModel::default();
            for m in msgs.iter() {
                model.push(m.clone());
            }
            model.push(format!("我: {}", text).into());
            ui.set_messages(std::rc::Rc::new(model).into());
            ui.set_input_text("".into());
        }
    });

    ui.on_dismiss_banner({
        let ui = ui.as_weak();
        move || {
            let ui = ui.unwrap();
            ui.set_banner("".into());
        }
    });

    ui.run()
}
