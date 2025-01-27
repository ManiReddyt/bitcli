use bip39::{Language, Mnemonic};
use bitcoin::{
    absolute::LockTime,
    bip32::{DerivationPath, Xpriv},
    consensus::serialize,
    key::Secp256k1,
    secp256k1::{Message, SecretKey},
    sighash::SighashCache,
    transaction::Version,
    Address, Amount, CompressedPublicKey, EcdsaSighashType, Network, OutPoint, PrivateKey, Script,
    ScriptBuf, Sequence, Transaction, TxIn, TxOut,
};
use dirs_next::data_dir;
use rand::RngCore;
use reqwest::Client;
use serde::Deserialize;
use std::{error::Error, fs, path::PathBuf, str::FromStr};

#[derive(Debug)]
pub struct Wallet {
    private_key: String,
    network: Network,
    address: Address,
    wif_key: String,
}

#[derive(Debug, Deserialize)]
struct BalanceResponse {
    #[serde(rename = "chain_stats")]
    chain_stats: ChainStats,
}

#[derive(Debug, Deserialize)]
struct ChainStats {
    funded_txo_sum: u64,
    spent_txo_sum: u64,
}

type UtxoResponse = Vec<Utxo>;

#[derive(Debug, Deserialize)]
struct Utxo {
    txid: String,
    vout: u32,
    value: u64,
    status: UtxoStatus,
}

#[derive(Debug, Deserialize)]
struct UtxoStatus {
    confirmed: bool,
    block_height: Option<u32>,
    block_hash: Option<String>,
    block_time: Option<u32>,
}

struct Fee {
    low: u32,
    medium: u32,
    high: u32,
}

#[derive(Debug, Deserialize)]
struct MempoolFeeResponse {
    #[serde(rename = "fastestFee")]
    fastest_fee: u32,
    #[serde(rename = "halfHourFee")]
    half_hour_fee: u32,
    #[serde(rename = "hourFee")]
    hour_fee: u32,
    #[serde(rename = "minimumFee")]
    minimum_fee: u32,
    #[serde(rename = "economyFee")]
    economy_fee: u32,
}

impl Wallet {
    pub fn create(network: Network) -> Self {
        let mut entropy: [u8; 16] = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut entropy);

        let mnemonic = Mnemonic::from_entropy(&entropy).expect("Failed to generate mnemonic");

        let wallet = Self::from_mnemonic(&mnemonic.to_string(), network);
        wallet
    }

    pub fn from_private_key(key: &str, network: Network) -> Self {
        let secret_key =
            SecretKey::from_str(key).expect("Failed to parse private key to secret key");
        let private_key = PrivateKey::new(secret_key, network);

        //get the public key
        let secp = Secp256k1::new();
        let compressed_public_key = CompressedPublicKey::from_private_key(&secp, &private_key)
            .expect("Failed to create compressed public key");
        let address = Address::p2wpkh(&compressed_public_key, Network::Testnet);

        Self {
            private_key: private_key.to_string(),
            network,
            address,
            wif_key: key.to_string(),
        }
    }

    pub fn from_mnemonic(mnemonic_phrase: &str, network: Network) -> Self {
        //parse the mnemonic phrase
        let mnemonic = Mnemonic::parse_in(Language::English, mnemonic_phrase)
            .expect("Failed to parse mnemonic");

        //get the seed
        let seed = mnemonic.to_seed("");

        // Use the seed to derive an extended private key (BIP32 root key)
        let secp = Secp256k1::new();
        let xpriv = Xpriv::new_master(Network::Bitcoin, &seed)
            .expect("Failed to create extended private key");

        // Derive a specific private key (e.g., m/44'/0'/0'/0/0 for the first Bitcoin address)
        let derivation_path = "m/84'/0'/0'/0/0"
            .parse::<DerivationPath>()
            .expect("Invalid derivation path");

        let child_priv_key = xpriv
            .derive_priv(&secp, &derivation_path)
            .expect("Failed to derive private key");

        let private_key = child_priv_key.private_key.display_secret().to_string();

        let wallet = Self::from_private_key(&private_key, network);

        Self::save_mnemonic(&mnemonic_phrase);

        wallet
    }

    fn get_storage_path() -> PathBuf {
        // Get the OS-specific data directory and append your app's name
        let mut path = data_dir().expect("Could not find data directory");
        path.push("bitcli");
        fs::create_dir_all(&path).expect("Failed to create app data directory");
        path
    }

    fn save_mnemonic(mnemonic: &str) {
        let storage_path = Self::get_storage_path();
        let file_path = storage_path.join("mnemonic.txt");
        fs::write(file_path, mnemonic).expect("Failed to save mnemonic");
    }

    pub fn load_mnemonic() -> String {
        let storage_path = Self::get_storage_path();
        let file_path = storage_path.join("mnemonic.txt");
        let mnemonic = match fs::read_to_string(file_path) {
            Ok(mnemonic) => mnemonic,
            Err(_) => "".to_string(),
        };
        mnemonic
    }

    fn get_api_url(network: Network) -> String {
        match network {
            Network::Testnet => "https://mempool.space/testnet4".to_string(),
            Network::Bitcoin => "https://mempool.space".to_string(),
            _ => "".to_string(),
        }
    }

    pub fn get_address(&self) -> String {
        self.address.to_string()
    }

    pub async fn get_balance(&self) -> Result<u64, Box<dyn Error>> {
        let api_url = Self::get_api_url(self.network);
        if api_url.is_empty() {
            return Err("Invalid network".into());
        }

        let url = format!("{}/api/address/{}", api_url, self.address.to_string());
        let response = reqwest::get(url).await?;
        let data: BalanceResponse = response.json().await?;
        Ok(data.chain_stats.funded_txo_sum - data.chain_stats.spent_txo_sum)
    }

    pub fn get_network(&self) -> String {
        self.network.to_string()
    }

    pub fn reset(&self) {
        let storage_path = Self::get_storage_path();
        fs::remove_dir_all(storage_path).expect("Failed to reset");
    }

    async fn fetch_utxos(&self) -> Result<UtxoResponse, Box<dyn Error>> {
        let api_url = Self::get_api_url(self.network);
        if api_url.is_empty() {
            return Err("Invalid network".into());
        }

        let url = format!("{}/api/address/{}/utxo", api_url, self.address.to_string());
        let response = reqwest::get(url).await?;
        let data: UtxoResponse = response.json().await.expect("Failed to parse utxos");

        Ok(data)
    }

    async fn fetch_fee_rates(&self) -> Result<Fee, Box<dyn Error>> {
        let api_url = Self::get_api_url(self.network);
        if api_url.is_empty() {
            return Err("Invalid network".into());
        }

        let url = format!("{}/api/v1/fees/recommended", api_url);
        let response = reqwest::get(url).await?;
        let data: MempoolFeeResponse = response.json().await?;

        Ok(Fee {
            low: data.minimum_fee,
            medium: data.half_hour_fee,
            high: data.fastest_fee,
        })
    }

    fn estimate_tx_size(&self, inputs: u32, outputs: u32) -> u32 {
        let size = 10 + (inputs * 148) + (outputs * 34);
        size
    }

    fn sign_tx(
        &self,
        mut tx: Transaction,
        utxos: &Vec<Utxo>,
    ) -> Result<Transaction, Box<dyn Error>> {
        let secp = Secp256k1::new();
        let mut sighasher = SighashCache::new(&mut tx);
        let secret_key = SecretKey::from_str(&self.wif_key).unwrap();

        // Sign each input
        for (index, utxo) in utxos.iter().enumerate() {
            let sighash_type = EcdsaSighashType::All;
            let amount = Amount::from_sat(utxo.value);
            let sighash = sighasher.p2wpkh_signature_hash(
                index,
                self.address.script_pubkey().as_script(),
                amount,
                sighash_type,
            )?;

            // Sign sighash
            let sighash_bytes: &[u8] = &sighash[..];
            let message = Message::from_digest_slice(&sighash_bytes).unwrap();
            let signature = secp.sign_ecdsa(&message, &secret_key);

            // Convert signature to Bitcoin-specific format
            let mut sig_with_hashtype = signature.serialize_der().to_vec();
            sig_with_hashtype.push(sighash_type as u8);

            // Add public key for verification
            let public_key = secret_key.public_key(&secp);
            let public_key_bytes = public_key.serialize().to_vec();

            // Update witness
            sighasher
                .witness_mut(index)
                .unwrap()
                .push(sig_with_hashtype);
            sighasher.witness_mut(index).unwrap().push(public_key_bytes);
        }

        Ok(sighasher.into_transaction().clone())
    }

    async fn build_tx(
        &self,
        to: &str,
        amount: u64,
        utxos: &Vec<Utxo>,
    ) -> Result<Transaction, Box<dyn Error>> {
        let mut total_utxos_value: u64 = 0;

        let fee_rate = self
            .fetch_fee_rates()
            .await
            .expect("Failed to fetch fee rates");
        let fee = fee_rate.high * self.estimate_tx_size(utxos.len() as u32, 2);

        let inputs: Vec<TxIn> = utxos
            .iter()
            .map(|utxo| {
                let txid = bitcoin::Txid::from_str(&utxo.txid).unwrap();

                total_utxos_value += utxo.value;

                TxIn {
                    previous_output: OutPoint {
                        txid,
                        vout: utxo.vout,
                    },
                    script_sig: ScriptBuf::default(),
                    sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                    witness: bitcoin::Witness::new(),
                }
            })
            .collect();

        if amount + fee as u64 > total_utxos_value {
            return Err("Insufficient funds".into());
        }

        let change = total_utxos_value - amount - fee as u64;

        let amt = Amount::from_sat(amount);
        let recipient_address = Address::from_str(to)
            .expect("Invalid address")
            .require_network(self.network)
            .expect("Invalid address");
        let tx_out = TxOut {
            value: amt,
            script_pubkey: recipient_address.script_pubkey(),
        };

        let change_amt = Amount::from_sat(change);
        let change_tx_out = TxOut {
            value: change_amt,
            script_pubkey: self.address.script_pubkey(),
        };

        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: inputs,
            output: vec![tx_out, change_tx_out],
        };

        Ok(tx)
    }

    async fn broadcast(&self, tx: Transaction) -> Result<String, Box<dyn Error>> {
        let api_url = Self::get_api_url(self.network);
        if api_url.is_empty() {
            return Err("Invalid network".into());
        }

        let client = Client::new();

        let raw_tx = serialize(&tx);
        let raw_tx_hex = hex::encode(raw_tx);

        let url = format!("{}/api/tx", api_url);
        let response = client
            .post(url)
            .header("Content-Type", "application/json")
            .body(raw_tx_hex)
            .send()
            .await?;

        if response.status().is_success() {
            let txid = response.text().await?;
            Ok(txid)
        } else {
            let error_message = response.text().await?;
            Err(format!("Failed to broadcast transaction: {}", error_message).into())
        }
    }

    pub async fn send(&self, to: &str, amount: u64) -> Result<String, Box<dyn Error>> {
        let utxos: Vec<Utxo> = self.fetch_utxos().await?;
        let tx = self.build_tx(to, amount, &utxos).await?;
        let signed_tx = self.sign_tx(tx, &utxos)?;

        let txid = self.broadcast(signed_tx).await?;

        Ok(txid)
    }
}
