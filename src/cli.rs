use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use console::style;
use std::path::PathBuf;

use portal::config;
use portal::core::progress::ProgressReporter;
use portal::core::{
    backup, bind, checksum, clone, diff, doctor, git_history, loader, plugins, remove, safety,
    skeleton, snapshot, transport,
};
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
    /// Swap to the previously active profile (instant toggle)
    Toggle,
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
        /// Attempt to reinstall failed plugins
        #[arg(long)]
        fix_plugins: bool,
    },
    /// Export a profile to a portable .tar.zst archive
    Export {
        /// Profile name to export
        name: String,
        /// Output path (file or directory; defaults to current directory)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Import a profile from a .tar.zst archive
    Import {
        /// Path to the .tar.zst archive
        path: String,
    },
    /// Clone a profile, selectively choosing what to bring
    Clone {
        /// Source profile name
        source: String,
        /// New profile name
        target: String,
        /// Description for the new profile
        #[arg(short, long, default_value = "")]
        description: String,
        /// Only include these categories (comma-separated: claude-md,settings,skills,rules,memory,commands,agents,hooks,plugins)
        #[arg(long)]
        only: Option<String>,
        /// Exclude these categories (comma-separated)
        #[arg(long)]
        without: Option<String>,
        /// Start with an empty CLAUDE.md instead of copying the source's
        #[arg(long)]
        fresh_claude_md: bool,
    },
    /// Recover from a crashed swap (.claude.portal-old)
    Recover,
    /// Diagnose portal health; show what's managed and offer guided repairs
    Doctor {
        /// Apply guided fixes (prompts before each, unless --force)
        #[arg(long)]
        fix: bool,
    },
    /// Show a profile's git history (commits on its history branch)
    History {
        /// Profile name (defaults to the active profile)
        name: Option<String>,
    },
    /// Launch a claude session bound to a profile's isolated config dir (no swap)
    Use {
        /// Profile name (defaults to the active profile)
        name: Option<String>,
        /// Print the `export CLAUDE_CONFIG_DIR=…` line instead of launching
        #[arg(long)]
        print_env: bool,
        /// Bind to the already-materialized dir without refreshing from CAS
        #[arg(long)]
        no_refresh: bool,
        /// Args passed through to `claude`
        #[arg(last = true)]
        args: Vec<String>,
    },
}

/// Search for an existing `.claude` directory: CWD first (interactive only), then `home_default`.
///
/// CWD search is restricted to interactive mode so test sandboxes (which set
/// `HOME` but not `CWD`) don't accidentally pick up the project's own `.claude`.
fn discover_claude_dir(home_default: &std::path::Path) -> Option<PathBuf> {
    if is_interactive() {
        if let Ok(cwd) = std::env::current_dir() {
            let p = cwd.join(".claude");
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    if home_default.is_dir() {
        return Some(home_default.to_path_buf());
    }
    None
}

/// On first run, confirm (or let the user choose) which `.claude` directory
/// portal should manage. Saves the answer to `portal.config.toml` so subsequent
/// runs skip the prompt.
///
/// Search priority: CWD/.claude → $HOME/.claude → ask.
/// Returns a new `PortalPaths` with the confirmed directory applied.
fn ensure_claude_dir_confirmed(paths: &PortalPaths) -> Result<PortalPaths> {
    let config_path = paths.config_file();
    let mut cfg = config::load(&config_path).unwrap_or_default();

    if cfg.claude_dir.is_none() {
        let found = discover_claude_dir(&paths.claude_root());
        let confirmed = if is_interactive() {
            if let Some(ref found_path) = found {
                let ok = dialoguer::Confirm::new()
                    .with_prompt(format!(
                        "Is this the .claude directory you want to manage: {}?",
                        found_path.display()
                    ))
                    .default(true)
                    .interact()?;
                if ok {
                    found_path.clone()
                } else {
                    let raw: String = dialoguer::Input::new()
                        .with_prompt("Path to your .claude directory")
                        .interact_text()?;
                    PathBuf::from(raw)
                }
            } else {
                // Nothing found anywhere — ask directly.
                let raw: String = dialoguer::Input::new()
                    .with_prompt("No .claude directory found. Enter the full path")
                    .interact_text()?;
                PathBuf::from(raw)
            }
        } else {
            // Non-interactive: use discovered path or fall back to $HOME/.claude.
            found.unwrap_or_else(|| paths.claude_root())
        };

        cfg.claude_dir = Some(confirmed);
        paths.ensure_dirs()?;
        config::save(&cfg, &config_path)?;
    }

    if let Some(dir) = cfg.claude_dir {
        Ok(paths.clone().with_claude_override(dir))
    } else {
        Ok(paths.clone())
    }
}

/// Run the CLI entry point.
///
/// # Errors
///
/// Returns an error if command execution fails.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = PortalPaths::detect();
    let paths = ensure_claude_dir_confirmed(&paths)?;

    match &cli.command {
        None => cmd_no_subcommand(&paths),
        Some(Commands::Save {
            name,
            description,
            tags,
        }) => cmd_save(&cli, &paths, name.as_deref(), description, tags),
        Some(Commands::Load { name }) => cmd_load(&cli, &paths, name),
        Some(Commands::Toggle) => cmd_toggle(&cli, &paths),
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
        Some(Commands::Verify { name, fix_plugins }) => {
            cmd_verify(&cli, &paths, name.as_deref(), *fix_plugins)
        }
        Some(Commands::Export { name, output }) => {
            cmd_export(&cli, &paths, name, output.as_deref())
        }
        Some(Commands::Import { path }) => cmd_import(&cli, &paths, path),
        Some(Commands::Clone {
            source,
            target,
            description,
            only,
            without,
            fresh_claude_md,
        }) => cmd_clone(
            &cli,
            &paths,
            source,
            target,
            description,
            only.as_deref(),
            without.as_deref(),
            *fresh_claude_md,
        ),
        Some(Commands::Recover) => cmd_recover(&cli, &paths),
        Some(Commands::Doctor { fix }) => cmd_doctor(&cli, &paths, *fix),
        Some(Commands::History { name }) => cmd_history(&cli, &paths, name.as_deref()),
        Some(Commands::Use {
            name,
            print_env,
            no_refresh,
            args,
        }) => cmd_use(&cli, &paths, name.as_deref(), *print_env, *no_refresh, args),
    }
}

#[allow(clippy::unnecessary_wraps, clippy::needless_return, unused_variables)]
fn cmd_no_subcommand(paths: &PortalPaths) -> Result<()> {
    #[cfg(feature = "tui-ratatui")]
    {
        return portal::tui::run(paths);
    }

    #[cfg(not(feature = "tui-ratatui"))]
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
        println!("  {}       Load a saved profile", style("load").green());
        println!(
            "  {}     Swap to the previous profile",
            style("toggle").green()
        );
        println!("  {}       List all profiles", style("list").green());
        println!("  {}       Show profile details", style("show").green());
        println!("  {}       Compare two profiles", style("diff").green());
        println!("  {}         Delete a profile", style("rm").green());
        println!(
            "  {}      Reset .claude/ to skeleton",
            style("reset").green()
        );
        println!("  {}       Undo last load/reset", style("undo").green());
        println!("  {}     Show current state", style("status").green());
        println!("  {}     Rename a profile", style("rename").green());
        println!("  {}     Verify profile integrity", style("verify").green());
        println!(
            "  {}     Export profile to archive",
            style("export").green()
        );
        println!(
            "  {}     Import profile from archive",
            style("import").green()
        );
        println!(
            "  {}      Clone profile selectively",
            style("clone").green()
        );
        println!(
            "  {}    Recover from crashed swap",
            style("recover").green()
        );
        println!("  {}     Diagnose health & repair", style("doctor").green());
        println!(
            "  {}    Show a profile's git history",
            style("history").green()
        );
        println!(
            "  {}        Launch a session bound to an isolated config dir",
            style("use").green()
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
    // The active profile (if any) — determines whether we're "saving over a
    // game we're already playing" and lets us default to overwriting it.
    let active = state::read(&paths.state_file())
        .ok()
        .and_then(|s| s.active_profile);

    let name = if let Some(n) = name {
        n.to_string()
    } else if let Some(active_name) = active.clone() {
        // No name given: default to overwriting the active profile.
        if cli.force || !is_interactive() {
            active_name
        } else {
            let confirm = dialoguer::Confirm::new()
                .with_prompt(format!("Save changes to active profile \"{active_name}\"?"))
                .default(true)
                .interact()?;
            if confirm {
                active_name
            } else {
                dialoguer::Input::<String>::new()
                    .with_prompt("Profile name")
                    .interact_text()?
            }
        }
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

    let is_active = active.as_deref() == Some(name.as_str());

    // Existing profile + not the active one + not --force: confirm overwrite.
    // Overwriting the *active* profile is the natural action for `portal save`
    // (like saving over a running game), so we don't gate it behind a prompt.
    if paths.profile_dir(&name).exists() && !cli.force && !is_active {
        if !is_interactive() {
            bail!("Profile \"{name}\" already exists. Use --force to overwrite.");
        }
        let choice = dialoguer::Select::new()
            .with_prompt(format!("Profile \"{name}\" already exists"))
            .items(&[
                "Overwrite (replace entirely)",
                "Merge (keep new, update changed)",
                "Cancel",
            ])
            .default(2)
            .interact()?;
        match choice {
            // Both overwrite and merge fall through; save rebuilds the
            // manifest and replaces files either way.
            0 | 1 => {}
            _ => {
                println!("{}", style("Aborted.").yellow());
                return Ok(());
            }
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

    let progress = CliProgress::new(if is_active { "Updating" } else { "Saving" });
    let result = snapshot::save_with_progress(paths, &name, description, tags, &progress)?;
    progress.finish(&format!(
        "{} profile \"{}\" ({} files)",
        if is_active { "Updated" } else { "Saved" },
        style(&name).green().bold(),
        result.files.len()
    ));

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

    let progress = CliProgress::new("Loading");
    let result = loader::load_with_progress(
        paths,
        name,
        cli.no_plugins,
        cli.no_backup,
        cli.force,
        &progress,
    )?;
    progress.finish(&format!(
        "Loaded profile \"{}\" ({} files)",
        style(&result.profile).green().bold(),
        result.files_loaded
    ));

    if !result.plugin_results.is_empty() && !cli.quiet {
        print_plugin_results(&result.plugin_results, cli.verbose);
    }

    if !cli.quiet {
        println!("  Backup: {}", style(result.backup_path.display()).dim());
    }

    Ok(())
}

// ── toggle ───────────────────────────────────────────────────────────

fn cmd_toggle(cli: &Cli, paths: &PortalPaths) -> Result<()> {
    let portal_state = state::read(&paths.state_file())?;
    let Some(target) = portal_state.previous_profile else {
        bail!(
            "No previous profile to toggle to. Load a profile first, then \
             load another to establish toggle history."
        );
    };

    if cli.dry_run {
        println!("[dry-run] Would toggle to profile \"{target}\"");
        return Ok(());
    }

    if cli.no_backup && !cli.force {
        bail!("--no-backup requires --force.");
    }

    let progress = CliProgress::new("Toggling");
    let result = loader::load_with_progress(
        paths,
        &target,
        cli.no_plugins,
        cli.no_backup,
        cli.force,
        &progress,
    )?;
    progress.finish(&format!(
        "Toggled to \"{}\" ({} files)",
        style(&result.profile).green().bold(),
        result.files_loaded
    ));

    if !result.plugin_results.is_empty() && !cli.quiet {
        print_plugin_results(&result.plugin_results, cli.verbose);
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
        "  {:<2} {:<20} {:<8} {:<10} {:<8} {}",
        "",
        style("Profile").bold().underlined(),
        style("Files").bold().underlined(),
        style("Size").bold().underlined(),
        style("Plugins").bold().underlined(),
        style("Tags").bold().underlined(),
    );

    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let manifest_path = paths.profile_manifest(&name);

        let (file_count, total_size, tags) = if manifest_path.exists() {
            match manifest::read(&manifest_path) {
                Ok(m) => {
                    let count = m.files.len();
                    let size: u64 = m.files.values().map(|f| f.size).sum();
                    let tags = m.tags.join(", ");
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

        // Pad name before styling so ANSI escapes don't break alignment
        let padded_name = format!("{name:<20}");
        let styled_name = if is_active {
            style(padded_name).green().bold().to_string()
        } else {
            padded_name
        };

        println!(
            "  {:<2} {} {:<8} {:<10} {:<8} {}",
            marker,
            styled_name,
            file_count,
            format_size(total_size),
            plugin_count,
            truncate_str(&tags, 20),
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
    if plugins_path.exists()
        && let Ok(bp) = plugins_manifest::read(&plugins_path)
    {
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

    println!();
    println!("  {}", style("Files:").underlined());
    let mut sorted_files: Vec<_> = m.files.iter().collect();
    sorted_files.sort_by_key(|(k, _)| (*k).clone());
    for (path, entry) in &sorted_files {
        let source_tag = match entry.source {
            portal::core::profile::FileSource::User => "",
            portal::core::profile::FileSource::Skeleton => " (skeleton)",
        };
        println!("    {path} {:>6}{source_tag}", format_size(entry.size));
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
    let right = b.map_or(diff::DiffSide::Skeleton, diff::DiffSide::Profile);

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
            .with_prompt(format!("Delete profile \"{name}\"? This cannot be undone"))
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

    // Shared with the TUI: removes the profile reference and clears state
    // pointers, but never touches the compressed backups under `backups/`.
    remove::delete_profile(paths, name)?;

    println!("{} Deleted profile \"{name}\"", style("✓").green().bold());
    println!(
        "{}",
        style("  backups under ~/.config/portal/backups are kept").dim()
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
            .with_prompt("Reset .claude/ to skeleton? Current configuration will be backed up")
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

    println!("{} Reset .claude/ to skeleton", style("✓").green().bold());
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

#[allow(clippy::too_many_lines)]
fn cmd_status(_cli: &Cli, paths: &PortalPaths) -> Result<()> {
    let state_path = paths.state_file();
    let portal_state = state::read(&state_path)?;

    println!("{}", style("Portal Status").bold());

    match &portal_state.active_profile {
        Some(name) => {
            println!("  Active profile: {}", style(name).green().bold());
            let manifest_path = paths.profile_manifest(name);
            if manifest_path.exists()
                && let Ok(m) = manifest::read(&manifest_path)
            {
                let total_size: u64 = m.files.values().map(|f| f.size).sum();
                println!(
                    "  Files:          {} ({})",
                    m.files.len(),
                    format_size(total_size)
                );
                if let Some(loaded) = m.last_loaded {
                    println!("  Last loaded:    {}", loaded.format("%Y-%m-%d %H:%M UTC"));
                }

                // Integrity check against stored profile.
                let files_dir = paths.profile_files_dir(name);
                match checksum::verify_manifest(&files_dir, &m.files) {
                    Ok(mismatches) if mismatches.is_empty() => {
                        println!(
                            "  Integrity:      {} all {} files verified",
                            style("✓").green(),
                            m.files.len()
                        );
                    }
                    Ok(mismatches) => {
                        println!(
                            "  Integrity:      {} {} file(s) differ",
                            style("✗").red(),
                            mismatches.len()
                        );
                    }
                    Err(_) => {
                        println!("  Integrity:      {}", style("error reading files").red());
                    }
                }

                // Plugin health.
                let plugins_path = paths.profile_plugins(name);
                if plugins_path.exists()
                    && let Ok(bp) = plugins_manifest::read(&plugins_path)
                {
                    println!("  Plugins:        {} blueprinted", bp.plugins.len());
                }
            }
        }
        None => {
            println!("  Active profile: {}", style("(none)").dim());
        }
    }

    if let Some(ref op) = portal_state.last_operation {
        println!();
        println!("  Last operation: {:?} \"{}\"", op.op_type, op.profile);
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

    // Crash recovery warning.
    if paths.claude_old().exists() {
        println!();
        println!(
            "  {} .claude.portal-old exists; previous swap may have crashed",
            style("WARNING:").red().bold()
        );
        println!("  Run {} to recover.", style("portal recover").bold());
    }

    // Count profiles.
    let profile_count = std::fs::read_dir(paths.profiles_root())
        .map_or(0, |rd| rd.filter_map(std::result::Result::ok).count());
    println!("  Profiles:         {profile_count}");

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

    // Update active / previous profile pointers in state if either matches.
    let state_path = paths.state_file();
    let mut portal_state = state::read(&state_path)?;
    let renamed_active = portal_state.active_profile.as_deref() == Some(old);
    let renamed_previous = portal_state.previous_profile.as_deref() == Some(old);
    if renamed_active {
        portal_state.active_profile = Some(new.to_string());
    }
    if renamed_previous {
        portal_state.previous_profile = Some(new.to_string());
    }
    if renamed_active || renamed_previous {
        state::write(&state_path, &portal_state)?;
    }

    println!(
        "{} Renamed \"{old}\" -> \"{new}\"",
        style("✓").green().bold(),
    );

    Ok(())
}

// ── verify ───────────────────────────────────────────────────────────

fn cmd_verify(cli: &Cli, paths: &PortalPaths, name: Option<&str>, fix_plugins: bool) -> Result<()> {
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

    // Checksum verification.
    let spinner = progress_spinner("Verifying checksums...");
    let mismatches = checksum::verify_manifest(&files_dir, &m.files)?;
    drop(spinner);

    if mismatches.is_empty() {
        println!(
            "{} Checksums: {}/{} files verified",
            style("✓").green().bold(),
            m.files.len(),
            m.files.len()
        );
    } else {
        println!(
            "{} Checksums: {} mismatch(es):",
            style("✗").red().bold(),
            mismatches.len()
        );
        for mm in &mismatches {
            println!(
                "  {} (expected {}, got {})",
                mm.path, mm.expected, mm.actual
            );
        }
    }

    // Plugin verification.
    let plugins_path = paths.profile_plugins(&name);
    if plugins_path.exists()
        && let Ok(bp) = plugins_manifest::read(&plugins_path)
    {
        println!("  Plugins:   {} blueprinted", bp.plugins.len());
        if fix_plugins && !bp.plugins.is_empty() {
            println!();
            println!("  {}", style("Reinstalling plugins...").bold());
            // verify --fix-plugins always does a full reinstall — it's
            // the user's escape hatch when plugin code is missing or
            // corrupt, so we deliberately bypass the diff fast-path.
            let results = plugins::reinstall(&bp);
            print_plugin_results(&results, cli.verbose);
            let failed: Vec<_> = results.iter().filter(|r| !r.success).collect();
            if !failed.is_empty() {
                println!(
                    "\n  {} {}/{} plugins failed to install",
                    style("!").yellow().bold(),
                    failed.len(),
                    results.len()
                );
            }
        }
    }

    if !mismatches.is_empty() && !cli.quiet {
        bail!("Integrity check failed for profile \"{name}\".");
    }

    Ok(())
}

// ── clone ───────────────────────────────────────────────────────────

#[allow(clippy::fn_params_excessive_bools, clippy::too_many_arguments)]
fn cmd_clone(
    cli: &Cli,
    paths: &PortalPaths,
    source: &str,
    target: &str,
    description: &str,
    only: Option<&str>,
    without: Option<&str>,
    fresh_claude_md: bool,
) -> Result<()> {
    let only_cats = only.map(clone::parse_categories).transpose()?;
    let without_cats = without.map(clone::parse_categories).transpose()?;

    if only_cats.is_some() && without_cats.is_some() {
        bail!("Cannot use both --only and --without. Pick one.");
    }

    if cli.dry_run {
        let label = match (&only_cats, &without_cats) {
            (Some(cats), _) => format!(
                "only {:?}",
                cats.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>()
            ),
            (_, Some(cats)) => format!(
                "without {:?}",
                cats.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>()
            ),
            _ => "all categories".to_string(),
        };
        println!(
            "[dry-run] Would clone \"{}\" -> \"{}\" ({}{})",
            source,
            target,
            label,
            if fresh_claude_md {
                ", fresh CLAUDE.md"
            } else {
                ""
            }
        );
        return Ok(());
    }

    paths.ensure_dirs()?;

    let opts = clone::CloneOptions {
        source,
        target,
        description,
        only: only_cats,
        without: without_cats,
        fresh_claude_md,
        file_picks: None, // CLI clone operates at category granularity only
    };

    let progress = CliProgress::new("Cloning");
    let result = clone::clone_profile_with_progress(paths, &opts, &progress)?;
    progress.finish(&format!(
        "Cloned \"{}\" -> \"{}\"",
        style(&result.source).cyan(),
        style(&result.target).green().bold(),
    ));

    println!(
        "  {} files cloned, {} skipped",
        result.files_cloned, result.files_skipped
    );
    if !result.categories_included.is_empty() {
        println!("  Categories: {}", result.categories_included.join(", "));
    }
    if result.plugins_included {
        println!("  Plugins: included");
    }

    Ok(())
}

// ── export ──────────────────────────────────────────────────────────

fn cmd_export(cli: &Cli, paths: &PortalPaths, name: &str, output: Option<&str>) -> Result<()> {
    let output_path = output.map_or_else(
        || std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        std::path::PathBuf::from,
    );

    if cli.dry_run {
        let target = if output_path.is_dir() {
            output_path.join(format!("{name}.portal.tar.zst"))
        } else {
            output_path
        };
        println!(
            "[dry-run] Would export profile \"{name}\" to {}",
            target.display()
        );
        return Ok(());
    }

    let spinner = progress_spinner("Exporting profile...");
    let archive_path = transport::export(paths, name, &output_path)?;
    let size = std::fs::metadata(&archive_path).map_or(0, |m| m.len());
    finish_spinner(
        &spinner,
        &format!(
            "Exported \"{}\" ({})",
            style(name).green().bold(),
            format_size(size)
        ),
    );
    println!("  Archive: {}", archive_path.display());

    Ok(())
}

// ── import ──────────────────────────────────────────────────────────

fn cmd_import(cli: &Cli, paths: &PortalPaths, archive: &str) -> Result<()> {
    let archive_path = std::path::Path::new(archive);

    if cli.dry_run {
        println!(
            "[dry-run] Would import profile from {}",
            archive_path.display()
        );
        return Ok(());
    }

    paths.ensure_dirs()?;

    let spinner = progress_spinner("Importing profile...");
    let name = transport::import(paths, archive_path, cli.force)?;
    finish_spinner(
        &spinner,
        &format!("Imported profile \"{}\"", style(&name).green().bold()),
    );

    // Show summary of what was imported.
    let manifest_path = paths.profile_manifest(&name);
    if manifest_path.exists()
        && let Ok(m) = manifest::read(&manifest_path)
    {
        let total_size: u64 = m.files.values().map(|f| f.size).sum();
        println!("  {} files ({})", m.files.len(), format_size(total_size));
    }

    Ok(())
}

// ── recover ─────────────────────────────────────────────────────────

fn cmd_recover(cli: &Cli, paths: &PortalPaths) -> Result<()> {
    let old_dir = paths.claude_old();
    if !old_dir.exists() {
        println!(
            "{} No crash recovery needed. .claude.portal-old does not exist.",
            style("✓").green().bold()
        );
        return Ok(());
    }

    let claude_dir = paths.claude_root();

    // Check if current .claude/ exists and is valid.
    let current_valid = claude_dir.exists() && claude_dir.join("settings.json").exists();

    if current_valid {
        println!("Found .claude.portal-old from a previously crashed swap.");
        println!();
        println!(
            "  Current .claude/ appears {}, .portal-old is a backup from before the swap.",
            style("valid").green()
        );

        if cli.force {
            // --force: assume swap succeeded, clean up.
            std::fs::remove_dir_all(&old_dir)?;
            println!(
                "{} Removed stale .claude.portal-old",
                style("✓").green().bold()
            );
        } else {
            if !is_interactive() {
                bail!("Use --force to clean up .portal-old automatically.");
            }
            let choice = dialoguer::Select::new()
                .with_prompt("What would you like to do?")
                .items(&[
                    "Keep current .claude/ and remove .portal-old (swap succeeded)",
                    "Restore .portal-old as .claude/ (swap failed, rollback)",
                    "Cancel",
                ])
                .default(0)
                .interact()?;
            match choice {
                0 => {
                    std::fs::remove_dir_all(&old_dir)?;
                    println!("{} Removed .claude.portal-old", style("✓").green().bold());
                }
                1 => {
                    if claude_dir.exists() {
                        std::fs::remove_dir_all(&claude_dir)?;
                    }
                    std::fs::rename(&old_dir, &claude_dir)?;
                    println!(
                        "{} Restored .claude/ from .portal-old",
                        style("✓").green().bold()
                    );
                }
                _ => {
                    println!("{}", style("Aborted.").yellow());
                }
            }
        }
    } else {
        // Current .claude/ is missing or invalid; .portal-old is likely the real config.
        println!(
            "{} .claude/ is missing or invalid. Restoring from .portal-old...",
            style("!").yellow().bold()
        );
        if claude_dir.exists() {
            std::fs::remove_dir_all(&claude_dir)?;
        }
        std::fs::rename(&old_dir, &claude_dir)?;
        println!(
            "{} Restored .claude/ from .portal-old",
            style("✓").green().bold()
        );
    }

    Ok(())
}

// ── doctor ───────────────────────────────────────────────────────────

fn cmd_doctor(cli: &Cli, paths: &PortalPaths, fix: bool) -> Result<()> {
    let report = doctor::diagnose(paths)?;

    println!("{}", style("Portal Doctor").bold());
    println!();

    // Managed-vs-excluded overview.
    println!("  {}", style("Managed directories").bold());
    for row in &report.managed_dirs {
        let marker = if row.exists {
            style("✓").green()
        } else {
            style("·").dim()
        };
        println!(
            "    {marker} {:<16} {:<10} {}",
            row.dir,
            style(format!("[{}]", row.category)).dim(),
            if row.exists {
                format!("{} file(s)", row.file_count)
            } else {
                style("missing").dim().to_string()
            }
        );
    }
    println!(
        "    {} ignored: {}",
        style("·").dim(),
        style(report.excluded_patterns.join(", ")).dim()
    );
    println!();

    // Checks.
    println!("  {}", style("Checks").bold());
    for c in &report.checks {
        println!(
            "    {} {}: {}",
            severity_icon(c.severity),
            c.title,
            c.detail
        );
    }

    // Fixes.
    let fixable: Vec<&doctor::Check> = report.fixable().collect();
    if !fixable.is_empty() {
        println!();
        if fix {
            for c in &fixable {
                apply_doctor_fix(cli, paths, c)?;
            }
        } else {
            println!(
                "  {} {} fixable issue(s). Run {} to repair.",
                style("→").cyan(),
                fixable.len(),
                style("portal doctor --fix").bold()
            );
        }
    }

    // Re-evaluate so the exit code reflects post-fix reality.
    let final_report = if fix {
        doctor::diagnose(paths)?
    } else {
        report
    };
    if final_report.has_errors() {
        bail!("portal doctor found unresolved error(s).");
    }
    Ok(())
}

/// Prompt for and apply a single fix. Legacy roots get a migrate/delete/skip
/// choice; everything else is a yes/no confirm. `--force` skips prompts.
fn apply_doctor_fix(cli: &Cli, paths: &PortalPaths, check: &doctor::Check) -> Result<()> {
    let Some(action) = &check.fix else {
        return Ok(());
    };

    let action = match action {
        doctor::FixAction::MigrateLegacyRoot { name, dir } if !cli.force => {
            if !is_interactive() {
                println!(
                    "  {} {} (skipped; use --force or run interactively)",
                    style("·").dim(),
                    check.detail
                );
                return Ok(());
            }
            let choice = dialoguer::Select::new()
                .with_prompt(format!("Legacy profile \"{name}\""))
                .items(&["Migrate into active root", "Delete", "Skip"])
                .default(0)
                .interact()?;
            match choice {
                0 => doctor::FixAction::MigrateLegacyRoot {
                    name: name.clone(),
                    dir: dir.clone(),
                },
                1 => doctor::FixAction::DeleteLegacyRoot { dir: dir.clone() },
                _ => return Ok(()),
            }
        }
        other if !cli.force => {
            if !is_interactive() {
                println!(
                    "  {} {} (skipped; use --force or run interactively)",
                    style("·").dim(),
                    check.detail
                );
                return Ok(());
            }
            let proceed = dialoguer::Confirm::new()
                .with_prompt(format!("Fix: {}?", check.detail))
                .default(true)
                .interact()?;
            if !proceed {
                return Ok(());
            }
            other.clone()
        }
        other => other.clone(),
    };

    match doctor::apply_fix(paths, &action) {
        Ok(summary) => println!("  {} {summary}", style("✓").green().bold()),
        Err(e) => println!("  {} {e}", style("✗").red().bold()),
    }
    Ok(())
}

fn severity_icon(s: doctor::Severity) -> console::StyledObject<&'static str> {
    match s {
        doctor::Severity::Ok => style("✓").green(),
        doctor::Severity::Info => style("·").dim(),
        doctor::Severity::Warning => style("!").yellow(),
        doctor::Severity::Error => style("✗").red(),
    }
}

// ── history ──────────────────────────────────────────────────────────

fn cmd_history(_cli: &Cli, paths: &PortalPaths, name: Option<&str>) -> Result<()> {
    let profile = match name {
        Some(n) => n.to_string(),
        None => {
            let Some(active) = state::read(&paths.state_file())?.active_profile else {
                bail!("No active profile; specify a profile name.");
            };
            active
        }
    };

    let commits = git_history::log(paths, &profile)?;
    println!(
        "{} {}",
        style("History for").bold(),
        style(&profile).green().bold()
    );
    if commits.is_empty() {
        println!("  {}", style("(no history recorded yet)").dim());
        return Ok(());
    }
    for c in &commits {
        let short = &c.hash[..c.hash.len().min(8)];
        println!(
            "  {} {}  {}",
            style(short).yellow(),
            style(&c.timestamp).dim(),
            c.summary
        );
    }
    Ok(())
}

// ── use (bind-mode) ──────────────────────────────────────────────────

fn cmd_use(
    _cli: &Cli,
    paths: &PortalPaths,
    name: Option<&str>,
    print_env: bool,
    no_refresh: bool,
    args: &[String],
) -> Result<()> {
    // Resolve the profile: explicit name, else the active profile.
    let profile = match name {
        Some(n) => n.to_string(),
        None => {
            let Some(active) = state::read(&paths.state_file())?.active_profile else {
                bail!("No active profile; specify a profile name.");
            };
            active
        }
    };

    let target = if no_refresh {
        let dir = paths.live_dir(&profile);
        if !dir.join(".portal-stamp").is_file() {
            bail!(
                "Profile \"{profile}\" has no materialized session dir yet. \
                 Run `portal use {profile}` once without --no-refresh first."
            );
        }
        bind::BindTarget {
            dir,
            refreshed: false,
        }
    } else {
        bind::materialize(paths, &profile, false)?
    };

    if print_env {
        println!("export CLAUDE_CONFIG_DIR={}", target.dir.display());
        return Ok(());
    }

    // Replace this process with `claude`, bound to the isolated config dir. On
    // success exec never returns; a returned error means the launch failed.
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new("claude");
    cmd.env("CLAUDE_CONFIG_DIR", &target.dir).args(args);
    let err = cmd.exec();
    if err.kind() == std::io::ErrorKind::NotFound {
        bail!("claude not found on PATH — install the Claude Code CLI to launch a bound session");
    }
    Err(err).with_context(|| "launching claude")
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

/// Pretty-print a `PluginInstallResult` slice with a tally line.
///
/// Distinguishes three states with three icons:
///   - `⋯` skipped (already installed from a prior load — no work done)
///   - `✓` freshly installed
///   - `✗` install failed
fn print_plugin_results(results: &[plugins::PluginInstallResult], verbose: bool) {
    if results.is_empty() {
        return;
    }
    println!();
    println!("  {}", style("Plugins:").bold());
    let mut installed = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    for pr in results {
        let icon = if pr.skipped {
            skipped += 1;
            style("  ⋯").dim()
        } else if pr.success {
            installed += 1;
            style("  ✓").green()
        } else {
            failed += 1;
            style("  ✗").red()
        };
        println!("{icon} {}", pr.id);
        if verbose && !pr.message.is_empty() {
            println!("    {}", pr.message.trim());
        }
    }
    // Tally line — only render when it adds information beyond the icons.
    if results.len() > 1 {
        let mut parts: Vec<String> = Vec::new();
        if installed > 0 {
            parts.push(format!("{installed} installed"));
        }
        if skipped > 0 {
            parts.push(format!("{skipped} already current"));
        }
        if failed > 0 {
            parts.push(format!("{failed} failed"));
        }
        if !parts.is_empty() {
            println!("  {}", style(parts.join(", ")).dim());
        }
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

/// Progress bar that shows `[current/total] filename` during file operations.
struct CliProgress {
    pb: indicatif::ProgressBar,
}

impl CliProgress {
    #[allow(clippy::expect_used, clippy::literal_string_with_formatting_args)]
    fn new(prefix: &str) -> Self {
        let pb = indicatif::ProgressBar::new(0);
        pb.set_style(
            indicatif::ProgressStyle::with_template("{spinner:.cyan} {prefix} [{pos}/{len}] {msg}")
                .expect("valid template")
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
        );
        pb.set_prefix(prefix.to_string());
        pb.enable_steady_tick(std::time::Duration::from_millis(80));
        Self { pb }
    }
}

impl portal::core::progress::ProgressReporter for CliProgress {
    fn set_total(&self, total: u64) {
        self.pb.set_length(total);
    }

    fn tick(&self, current: u64, item: &str) {
        self.pb.set_position(current);
        self.pb.set_message(item.to_string());
    }

    fn finish(&self, message: &str) {
        self.pb
            .finish_with_message(format!("{} {message}", style("✓").green().bold()));
    }

    fn phase(&self, label: &str) {
        // Roll the prefix label as we cross phase boundaries so the spinner
        // line reads "Backing up [..]", then "Building [..]", etc.
        self.pb.set_prefix(label.to_string());
        // Reset the per-phase length: each phase that emits set_total will
        // overwrite it; phases that don't emit ticks (backup, swap) stay at 0.
        self.pb.set_length(0);
        self.pb.set_position(0);
        self.pb.set_message(String::new());
    }
}
