use anyhow::{bail, Result};
use std::io::{self, IsTerminal, Write};

pub fn confirm_mutation(dry_run: bool, yes: bool, prompt: &str) -> Result<()> {
    if dry_run {
        return Ok(());
    }
    if yes {
        return Ok(());
    }
    if !io::stdout().is_terminal() {
        bail!("refusing mutation in non-interactive mode without --yes or --dry-run");
    }
    print!("{prompt}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    if line.trim() != "yes" {
        bail!("aborted");
    }
    Ok(())
}
