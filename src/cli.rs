use anyhow::Result;

/// Run the CLI entry point.
///
/// # Errors
///
/// Returns an error if command execution fails.
#[allow(clippy::unnecessary_wraps)]
pub fn run() -> Result<()> {
    println!("portal v{}", env!("CARGO_PKG_VERSION"));
    Ok(())
}
