pub use solana_sdk::pubkey;
pub use solana_sdk::{pubkey::Pubkey, pubkey::PUBKEY_BYTES, signature::Signature, signer::Signer};

pub mod serde_pubkey {
    use super::*;
    use serde::de::{self, Deserialize};
    use std::str::FromStr;

    pub fn serialize<S>(value: &Pubkey, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deser: D) -> std::result::Result<Pubkey, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let str = String::deserialize(deser)?;
        Pubkey::from_str(&str).map_err(|_| de::Error::custom("invalid public key"))
    }
}

pub mod serde_opt_pubkey {
    use super::*;
    use serde::{Deserialize, Serialize};

    pub fn serialize<S>(
        value: &Option<Pubkey>,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct Helper<'a>(#[serde(with = "serde_pubkey")] &'a Pubkey);
        value.as_ref().map(Helper).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<Option<Pubkey>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper(#[serde(with = "serde_pubkey")] Pubkey);
        let helper = Option::deserialize(deserializer)?;
        Ok(helper.map(|Helper(external)| external))
    }
}

pub mod serde_vec_pubkey {
    use super::*;
    use serde::{Deserialize, Serialize};

    pub fn serialize<S>(value: &[Pubkey], serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct Helper<'a>(#[serde(with = "serde_pubkey")] &'a Pubkey);
        value
            .iter()
            .map(Helper)
            .collect::<Vec<_>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<Vec<Pubkey>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper(#[serde(with = "serde_pubkey")] Pubkey);
        let helper = Vec::<Helper>::deserialize(deserializer)?;
        Ok(helper.iter().map(|h| h.0).collect())
    }
}

pub fn print_json<T: ?Sized + serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
