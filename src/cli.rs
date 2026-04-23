use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use console::style;

use portal::core::{backup, checksum, diff, loader, safety, skeleton, snapshot};
use portal::storage::{manifest, paths::PortalPaths, plugins_manifest, state};

/// Configuration transport layer for Claude Code.
#[derive(Parser)]
#[command(name = "portal", version, about, long_about = None)]
#[allow(clippy::struct_excessive_bools)]
struct Cli {
    /// Show what would happen without making changes
    #[arg(long, global = true)]
    dry_run: bool,

    /// Skip auto-backup (requires --force)
    #[arg(long, global = true)]
    no_backup: bool,

    /// Skip plugin reinstallation on load
    #[arg(long, global = true)]
    no_plugins: bool,

    /// Override safety checks / skip interactive prompts
    #[arg(long, global = true)]
    force: bool,

    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Quiet mode — suppress non-essential output
    #[arg(short, long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Save current .claude/ as a named profile
    Save {
        /// Profile name (prompted if omitted in interactive mode)
        name: Option<String>,
        /// Profile description
        #[arg(short, long, default_value = "")]
        description: String,
        /// Tags (comma-separated)
        #[arg(short, long, value_delimiter = ',')]
        tags: Vec<String>,
    },
    /// Load a saved profile into .claude/ (atomic swap)
    Load {
        /// Profile name to load
        name: String,
    },
    /// List all saved profiles
    List,
    /// Show detailed info about a profile
    Show {
        /// Profile name
        name: String,
    },
    /// Diff two profiles (B defaults to skeleton)
    Diff {
        /// Left profile name
        a: String,
        /// Right profile name (defaults to skeleton)
        b: Option<String>,
        /// Show content diff for a specific file
        #[arg(long)]
        file: Option<String>,
    },
    /// Delete a profile
    Rm {
        /// Profile name to delete
        name: String,
    },
    /// Reset .claude/ to skeleton defaults
    Reset,
    /// Undo the last load/reset operation
    Undo,
    /// Show current active profile and state
    Status,
    /// Rename a profile
    Rename {
        /// Current name
        old: String,
        /// New name
        new: String,
    },
    /// Verify profile integrity (checksums)
    Verify {
        /// Profile name (defaults to active profile)
        name: Option<String>,
    },
}

/// Run the CLI entry point.
///
/// # Errors
///
/// Returns an error if command execution fails.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = PortalPaths::detect();

    match &cli.command {
        None => cmd_no_subcommand(&paths),
        Some(Commands::Save {
            name,
            description,
            tags,
        }) => cmd_save(&cli, &paths, name.as_deref(), description, tags),
        Some(Commands::Load { name }) => cmd_load(&cli, &paths, name),
        Some(Commands::List) => cmd_list(&cli, &paths),
        Some(Commands::Show { name }) => cmd_show(&cli, &paths, name),
        Some(Commands::Diff { a, b, file }) => {
            cmd_diff(&cli, &paths, a, b.as_deref(), file.as_deref())
        }
        Some(Commands::Rm { name }) => cmd_rm(&cli, &paths, name),
        Some(Commands::Reset) => cmd_reset(&cli, &paths),
        Some(Commands::Undo) => cmd_undo(&cli, &paths),
        Some(Commands::Status) => cmd_status(&cli, &paths),
        Some(Commands::Rename { old, new }) => cmd_rename(&cli, &paths, old, new),
        Some(Commands::Verify { name }) => cmd_verify(&cli, &paths, name.as_deref()),
    }
}

#[allow(clippy::unnecessary_wraps, clippy::needless_return, unused_variables)]
fn cmd_no_subcommand(paths: &PortalPaths) -> Result<()> {
    #[cfg(feature = "tui-ratatui")]
    {
        return portal::tui::run(paths);
    }

    #[cfg(feature = "tui-ftui")]
    {
        return portal::tui::run(paths);
    }

    #[cfg(not(any(feature = "tui-ratatui", feature = "tui-ftui")))]
    {
        println!(
            "{} v{}",
            style("portal").bold().cyan(),
            env!("CARGO_PKG_VERSION")
        );
        println!();
        println!("Configuration transport layer for Claude Code.");
        println!();
        println!("Usage: {} <COMMAND>", style("portal").bold());
        println!();
        println!("Commands:");
        println!(
            "  {}       Save current .claude/ as a profile",
            style("save").green()
        );
        println!(
            "  {}       Load a saved profile",
            style("load").green()
        );
        println!(
            "  {}       List all profiles",
            style("list").green()
        );
        println!(
            "  {}       Show profile details",
            style("show").green()
        );
        println!(
            "  {}       Compare two profiles",
            style("diff").green()
        );
        println!(
            "  {}         Delete a profile",
            style("rm").green()
        );
        println!(
            "  {}      Reset .claude/ to skeleton",
            style("reset").green()
        );
        println!(
            "  {}       Undo last load/reset",
            style("undo").green()
        );
        println!(
            "  {}     Show current state",
            style("status").green()
        );
        println!(
            "  {}     Rename a profile",
            style("rename").green()
        );
        println!(
            "  {}     Verify profile integrity",
            style("verify").green()
        );
        println!();
        println!(
            "Run {} for more info.",
            style("portal <command> --help").bold()
        );
        Ok(())
    }
}

// ── save ─────────────────────────────────────────────────────────────

fn cmd_save(
    cli: &Cli,
    paths: &PortalPaths,
    name: Option<&str>,
    description: &str,
    tags: &[String],
) -> Result<()> {
    let name = if let Some(n) = name {
        n.to_string()
    } else {
        if cli.force || !is_interactive() {
            bail!("Profile name required (use --force or provide NAME).");
        }
        dialoguer::Input::<String>::new()
            .with_prompt("Profile name")
            .interact_text()?
    };

    if !cli.force {
        safety::preflight_save(paths)?;
    }

    paths.ensure_dirs()?;

    // Check if profile already exists.
    if paths.profile_dir(&name).exists() && !cli.force {
        if !is_interactive() {
            bail!("Profile \"{name}\" already exists. Use --force to overwrite.");
        }
        let overwrite = dialoguer::Confirm::new()
            .with_prompt(format!("Profile \"{name}\" already exists. Overwrite?"))
            .default(false)
            .interact()?;
        if !overwrite {
            println!("{}", style("Aborted.").yellow());
            return Ok(());
        }
    }

    if cli.dry_run {
        let files = snapshot::scan_trackable_files(&paths.claude_root())?;
        println!(
            "[dry-run] Would save {} files as profile \"{name}\"",
            files.len()
        );
        return Ok(());
    }

    let spinner = progress_spinner("Saving profile...");
    let result = snapshot::save(paths, &name, description, tags)?;
    finish_spinner(
        &spinner,
        &format!(
            "Saved profile \"{}\" ({} files)",
            style(&name).green().bold(),
            result.files.len()
        ),
    );

    if cli.verbose {
        for (path, entry) in &result.files {
            println!("  {path} ({} bytes)", entry.size);
        }
    }

    Ok(())
}

// ── load ─────────────────────────────────────────────────────────────

fn cmd_load(cli: &Cli, paths: &PortalPaths, name: &str) -> Result<()> {
    if cli.no_backup && !cli.force {
        bail!("--no-backup requires --force.");
    }

    if cli.dry_run {
        let manifest_path = paths.profile_manifest(name);
        if !manifest_path.exists() {
            bail!("Profile \"{name}\" not found.");
        }
        let m = manifest::read(&manifest_path)?;
        println!(
            "[dry-run] Would load profile \"{name}\" ({} files)",
            m.files.len()
        );
        return Ok(());
    }

    let spinner = progress_spinner("Loading profile...");
    let result = loader::load(paths, name, cli.no_plugins, cli.force)?;
    finish_spinner(
        &spinner,
        &format!(
            "Loaded profile \"{}\" ({} files)",
            style(&result.profile).green().bold(),
            result.files_loaded
        ),
    );

    if !result.plugin_results.is_empty() && !cli.quiet {
        println!();
        println!("  {}", style("Plugins:").bold());
        for pr in &result.plugin_results {
            let icon = if pr.success {
                style("  ✓").green()
            } else {
                style("  ✗").red()
            };
            println!("{icon} {}", pr.id);
            if cli.verbose && !pr.message.is_empty() {
                println!("    {}", pr.message.trim());
            }
        }
    }

    if !cli.quiet {
        println!("  Backup: {}", style(result.backup_path.display()).dim());
    }

    Ok(())
}

// ── list ─────────────────────────────────────────────────────────────

fn cmd_list(cli: &Cli, paths: &PortalPaths) -> Result<()> {
    let profiles_root = paths.profiles_root();
    if !profiles_root.exists() {
        if !cli.quiet {
            println!(
                "No profiles yet. Run {} to create one.",
                style("portal save").bold()
            );
        }
        return Ok(());
    }

    let current_state = state::read(&paths.state_file())?;
    let active = current_state.active_profile.as_deref();

    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(&profiles_root)?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();

    entries.sort_by_key(std::fs::DirEntry::file_name);

    if entries.is_empty() {
        if !cli.quiet {
            println!(
                "No profiles yet. Run {} to create one.",
                style("portal save").bold()
            );
        }
        return Ok(());
    }

    // Header
    println!(
        "  {:<20} {:>5}   {:>6}   {:>7}   {:<20} {}",
        style("Profile").bold().underlined(),
        style("Files").bold().underlined(),
        style("Size").bold().underlined(),
        style("Plugins").bold().underlined(),
        style("Tags").bold().underlined(),
        style("Active").bold().underlined(),
    );

    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let manifest_path = paths.profile_manifest(&name);

        let (file_count, total_size, tags) = if manifest_path.exists() {
            match manifest::read(&manifest_path) {
                Ok(m) => {
                    let count = m.files.len();
                    let size: u64 = m.files.values().map(|f| f.size).sum();
                    let tags = m.tags.join(",");
                    (count, size, tags)
                }
                Err(_) => (0, 0, String::new()),
            }
        } else {
            (0, 0, String::new())
        };

        let plugins_path = paths.profile_plugins(&name);
        let plugin_count = if plugins_path.exists() {
            plugins_manifest::read(&plugins_path).map_or(0, |b| b.plugins.len())
        } else {
            0
        };

        let is_active = active.is_some_and(|a| a == name);
        let marker = if is_active { "●" } else { "○" };

        println!(
            "  {:<20} {:>5}   {:>6}   {:>7}   {:<20} {}",
            if is_active {
                style(&name).green().bold().to_string()
            } else {
                name.clone()
            },
            file_count,
            format_size(total_size),
            plugin_count,
            truncate_str(&tags, 20),
            marker,
        );
    }

    Ok(())
}

// ── show ─────────────────────────────────────────────────────────────

fn cmd_show(_cli: &Cli, paths: &PortalPaths, name: &str) -> Result<()> {
    let manifest_path = paths.profile_manifest(name);
    if !manifest_path.exists() {
        bail!("Profile \"{name}\" not found.");
    }

    let m = manifest::read(&manifest_path)?;
    let total_size: u64 = m.files.values().map(|f| f.size).sum();

    println!("{}", style(format!("Profile: {}", m.name)).bold());
    println!(
        "  Created:     {}",
        m.created_at.format("%Y-%m-%d %H:%M UTC")
    );
    if let Some(loaded) = m.last_loaded {
        println!("  Last loaded: {}", loaded.format("%Y-%m-%d %H:%M UTC"));
    }
    println!("  Load count:  {}", m.load_count);
    println!(
        "  Description: {}",
        if m.description.is_empty() {
            "(none)"
        } else {
            &m.description
        }
    );
    println!(
        "  Tags:        {}",
        if m.tags.is_empty() {
            "(none)".to_string()
        } else {
            m.tags.join(", ")
        }
    );
    println!(
        "  Files:       {} ({})",
        m.files.len(),
        format_size(total_size)
    );

    let plugins_path = paths.profile_plugins(name);
    if plugins_path.exists() {
        if let Ok(bp) = plugins_manifest::read(&plugins_path) {
            println!("  Plugins:     {}", bp.plugins.len());
            for p in &bp.plugins {
                let status = if p.enabled {
                    style("enabled").green()
                } else {
                    style("disabled").dim()
                };
                println!("    {} ({status})", p.id);
            }
        }
    }

    println!();
    println!("  {}", style("Files:").underlined());
    let mut sorted_files: Vec<_> = m.files.iter().collect();
    sorted_files.sort_by_key(|(k, _)| (*k).clone());
    for (path, entry) in &sorted_files {
        let source_tag = match entry.source {
            portal::core::profile::FileSource::User => "",
            portal::core::profile::FileSource::Skeleton => " (skeleton)",
        };
        println!(
            "    {path} {:>6}{source_tag}",
            format_size(entry.size)
        );
    }

    Ok(())
}

// ── diff ─────────────────────────────────────────────────────────────

fn cmd_diff(
    _cli: &Cli,
    paths: &PortalPaths,
    a: &str,
    b: Option<&str>,
    file: Option<&str>,
) -> Result<()> {
    let left = diff::DiffSide::Profile(a);
    let right =
        b.map_or(diff::DiffSide::Skeleton, diff::DiffSide::Profile);

    // Content diff for a specific file.
    if let Some(file_path) = file {
        let output = diff::content_diff(paths, &left, &right, file_path)?;
        if output.is_empty() {
            println!("Files are identical.");
        } else {
            print!("{output}");
        }
        return Ok(());
    }

    let result = diff::diff_profiles(paths, &left, &right)?;

    println!(
        "{}",
        style(format!(
            "Diff: {} vs {}",
            result.left_name, result.right_name
        ))
        .bold()
    );
    println!();

    if !result.different_content.is_empty() {
        println!(
            "  {} Modified:",
            style(result.different_content.len()).yellow()
        );
        for fd in &result.different_content {
            println!(
                "    {} ({} -> {})",
                style(&fd.path).yellow(),
                format_size(fd.left_size),
                format_size(fd.right_size)
            );
        }
    }

    if !result.only_left.is_empty() {
        println!(
            "  {} Only in {}:",
            style(result.only_left.len()).red(),
            result.left_name
        );
        for f in &result.only_left {
            println!("    {}", style(f).red());
        }
    }

    if !result.only_right.is_empty() {
        println!(
            "  {} Only in {}:",
            style(result.only_right.len()).green(),
            result.right_name
        );
        for f in &result.only_right {
            println!("    {}", style(f).green());
        }
    }

    if !result.shared_same.is_empty() {
        println!(
            "  {} Identical files",
            style(result.shared_same.len()).dim()
        );
    }

    Ok(())
}

// ── rm ───────────────────────────────────────────────────────────────

fn cmd_rm(cli: &Cli, paths: &PortalPaths, name: &str) -> Result<()> {
    let profile_dir = paths.profile_dir(name);
    if !profile_dir.exists() {
        bail!("Profile \"{name}\" not found.");
    }

    if !cli.force {
        if !is_interactive() {
            bail!("Refusing to delete without confirmation. Use --force.");
        }
        let confirm = dialoguer::Confirm::new()
            .with_prompt(format!(
                "Delete profile \"{name}\"? This cannot be undone"
            ))
            .default(false)
            .interact()?;
        if !confirm {
            println!("{}", style("Aborted.").yellow());
            return Ok(());
        }
    }

    if cli.dry_run {
        println!("[dry-run] Would delete profile \"{name}\"");
        return Ok(());
    }

    std::fs::remove_dir_all(&profile_dir)?;

    // If this was the active profile, clear it from state.
    let state_path = paths.state_file();
    let mut portal_state = state::read(&state_path)?;
    if portal_state.active_profile.as_deref() == Some(name) {
        portal_state.active_profile = None;
        state::write(&state_path, &portal_state)?;
    }

    println!(
        "{} Deleted profile \"{name}\"",
        style("✓").green().bold(),
    );

    Ok(())
}

// ── reset ────────────────────────────────────────────────────────────

fn cmd_reset(cli: &Cli, paths: &PortalPaths) -> Result<()> {
    if !cli.force {
        if !is_interactive() {
            bail!("Refusing to reset without confirmation. Use --force.");
        }
        let confirm = dialoguer::Confirm::new()
            .with_prompt(
                "Reset .claude/ to skeleton? Current configuration will be backed up",
            )
            .default(false)
            .interact()?;
        if !confirm {
            println!("{}", style("Aborted.").yellow());
            return Ok(());
        }
    }

    if cli.dry_run {
        println!("[dry-run] Would reset .claude/ to skeleton defaults");
        return Ok(());
    }

    paths.ensure_dirs()?;

    let claude_dir = paths.claude_root();

    // Back up if exists and not --no-backup.
    let backup_path = if claude_dir.exists() && !cli.no_backup {
        Some(backup::create(paths, "reset", "skeleton")?)
    } else {
        None
    };

    // Remove existing.
    if claude_dir.exists() {
        std::fs::remove_dir_all(&claude_dir)?;
    }

    // Create skeleton.
    skeleton::create(&claude_dir)?;

    // Update state.
    let state_path = paths.state_file();
    let mut portal_state = state::read(&state_path)?;
    portal_state.active_profile = None;
    if let Some(ref bp) = backup_path {
        portal_state.last_operation = Some(portal::core::profile::LastOperation {
            op_type: portal::core::profile::OperationType::Reset,
            profile: "skeleton".to_string(),
            timestamp: chrono::Utc::now(),
            backup_path: bp.to_string_lossy().to_string(),
            plugins_installed: false,
        });
    }
    state::write(&state_path, &portal_state)?;

    println!(
        "{} Reset .claude/ to skeleton",
        style("✓").green().bold(),
    );
    if let Some(bp) = backup_path {
        println!("  Backup: {}", style(bp.display()).dim());
    }

    Ok(())
}

// ── undo ─────────────────────────────────────────────────────────────

fn cmd_undo(cli: &Cli, paths: &PortalPaths) -> Result<()> {
    let state_path = paths.state_file();
    let portal_state = state::read(&state_path)?;

    let Some(last_op) = portal_state.last_operation.as_ref() else {
        bail!("Nothing to undo. No previous operation recorded.");
    };

    let backup_path = std::path::PathBuf::from(&last_op.backup_path);
    if !backup_path.exists() {
        bail!(
            "Backup for last operation not found: {}",
            backup_path.display()
        );
    }

    if !cli.force {
        if !is_interactive() {
            bail!("Refusing to undo without confirmation. Use --force.");
        }
        let confirm = dialoguer::Confirm::new()
            .with_prompt(format!(
                "Undo last {:?} of \"{}\"? This will restore from backup",
                last_op.op_type, last_op.profile
            ))
            .default(false)
            .interact()?;
        if !confirm {
            println!("{}", style("Aborted.").yellow());
            return Ok(());
        }
    }

    if cli.dry_run {
        println!(
            "[dry-run] Would restore from backup: {}",
            backup_path.display()
        );
        return Ok(());
    }

    let spinner = progress_spinner("Restoring from backup...");
    backup::restore(paths, &backup_path)?;

    // Update state to record undo.
    let mut new_state = state::read(&state_path)?;
    new_state.last_operation = Some(portal::core::profile::LastOperation {
        op_type: portal::core::profile::OperationType::Undo,
        profile: last_op.profile.clone(),
        timestamp: chrono::Utc::now(),
        backup_path: backup_path.to_string_lossy().to_string(),
        plugins_installed: false,
    });
    state::write(&state_path, &new_state)?;

    finish_spinner(&spinner, "Restored from backup");
    Ok(())
}

// ── status ───────────────────────────────────────────────────────────

fn cmd_status(_cli: &Cli, paths: &PortalPaths) -> Result<()> {
    let state_path = paths.state_file();
    let portal_state = state::read(&state_path)?;

    println!("{}", style("Portal Status").bold());

    match &portal_state.active_profile {
        Some(name) => {
            println!("  Active profile: {}", style(name).green().bold());
            let manifest_path = paths.profile_manifest(name);
            if manifest_path.exists() {
                if let Ok(m) = manifest::read(&manifest_path) {
                    let total_size: u64 = m.files.values().map(|f| f.size).sum();
                    println!(
                        "  Files:          {} ({})",
                        m.files.len(),
                        format_size(total_size)
                    );
                    if let Some(loaded) = m.last_loaded {
                        println!(
                            "  Last loaded:    {}",
                            loaded.format("%Y-%m-%d %H:%M UTC")
                        );
                    }
                }
            }
        }
        None => {
            println!("  Active profile: {}", style("(none)").dim());
        }
    }

    if let Some(ref op) = portal_state.last_operation {
        println!();
        println!(
            "  Last operation: {:?} \"{}\"",
            op.op_type, op.profile
        );
        println!(
            "  Timestamp:      {}",
            op.timestamp.format("%Y-%m-%d %H:%M UTC")
        );
    }

    let claude_dir = paths.claude_root();
    let skeleton_status = if claude_dir.exists() {
        match skeleton::verify(&claude_dir) {
            Ok(issues) if issues.is_empty() => style("valid").green().to_string(),
            Ok(issues) => {
                format!("{} issue(s)", style(issues.len()).yellow())
            }
            Err(_) => style("error").red().to_string(),
        }
    } else {
        style("missing").red().to_string()
    };
    println!();
    println!("  .claude/ skeleton: {skeleton_status}");

    let backups = backup::list(paths)?;
    println!("  Backups:          {}", backups.len());

    Ok(())
}

// ── rename ───────────────────────────────────────────────────────────

fn cmd_rename(_cli: &Cli, paths: &PortalPaths, old: &str, new: &str) -> Result<()> {
    let old_dir = paths.profile_dir(old);
    if !old_dir.exists() {
        bail!("Profile \"{old}\" not found.");
    }
    let new_dir = paths.profile_dir(new);
    if new_dir.exists() {
        bail!("Profile \"{new}\" already exists.");
    }

    std::fs::rename(&old_dir, &new_dir)?;

    // Update manifest name field.
    let manifest_path = paths.profile_manifest(new);
    if manifest_path.exists() {
        let mut m = manifest::read(&manifest_path)?;
        m.name = new.to_string();
        manifest::write(&manifest_path, &m)?;
    }

    // Update active profile in state if needed.
    let state_path = paths.state_file();
    let mut portal_state = state::read(&state_path)?;
    if portal_state.active_profile.as_deref() == Some(old) {
        portal_state.active_profile = Some(new.to_string());
        state::write(&state_path, &portal_state)?;
    }

    println!(
        "{} Renamed \"{old}\" -> \"{new}\"",
        style("✓").green().bold(),
    );

    Ok(())
}

// ── verify ───────────────────────────────────────────────────────────

fn cmd_verify(cli: &Cli, paths: &PortalPaths, name: Option<&str>) -> Result<()> {
    let name = if let Some(n) = name {
        n.to_string()
    } else {
        let portal_state = state::read(&paths.state_file())?;
        match portal_state.active_profile {
            Some(n) => n,
            None => bail!("No profile specified and no active profile."),
        }
    };

    let manifest_path = paths.profile_manifest(&name);
    if !manifest_path.exists() {
        bail!("Profile \"{name}\" not found.");
    }

    let m = manifest::read(&manifest_path)?;
    let files_dir = paths.profile_files_dir(&name);

    let spinner = progress_spinner("Verifying checksums...");
    let mismatches = checksum::verify_manifest(&files_dir, &m.files)?;
    drop(spinner);

    if mismatches.is_empty() {
        println!(
            "{} Profile \"{name}\" — all {} files verified",
            style("✓").green().bold(),
            m.files.len()
        );
    } else {
        println!(
            "{} Profile \"{name}\" — {} checksum mismatch(es):",
            style("✗").red().bold(),
            mismatches.len()
        );
        for mm in &mismatches {
            println!(
                "  {} (expected {}, got {})",
                mm.path, mm.expected, mm.actual
            );
        }
        if !cli.quiet {
            bail!("Integrity check failed for profile \"{name}\".");
        }
    }

    Ok(())
}

// ── helpers ──────────────────────────────────────────────────────────

fn is_interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

#[allow(clippy::cast_precision_loss)]
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

#[allow(clippy::expect_used, clippy::literal_string_with_formatting_args)]
fn progress_spinner(msg: &str) -> indicatif::ProgressBar {
    let pb = indicatif::ProgressBar::new_spinner();
    pb.set_style(
        indicatif::ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .expect("valid template")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}

fn finish_spinner(pb: &indicatif::ProgressBar, msg: &str) {
    pb.finish_with_message(format!("{} {msg}", style("✓").green().bold()));
}
