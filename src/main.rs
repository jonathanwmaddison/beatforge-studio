#![allow(dead_code, unused_variables, unused_imports)]

mod app;
mod audio;
mod synth;
mod slicer;
mod recorder;
mod presets;
mod project;
mod automation;
mod eq;
mod analyzer;
mod midi;
mod fx;
mod mic;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([1000.0, 650.0])
            .with_title("BeatForge Studio"),
        ..Default::default()
    };

    eframe::run_native(
        "BeatForge Studio",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(app::dark_theme());
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "mono".into(),
                egui::FontData::from_static(
                    include_bytes!("../fonts/JetBrainsMono-Regular.ttf"),
                ),
            );
            fonts
                .families
                .get_mut(&egui::FontFamily::Monospace)
                .unwrap()
                .insert(0, "mono".into());
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(app::BeatForge::new()))
        }),
    )
}
