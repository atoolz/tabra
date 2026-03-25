pub mod hook;

use clap::ValueEnum;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ShellType {
    Zsh,
    Bash,
    Fish,
}

/// Print the shell hook script to stdout.
/// Usage: eval "$(tabra init zsh)"
pub fn print_hook(shell: ShellType) -> anyhow::Result<()> {
    let script = match shell {
        ShellType::Zsh => hook::zsh_hook(),
        ShellType::Bash => {
            eprintln!("bash support is not yet implemented (coming soon)");
            return Ok(());
        }
        ShellType::Fish => {
            eprintln!("fish support is not yet implemented (coming soon)");
            return Ok(());
        }
    };
    print!("{script}");
    Ok(())
}
