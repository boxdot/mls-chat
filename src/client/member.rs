use anyhow::{Context, ensure};
use openmls::{
    group::{GroupId, MlsGroup},
    prelude::{BasicCredential, DeserializeBytes, KeyPackageIn, tls_codec::Serialize},
};
use openmls_traits::OpenMlsProvider;
use uuid::Uuid;

use crate::{
    client::Client,
    grpc::{FetchKeyPackageRequest, SendMessageRequest},
    provider::PROTOCOL_VERSION,
};

impl Client {
    pub async fn add_member(
        &mut self,
        username: String,
        group_uuid: Uuid,
        new_member: String,
    ) -> anyhow::Result<()> {
        let (signing_private_key, _credential_with_key) = self.credential(&username).await?;

        let response = self
            .client
            .fetch_key_package(FetchKeyPackageRequest {
                client_id: new_member.clone(),
            })
            .await?
            .into_inner();

        let provider = self.provider();

        let key_package_bytes = response
            .key_package
            .context("Missing key package")?
            .key_package_bytes;
        let key_package = KeyPackageIn::tls_deserialize_exact_bytes(&key_package_bytes)?;
        let key_package = key_package.validate(provider.crypto(), PROTOCOL_VERSION)?;

        let group_id = GroupId::from_slice(group_uuid.as_bytes());
        let mut group =
            MlsGroup::load(provider.storage(), &group_id)?.context("Group not found")?;

        let members: Vec<String> = group
            .members()
            .filter_map(|member| {
                let credential = BasicCredential::try_from(member.credential).ok()?;
                let member = str::from_utf8(credential.identity()).ok()?;
                if member != username {
                    Some(member.to_string())
                } else {
                    None
                }
            })
            .collect();

        ensure!(!members.contains(&new_member), "Member already exists");

        let (commit, welcome, _group_info) =
            group.add_members(&provider, &signing_private_key, &[key_package])?;

        group.merge_pending_commit(&provider)?;

        if !members.is_empty() {
            self.client
                .send_message(SendMessageRequest {
                    sender: username.clone(),
                    recipients: members,
                    content: commit.tls_serialize_detached()?,
                })
                .await?;
        }

        self.client
            .send_message(SendMessageRequest {
                sender: username.clone(),
                recipients: vec![new_member],
                content: welcome.tls_serialize_detached()?,
            })
            .await?;

        Ok(())
    }

    pub async fn remove_member(
        &mut self,
        sender: String,
        group_uuid: Uuid,
        remove_member: String,
    ) -> anyhow::Result<()> {
        let (signing_private_key, _credential_with_key) = self.credential(&sender).await?;

        let provider = self.provider();

        let group_id = GroupId::from_slice(group_uuid.as_bytes());
        let mut group =
            MlsGroup::load(provider.storage(), &group_id)?.context("Group not found")?;

        let leaf_index = group
            .members()
            .find_map(|member| {
                let credential = BasicCredential::try_from(member.credential).ok()?;
                let user = str::from_utf8(credential.identity()).ok()?;
                if user == remove_member {
                    Some(member.index)
                } else {
                    None
                }
            })
            .context("Member not found")?;

        let (commit, welcome, _) =
            group.remove_members(&provider, &signing_private_key, &[leaf_index])?;
        ensure!(welcome.is_none(), "Nobody should be added to the group");

        group.merge_pending_commit(&provider)?;

        let recipients: Vec<String> = group
            .members()
            .filter_map(|member| {
                let credential = BasicCredential::try_from(member.credential).ok()?;
                let user = str::from_utf8(credential.identity()).ok()?;
                if user != sender {
                    Some(user.to_string())
                } else {
                    None
                }
            })
            .collect();

        if !recipients.is_empty() {
            self.client
                .send_message(SendMessageRequest {
                    sender: sender.clone(),
                    recipients,
                    content: commit.tls_serialize_detached()?,
                })
                .await?;
        }

        Ok(())
    }
}
