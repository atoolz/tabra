pub mod bash_hook;
pub mod fish_hook;
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
        ShellType::Bash => bash_hook::bash_hook(),
        ShellType::Fish => fish_hook::fish_hook(),
    };
    print!("{script}");
    Ok(())
}
