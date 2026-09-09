#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod ui;
fn main() -> eframe::Result {
    eframe::run_native(
        "OpenCast",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size([820.0, 526.0])
                .with_min_inner_size([660.0, 438.0])
                .with_icon(
                    eframe::icon_data::from_png_bytes(include_bytes!("../assets/opencast.png"))
                        .expect("valid app icon"),
                )
                .with_decorations(false)
                .with_transparent(true),
            #[cfg(windows)]
            renderer: eframe::Renderer::Wgpu,
            centered: true,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(ui::OpenCast::new(cc)))),
    )
}
