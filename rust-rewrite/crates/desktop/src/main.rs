use anyhow::Result;
use organizer_core::{organize_path, OrganizerConfig, OrganizerStats};
use std::env;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let base_path = args
        .next()
        .map(|path| PathBuf::from(shellexpand::tilde(&path).into_owned()))
        .unwrap_or_else(|| {
            PathBuf::from(shellexpand::tilde("~/Pictures/VRChat/VRChat").into_owned())
        });

    let dry_run = env::var("VRCHAT_DRY_RUN").is_ok();
    let scan_all_months = env::var("VRCHAT_SCAN_ALL_MONTHS").is_ok();
    let single_folder = env::var("VRCHAT_SINGLE_FOLDER").is_ok();

    let config = OrganizerConfig {
        base_path: base_path.clone(),
        dry_run,
        scan_all_months,
        single_folder,
        template: "{world}".to_string(),
    };

    let mut stats = OrganizerStats::default();
    organize_path(&config, &mut stats)?;

    println!("base path: {}", base_path.display());
    println!("processed: {}", stats.processed);
    println!("organized: {}", stats.organized);
    println!("no metadata: {}", stats.no_metadata);
    println!("errors: {}", stats.errors);
    println!("total: {}", stats.total);
    Ok(())
}
