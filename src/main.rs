mod app;

use app::JvApp;

fn main() -> eframe::Result {
    tracing_subscriber::fmt::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title(if cfg!(debug_assertions) {
                "jv - JSON Viewer [DEBUG]"
            } else {
                "jv - JSON Viewer"
            }),
        ..Default::default()
    };

    eframe::run_native(
        "jv",
        options,
        Box::new(|cc| Ok(Box::new(JvApp::new(cc)))),
    )
}
