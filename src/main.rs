#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> iced::Result {
    // wgpu otherwise also starts its OpenGL backend, whose driver alone held about 85 MB. Vulkan
    // draws, with DirectX 12 for machines without it. WGPU_BACKEND still overrides this.
    if std::env::var_os("WGPU_BACKEND").is_none() {
        // SAFETY: no other thread has started yet.
        unsafe { std::env::set_var("WGPU_BACKEND", "vulkan,dx12") };
    }

    let mut velopack = velopack::VelopackApp::build();
    velopack.run();

    if !prime::single_instance::claim() {
        return Ok(());
    }
    prime::ui::run()
}
