use anyhow::{Context, anyhow};
use openmls::prelude::{
    BasicCredential, Capabilities, Credential, CredentialWithKey, ExtensionType, KeyPackage,
    OpenMlsCrypto, SignaturePublicKey, SignatureScheme, tls_codec::Serialize,
};
use openmls_rust_crypto::RustCrypto;
use openmls_sqlx_storage::Codec;
use openmls_traits::signatures::{Signer, SignerError};
use sqlx::query;

use crate::{
    client::Client,
    grpc::{self, UploadKeyPackageRequest},
    provider::{CIPHERSUITE, JsonCodec},
};

impl Client {
    pub async fn register(&mut self, username: String) -> anyhow::Result<()> {
        let credential: Credential = BasicCredential::new(username.as_bytes().to_vec()).into();

        let (signature_private_key, signature_key) = SignaturePrivateKey::generate();

        let credential_with_key = CredentialWithKey {
            credential,
            signature_key,
        };

        let credential_with_key_blob = JsonCodec::to_vec(&credential_with_key)?;
        query!(
            "INSERT INTO client_user (
                username,
                signature_private_key,
                credential_with_key
            ) VALUES (?, ?, ?)",
            username,
            signature_private_key.key,
            credential_with_key_blob,
        )
        .execute(&mut self.connection)
        .await?;

        let key_package_bundle = KeyPackage::builder()
            .leaf_node_capabilities(
                Capabilities::builder()
                    .extensions(vec![ExtensionType::LastResort])
                    .build(),
            )
            .mark_as_last_resort()
            .build(
                CIPHERSUITE,
                &self.provider(),
                &signature_private_key,
                credential_with_key,
            )?;

        self.client
            .upload_key_package(UploadKeyPackageRequest {
                client_id: username.clone(),
                key_package: Some(grpc::KeyPackage {
                    key_package_bytes: key_package_bundle.key_package().tls_serialize_detached()?,
                }),
            })
            .await?;

        Ok(())
    }

    pub(crate) async fn credential(
        &mut self,
        username: &str,
    ) -> anyhow::Result<(SignaturePrivateKey, CredentialWithKey)> {
        let record = sqlx::query!(
            "SELECT
                signature_private_key,
                credential_with_key
            FROM client_user
            WHERE username = ?",
            username
        )
        .fetch_optional(&mut self.connection)
        .await?
        .with_context(|| anyhow!("User {username} is not registered"))?;

        let signature_private_key = SignaturePrivateKey {
            key: record.signature_private_key,
        };
        let credential_with_key: CredentialWithKey =
            JsonCodec::from_slice(&record.credential_with_key)?;

        Ok((signature_private_key, credential_with_key))
    }
}

pub(crate) struct SignaturePrivateKey {
    key: Vec<u8>,
}

impl SignaturePrivateKey {
    fn generate() -> (Self, SignaturePublicKey) {
        let (sk, pk) = RustCrypto::default()
            .signature_key_gen(SignatureScheme::ED25519)
            .unwrap();
        let signature_key = SignaturePublicKey::from(pk);
        let signature_private_key = SignaturePrivateKey { key: sk };
        (signature_private_key, signature_key)
    }
}

impl Signer for SignaturePrivateKey {
    fn sign(&self, payload: &[u8]) -> Result<Vec<u8>, SignerError> {
        RustCrypto::default()
            .sign(SignatureScheme::ED25519, payload, &self.key)
            .map_err(SignerError::CryptoError)
    }

    fn signature_scheme(&self) -> SignatureScheme {
        SignatureScheme::ED25519
    }
}
