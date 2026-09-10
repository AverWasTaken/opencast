#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#[cfg(windows)]
mod native;
mod ui;
fn main() -> eframe::Result {
    #[cfg(windows)]
    let _instance = {
        let quit = std::env::args().any(|a| a == "--quit");
        let background = std::env::args().any(|a| a == "--background");
        match native::acquire_instance(quit, background) {
            Ok(Some(instance)) => instance,
            Ok(None) => return Ok(()),
            Err(error) => {
                eprintln!("{error}");
                return Ok(());
            }
        }
    };
    eframe::run_native(
        "OpenCast",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_title("OpenCast")
                .with_inner_size([740.0, 476.0])
                .with_min_inner_size([660.0, 438.0])
                .with_icon(
                    eframe::icon_data::from_png_bytes(include_bytes!("../assets/opencast.png"))
                        .expect("valid app icon"),
                )
                .with_visible(!cfg!(windows) || !std::env::args().any(|a| a == "--background"))
                .with_taskbar(false)
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
