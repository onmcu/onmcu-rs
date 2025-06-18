use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "onmcu")]
#[command(about = "A CLI tool for remote MCU development and testing")]
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run and flash firmware to MCU
    Run {
        /// Target chip name
        #[arg(long)]
        chip: String,
        
        /// Path to the binary or package to flash
        path: String,
    },
}

fn main() {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Run { chip, path } => {
            handle_run_command(chip, path);
        }
    }
}

fn handle_run_command(chip: String, path: String) {
    // Check if chip is supported
    if !is_chip_supported(&chip) {
        eprintln!("Error: Chip '{}' is not supported.", chip);
        eprintln!("See supported chips at: https://docs.onmcu.com/chips");
        std::process::exit(1);
    }
    
    println!("Path: {}", path);
    println!("Chip: {}", chip);
    
    // Here you would implement the actual flashing logic
    println!("Uploading binary...");
}

fn is_chip_supported(_chip: &str) -> bool {
    // TODO: Currently always returns true
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_chip_support() {
        assert!(is_chip_supported("nRF52840_xxAA"));
        assert!(is_chip_supported("any_chip"));
    }
}
