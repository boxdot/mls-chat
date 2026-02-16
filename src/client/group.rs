use anyhow::Context;
use openmls::{
    group::{GroupId, MlsGroup},
    prelude::{LeafNodeParameters, OpenMlsProvider, tls_codec::Serialize},
};
use tracing::debug;
use uuid::Uuid;

use crate::{client::Client, grpc::SendMessageRequest, provider::CIPHERSUITE};

impl Client {
    pub async fn create_group(&mut self, user: String) -> anyhow::Result<Uuid> {
        let (signing_private_key, credential_with_key) = self.credential(&user).await?;

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

    pub async fn update_group(&mut self, user: String, group_uuid: Uuid) -> anyhow::Result<()> {
        let (signing_private_key, _credential_with_key) = self.credential(&user).await?;

        let group_id = GroupId::from_slice(group_uuid.as_bytes());
        let provider = self.provider();
        let mut group =
            MlsGroup::load(provider.storage(), &group_id)?.context("Group not found")?;

        let bundle = group.self_update(
            &provider,
            &signing_private_key,
            LeafNodeParameters::builder().build(),
        )?;
        group.merge_pending_commit(&provider)?;

        let recipients = group
            .members()
            .filter_map(|member| {
                let member = str::from_utf8(member.credential.serialized_content()).ok()?;
                (member != user).then(|| member.to_string())
            })
            .collect();

        self.client
            .send_message(SendMessageRequest {
                sender: user,
                recipients,
                content: bundle.into_commit().tls_serialize_detached()?,
            })
            .await?;

        Ok(())
    }
}
