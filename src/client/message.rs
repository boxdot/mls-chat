use anyhow::{Context, bail};
use openmls::{
    group::{GroupId, MlsGroup, MlsGroupJoinConfig, StagedWelcome},
    prelude::{
        BasicCredential, DeserializeBytes, MlsMessageBodyIn, MlsMessageIn, ProcessedMessageContent,
        ProtocolMessage, Sender, tls_codec::Serialize,
    },
};
use openmls_traits::OpenMlsProvider;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    client::Client,
    grpc::{ReceiveMessagesRequest, SendMessageRequest},
};

impl Client {
    pub async fn send(
        &mut self,
        user: String,
        group_uuid: Uuid,
        message: String,
    ) -> anyhow::Result<()> {
        let (signing_private_key, _credential_with_key) = self.credential(&user).await?;

        let group_id = GroupId::from_slice(group_uuid.as_bytes());

        let provider = self.provider();
        let mut group =
            MlsGroup::load(provider.storage(), &group_id)?.context("Group not found")?;
        let message = group.create_message(&provider, &signing_private_key, message.as_bytes())?;

        let recipients = group
            .members()
            .filter_map(|member| {
                let credential = BasicCredential::try_from(member.credential).ok()?;
                let member = str::from_utf8(credential.identity()).ok()?;
                if member != user {
                    Some(member.to_string())
                } else {
                    None
                }
            })
            .collect();

        self.client
            .send_message(SendMessageRequest {
                sender: user.clone(),
                recipients,
                content: message.tls_serialize_detached()?,
            })
            .await?;

        Ok(())
    }

    pub async fn receive(&mut self, user: String) -> anyhow::Result<()> {
        let mut messages = self
            .client
            .receive_messages(ReceiveMessagesRequest { client_id: user })
            .await?
            .into_inner();

        while let Some(message) = messages.message().await? {
            let message: MlsMessageIn =
                MlsMessageIn::tls_deserialize_exact_bytes(&message.content)?;

            let message = message.extract();

            info!(?message, "Incoming message");

            match message {
                MlsMessageBodyIn::PublicMessage(message) => {
                    self.handle_protocol_message(message)?;
                }
                MlsMessageBodyIn::PrivateMessage(message) => {
                    self.handle_protocol_message(message)?;
                }
                MlsMessageBodyIn::Welcome(welcome) => {
                    let provider = self.provider();
                    let group_config = MlsGroupJoinConfig::builder().build();
                    let staged_welcome =
                        StagedWelcome::new_from_welcome(&provider, &group_config, welcome, None)?;
                    let group = staged_welcome.into_group(&provider)?;
                    let group_id = Uuid::from_slice(group.group_id().as_slice())?;
                    info!(%group_id, "Received welcome and joined group");
                }
                MlsMessageBodyIn::GroupInfo(_) => bail!("GroupInfo not supported"),
                MlsMessageBodyIn::KeyPackage(_) => bail!("KeyPackage not supported"),
            }
        }

        Ok(())
    }

    fn handle_protocol_message(
        &mut self,
        message: impl Into<ProtocolMessage>,
    ) -> Result<(), anyhow::Error> {
        let message = message.into();

        let provider = self.provider();

        let mut group =
            MlsGroup::load(provider.storage(), message.group_id())?.context("Group not found")?;
        let processed_message = group.process_message(&provider, message)?;

        let sender = match processed_message.sender() {
            Sender::Member(leaf_index) => {
                let member = group.member_at(*leaf_index).context("Member not found")?;
                String::from_utf8_lossy(&member.credential.serialized_content()).into_owned()
            }
            _ => {
                warn!("Received message from non-member");
                return Ok(());
            }
        };
        Ok(match processed_message.into_content() {
            ProcessedMessageContent::ApplicationMessage(application_message) => {
                let text = String::from_utf8_lossy(&application_message.into_bytes()).into_owned();
                println!("{sender}: {text}");
            }
            ProcessedMessageContent::ProposalMessage(queued_proposal) => {
                group.store_pending_proposal(provider.storage(), (*queued_proposal).clone())?
            }
            ProcessedMessageContent::ExternalJoinProposalMessage(queued_proposal) => {
                group.store_pending_proposal(provider.storage(), (*queued_proposal).clone())?;
            }
            ProcessedMessageContent::StagedCommitMessage(staged_commit) => {
                group.merge_staged_commit(&provider, *staged_commit)?;
            }
        })
    }
}
