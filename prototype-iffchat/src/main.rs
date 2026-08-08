// THROWAWAY visual prototype — no interaction logic beyond page switching.
slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = Iffchat::new()?;
    ui.on_goto_chat({ let ui = ui.as_weak(); move || ui.unwrap().set_page(0) });
    ui.on_goto_friends({ let ui = ui.as_weak(); move || ui.unwrap().set_page(1) });
    ui.on_goto_new_friends({ let ui = ui.as_weak(); move || ui.unwrap().set_page(2) });
    ui.run()
}
