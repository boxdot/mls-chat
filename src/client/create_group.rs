use openmls::group::{GroupId, MlsGroup};
use tracing::debug;
use uuid::Uuid;

use crate::{client::Client, provider::CIPHERSUITE};

impl Client {
    pub async fn create_group(&mut self, username: String) -> anyhow::Result<Uuid> {
        let (signing_private_key, credential_with_key) = self.credential(&username).await?;

        let group_uuid = Uuid::new_v4();
        let group_id = GroupId::from_slice(group_uuid.as_bytes());

        let group = MlsGroup::builder()
            .with_group_id(group_id)
            .ciphersuite(CIPHERSUITE)
            .use_ratchet_tree_extension(true)
            .build(&self.provider(), &signing_private_key, credential_with_key)?;

        debug!(?group, "Created group");

        Ok(group_uuid)
    }
}
