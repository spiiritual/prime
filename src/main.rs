#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> iced::Result {
    let mut velopack = velopack::VelopackApp::build();
    velopack.run();

    if !prime::single_instance::claim() {
        return Ok(());
    }
    prime::ui::run()
}
