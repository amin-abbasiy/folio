mod app;
mod editor;
mod icons;
mod markdown;
mod selection;
mod storage;
mod theme;

use std::path::PathBuf;

use directories::BaseDirs;
use eframe::egui;

fn main() {
    if let Err(error) = run() {
        eprintln!("Folio: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut data_dir = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--help" | "-h") => {
                println!(
                    "Folio {} — a quiet, local-only notebook\n\nUsage: folio [--data-dir DIRECTORY]\n\n  --data-dir DIRECTORY  Use a dedicated notebook directory\n  --version             Print version\n  --help                Show this help\n\nDefault storage: $XDG_DATA_HOME/folio (usually ~/.local/share/folio)\nNo accounts, cloud services, telemetry, or application network requests.",
                    env!("CARGO_PKG_VERSION")
                );
                return Ok(());
            }
            Some("--version" | "-V") => {
                println!("Folio {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Some("--data-dir") => {
                data_dir = Some(PathBuf::from(
                    args.next().ok_or("--data-dir needs a directory")?,
                ));
            }
            _ => {
                return Err(
                    format!("Unknown argument: {}. Use --help.", arg.to_string_lossy()).into(),
                );
            }
        }
    }

    let directory = data_dir.unwrap_or(
        BaseDirs::new()
            .ok_or("Could not determine your local data directory. Use --data-dir.")?
            .data_local_dir()
            .join("folio"),
    );
    let mut store = storage::Store::open(&directory)?;
    store.seed_welcome()?;
    let backup_warning = store.daily_backup().err().map(|error| error.to_string());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Folio")
            .with_app_id("folio")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([820.0, 540.0])
            .with_icon(icons::app_icon()),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "Folio",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::Folio::new(
                &cc.egui_ctx,
                store,
                backup_warning,
            )))
        }),
    )?;
    Ok(())
}
