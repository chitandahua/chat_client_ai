// THROWAWAY UI prototype glue — cycles the variant switcher and sends mock messages.
slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = Prototype::new()?;

    ui.on_next({
        let ui = ui.as_weak();
        move || {
            let ui = ui.unwrap();
            let v = (ui.get_variant() + 1) % 5;
            ui.set_variant(v);
        }
    });
    ui.on_prev({
        let ui = ui.as_weak();
        move || {
            let ui = ui.unwrap();
            let v = (ui.get_variant() + 4) % 5;
            ui.set_variant(v);
        }
    });
    ui.on_select_friend({
        let ui = ui.as_weak();
        move |name| {
            if let Some(ui) = ui.upgrade() {
                ui.set_selected_friend(name.into());
            }
        }
    });
    ui.on_send_message({
        let ui = ui.as_weak();
        move |text| {
            let ui = ui.unwrap();
            let t = text.trim().to_string();
            if t.is_empty() {
                return;
            }
            ui.set_message_input("".into());
            // mock append
            let _ = t;
        }
    });

    ui.run()
}
