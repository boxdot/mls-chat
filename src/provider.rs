use openmls::prelude::{Ciphersuite, OpenMlsProvider, ProtocolVersion};
use openmls_rust_crypto::RustCrypto;
use openmls_sqlx_storage::{Codec, SqliteStorageProvider};
use serde::{Serialize, de::DeserializeOwned};

use crate::client::Client;

pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::Mls10;

pub(crate) const CIPHERSUITE: Ciphersuite =
    Ciphersuite::MLS_128_DHKEMX25519_CHACHA20POLY1305_SHA256_Ed25519;

impl Client {
    pub(crate) fn provider(&mut self) -> Provider<'_> {
        let storage = SqliteStorageProvider::<JsonCodec>::new(&mut self.connection);
        Provider::new(storage)
    }
}

pub(crate) struct Provider<'a> {
    storage: SqliteStorageProvider<'a, JsonCodec>,
    crypto: RustCrypto,
}

impl<'a> Provider<'a> {
    pub(crate) fn new(storage: SqliteStorageProvider<'a, JsonCodec>) -> Self {
        Self {
            storage,
            crypto: Default::default(),
        }
    }
}

impl<'a> OpenMlsProvider for Provider<'a> {
    type CryptoProvider = RustCrypto;

    type RandProvider = RustCrypto;

    type StorageProvider = SqliteStorageProvider<'a, JsonCodec>;

    fn storage(&self) -> &Self::StorageProvider {
        &self.storage
    }

    fn crypto(&self) -> &Self::CryptoProvider {
        &self.crypto
    }

    fn rand(&self) -> &Self::RandProvider {
        &self.crypto
    }
}

#[derive(Default)]
pub(crate) struct JsonCodec;

impl Codec for JsonCodec {
    type Error = serde_json::Error;

    fn to_vec<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, Self::Error> {
        serde_json::to_vec(value)
    }

    fn from_slice<T: DeserializeOwned>(slice: &[u8]) -> Result<T, Self::Error> {
        serde_json::from_slice(slice)
    }
}
