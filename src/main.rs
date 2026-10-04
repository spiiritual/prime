#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> iced::Result {
    // wgpu otherwise also starts its OpenGL backend, whose driver alone held about 85 MB. Vulkan
    // draws, with DirectX 12 for machines without it. WGPU_BACKEND still overrides this.
    if std::env::var_os("WGPU_BACKEND").is_none() {
        // SAFETY: no other thread has started yet.
        unsafe { std::env::set_var("WGPU_BACKEND", "vulkan,dx12") };
    }
    // iced asks for the high-performance GPU, which on laptops with two wakes the discrete one
    // for a 2D window. WGPU_POWER_PREF and Windows' per-app graphics setting still override this.
    if std::env::var_os("WGPU_POWER_PREF").is_none() {
        // SAFETY: no other thread has started yet.
        unsafe { std::env::set_var("WGPU_POWER_PREF", "low") };
    }

    let mut velopack = velopack::VelopackApp::build();
    velopack.run();

    if !prime::single_instance::claim() {
        return Ok(());
    }
    prime::ui::run()
}
