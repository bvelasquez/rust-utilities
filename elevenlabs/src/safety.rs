use anyhow::{bail, Result};

pub fn require_mutation_approval(yes: bool, dry_run: bool, action: &str) -> Result<()> {
    if dry_run {
        return Ok(());
    }
    if yes {
        return Ok(());
    }
    if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        use dialoguer::Confirm;
        let ok = Confirm::new()
            .with_prompt(format!("Apply mutation: {action}?"))
            .default(false)
            .interact()?;
        if ok {
            return Ok(());
        }
    }
    bail!("mutation requires --yes, --dry-run, or interactive confirmation (non-TTY: use --yes)")
}
