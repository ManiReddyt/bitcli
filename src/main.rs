use std::time::Duration;

use bitcoin::Network;
use clap::{Parser, Subcommand};
use dotenv::dotenv;
use indicatif::{ProgressBar, ProgressStyle};

mod wallet;

/// Cli bitcoin wallet
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a wallet
    #[command(alias = "c")]
    Create,
    /// Send bitcoin to an address
    #[command(alias = "s")]
    Send {
        /// The address to send the bitcoin to
        to: String,
        /// The amount of bitcoin to send
        amount: u64,
    },
    /// Create a wallet from a mnemonic phrase
    #[command(alias = "m")]
    Mnemonic {
        /// The mnemonic phrase
        mnemonic: Vec<String>,
    },
    /// Get the balance of the wallet
    #[command(alias = "b")]
    Balance,
    /// Get the address of the wallet
    #[command(alias = "a")]
    Address,
    /// Get the network of the wallet
    #[command(alias = "n")]
    Network,
    /// Reset the wallet
    #[command(alias = "r")]
    Reset,
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    let args = Cli::parse();

    let existing_mnemonic = wallet::Wallet::load_mnemonic();

    let wallet = if !existing_mnemonic.is_empty() {
        Some(wallet::Wallet::from_mnemonic(
            &existing_mnemonic,
            Network::Testnet,
        ))
    } else {
        None
    };

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::with_template("{spinner:.red} {msg}")
            .unwrap()
            .tick_strings(&["-", "\\", "|", "=>"]),
    );

    spinner.enable_steady_tick(Duration::from_millis(100));

    match args.command {
        Commands::Create => {
            let wallet = wallet::Wallet::create(Network::Testnet);
            println!("{:?}", wallet);
        }
        Commands::Send { to, amount } => match wallet {
            Some(wallet) => {
                spinner.set_message("Sending transaction...");
                match wallet.send(&to, amount).await {
                    Ok(txid) => println!("Transaction submitted successfully: {}", txid),
                    Err(e) => println!("Error submitting transaction: {}", e),
                }
            }
            None => println!("Wallet not initialized"),
        },
        Commands::Mnemonic { mnemonic } => {
            let wallet =
                wallet::Wallet::from_mnemonic(mnemonic.join(" ").as_str(), Network::Testnet);
            println!("{:?}", wallet);
        }
        Commands::Balance => match wallet {
            Some(wallet) => {
                spinner.set_message("Fetching balance...");
                spinner.finish_with_message(format!(
                    "Balance: {}",
                    wallet.get_balance().await.unwrap()
                ));
            }
            None => println!("Wallet not initialized"),
        },
        Commands::Address => match wallet {
            Some(wallet) => {
                spinner.set_message("Fetching address...");
                spinner.finish_with_message(format!("Address: {}", wallet.get_address()));
            }
            None => println!("Wallet not initialized"),
        },
        Commands::Network => match wallet {
            Some(wallet) => spinner.finish_with_message(wallet.get_network()),
            None => println!("Wallet not initialized"),
        },
        Commands::Reset => match wallet {
            Some(wallet) => wallet.reset(),
            None => println!("Wallet not initialized"),
        },
        // Commands::History => match wallet {
        //     Some(wallet) => println!("History: {:?}", wallet.get_history()),
        //     None => println!("Wallet not initialized"),
        // },
    }
}
