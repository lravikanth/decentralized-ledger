use crate::sha256::Hash;
use crate::util::Savable;
use ecdsa::signature::rand_core::OsRng;
use ecdsa::{
    Signature as ECDSASignature, SigningKey, VerifyingKey, signature::Signer, signature::Verifier,
};
use k256::Secp256k1;
use serde::{Deserialize, Serialize};
use spki::EncodePublicKey;
use std::cmp::Ordering;
use std::io::{Error as IoError, ErrorKind as IoErrorKind, Read, Result as IoResult, Write};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Signature(ECDSASignature<Secp256k1>);

impl Signature {
    // Sign a TransactionOutput (represented by its SHA256 hash)
    pub fn sign_output(output_hash: &Hash, private_key: &PrivateKey) -> Self {
        let signing_key = &private_key.0; // 1. Extract SigningKey<Secp256k1>
        let signature = signing_key.sign(&output_hash.as_bytes()); // 2. Sign the hash bytes
        Signature(signature) // 3. Wrap it inside Signature newtype
    }

    // Verify a signature
    pub fn verify(&self, output_hash: &Hash, public_key: &PublicKey) -> bool {
        public_key
            .0
            .verify(&output_hash.as_bytes(), &self.0) // 4. Verify
            .is_ok() // 5. Return true if valid
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct PublicKey(VerifyingKey<Secp256k1>);

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PrivateKey(#[serde(with = "signkey_serde")] pub SigningKey<Secp256k1>);

mod signkey_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialize a SigningKey<Secp256k1> as raw bytes
    pub fn serialize<S>(
        key: &super::SigningKey<super::Secp256k1>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&key.to_bytes())
    }

    /// Deserialize a SigningKey<Secp256k1> from raw bytes
    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<super::SigningKey<super::Secp256k1>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Vec::<u8>::deserialize(deserializer)?;
        Ok(super::SigningKey::from_slice(&bytes).unwrap())
    }
}

impl PrivateKey {
    pub fn new() -> Self {
        PrivateKey(SigningKey::random(&mut OsRng))
    }

    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.0.verifying_key().clone())
    }
}

impl Savable for PrivateKey {
    fn load<I: Read>(reader: I) -> IoResult<Self> {
        ciborium::de::from_reader(reader)
            .map_err(|_| IoError::new(IoErrorKind::InvalidData, "Failed to deserialize PrivateKey"))
    }

    fn save<O: Write>(&self, writer: O) -> IoResult<()> {
        ciborium::ser::into_writer(self, writer).map_err(|_| {
            IoError::new(IoErrorKind::InvalidData, "Failed to serialize PrivateKey")
        })?;

        Ok(())
    }
}

impl Savable for PublicKey {
    fn load<I: Read>(mut reader: I) -> IoResult<Self> {
        let mut buf = String::new();
        reader.read_to_string(&mut buf);

        let public_key = buf
            .parse()
            .map_err(|_| IoError::new(IoErrorKind::InvalidData, "Failed to parse public key"))?;

        Ok(PublicKey(public_key))
    }

    fn save<O: Write>(&self, mut writer: O) -> IoResult<()> {
        // Convert the public key to PEM (string)
        let pem = self
            .0
            .to_public_key_pem(Default::default())
            .map_err(|_| IoError::new(IoErrorKind::InvalidData, "Failed to serialize PublicKey"))?;

        // Write the PEM bytes to the writer
        writer.write_all(pem.as_bytes())?;

        Ok(())
    }
}
impl PublicKey {
    /// Return the compressed SEC1 encoding of this public key (33 bytes)
    pub fn to_bytes(&self) -> [u8; 33] {
        let encoded = self.0.to_encoded_point(true); // compressed
        let bytes = encoded.as_bytes();
        // SEC1 compressed for secp256k1 is always 33 bytes
        bytes
            .try_into()
            .expect("SEC1 encoded point must be 33 bytes")
    }
}
impl Ord for PublicKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_bytes().cmp(&other.to_bytes())
    }
}

impl PartialOrd for PublicKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
