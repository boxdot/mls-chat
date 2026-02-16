use std::{path::Path, pin::Pin, result::Result};

use crate::{
    grpc::{
        self, FetchKeyPackageRequest, FetchKeyPackageResponse, ReceiveMessagesRequest,
        SendMessageRequest, SendMessageResponse, UploadKeyPackageRequest, UploadKeyPackageResponse,
        chat_service_server::ChatService,
    },
    provider::PROTOCOL_VERSION,
};
use dashmap::DashMap;
use openmls::prelude::{BasicCredential, DeserializeBytes, KeyPackageIn};
use openmls_rust_crypto::RustCrypto;
use sqlx::{
    SqlitePool, migrate, query, query_scalar,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
    types::chrono::{DateTime, Utc},
};
use tokio::sync::mpsc;
use tokio_stream::{Stream, StreamExt, wrappers::ReceiverStream};
use tonic::{Request, Response, Status};
use uuid::Uuid;

pub struct ChatServiceImpl {
    pool: SqlitePool,
    connected: DashMap<String, mpsc::Sender<Result<grpc::ReceiveMessagesResponse, Status>>>,
}

impl ChatServiceImpl {
    pub async fn new(db_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let opts: SqliteConnectOptions = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Extra);
        let pool = SqlitePool::connect_with(opts).await?;
        migrate!().run(&pool).await?;
        Ok(Self {
            pool,
            connected: DashMap::new(),
        })
    }
}

#[tonic::async_trait]
impl ChatService for ChatServiceImpl {
    async fn create_group(
        &self,
        _request: Request<grpc::CreateGroupRequest>,
    ) -> std::result::Result<Response<grpc::CreateGroupResponse>, Status> {
        todo!()
    }

    async fn add_member(
        &self,
        _request: Request<grpc::AddMemberRequest>,
    ) -> std::result::Result<Response<grpc::AddMemberResponse>, Status> {
        todo!()
    }

    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        let request = request.into_inner();

        let message_id = Uuid::new_v4();
        let created_at = Utc::now();

        for recipient in request.recipients {
            if let Some(tx) = self.connected.get(&recipient)
                && tx
                    .send(Ok(grpc::ReceiveMessagesResponse {
                        content: request.content.clone(),
                        timestamp: created_at.timestamp_millis(),
                    }))
                    .await
                    .is_ok()
            {
                continue;
            }
            self.enqueue_message(message_id, recipient, request.content.clone(), created_at)
                .await
                .map_err(|error| Status::internal(format!("Database error: {error}")))?;
        }

        let response = SendMessageResponse {
            timestamp: created_at.timestamp_millis(),
        };
        Ok(response.into())
    }

    type ReceiveMessagesStream =
        Pin<Box<dyn Stream<Item = Result<grpc::ReceiveMessagesResponse, Status>> + Send + 'static>>;

    async fn receive_messages(
        &self,
        request: Request<ReceiveMessagesRequest>,
    ) -> Result<Response<Self::ReceiveMessagesStream>, Status> {
        let client_id = request.into_inner().client_id;
        let records = query!(
            "WITH target_messages AS (
                SELECT message_id
                FROM server_message
                WHERE recipient = ?
                ORDER BY created_at ASC
            )
            DELETE FROM server_message
            WHERE message_id IN (SELECT message_id FROM target_messages)
            RETURNING
                content,
                created_at as \"created_at: DateTime<Utc>\"",
            client_id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| Status::internal(format!("Database error: {error}")))?;

        let messages = tokio_stream::iter(records.into_iter().map(|record| {
            let content = record.content;
            let created_at = record.created_at;
            Ok(grpc::ReceiveMessagesResponse {
                content,
                timestamp: created_at.timestamp_millis(),
            })
        }));

        let (tx, rx) = tokio::sync::mpsc::channel(100);

        self.connected.insert(client_id.clone(), tx);

        let messages = messages.chain(ReceiverStream::new(rx));

        Ok(Response::new(Box::pin(messages)))
    }

    async fn upload_key_package(
        &self,
        request: Request<UploadKeyPackageRequest>,
    ) -> Result<Response<UploadKeyPackageResponse>, Status> {
        let request = request.into_inner();
        let client_id = request.client_id;
        let key_package_proto = request
            .key_package
            .ok_or_else(|| Status::invalid_argument("Key package is required"))?;
        let key_package =
            KeyPackageIn::tls_deserialize_exact_bytes(&key_package_proto.key_package_bytes)
                .map_err(|_| Status::invalid_argument("Invalid key package bytes"))?;

        let key_package = key_package
            .validate(&RustCrypto::default(), PROTOCOL_VERSION)
            .map_err(|error| Status::invalid_argument(format!("Invalid key package: {error}")))?;

        let credential: BasicCredential = key_package
            .leaf_node()
            .credential()
            .clone()
            .try_into()
            .map_err(|error| {
            Status::invalid_argument(format!("Invalid credential: {error}"))
        })?;

        if !key_package.last_resort() {
            return Err(Status::invalid_argument("Key package is not last resort"));
        }

        if credential.identity() != client_id.as_bytes() {
            return Err(Status::invalid_argument(
                "Client ID mismatch with credential",
            ));
        }

        let package_id = Uuid::new_v4();
        let created_at = Utc::now();

        sqlx::query!(
            "INSERT INTO server_key_package (
                package_id, client_id, package, created_at
            ) VALUES (?, ?, ?, ?)",
            package_id,
            client_id,
            key_package_proto.key_package_bytes,
            created_at,
        )
        .execute(&self.pool)
        .await
        .map_err(|error| Status::internal(format!("Database error: {error}")))?;

        Ok(Response::new(UploadKeyPackageResponse {
            package_id: package_id.to_string(),
        }))
    }

    async fn fetch_key_package(
        &self,
        request: Request<FetchKeyPackageRequest>,
    ) -> Result<Response<FetchKeyPackageResponse>, Status> {
        let client_id = request.into_inner().client_id;

        let key_package_bytes = query_scalar!(
            "SELECT package FROM server_key_package WHERE client_id = ?",
            client_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| Status::internal(format!("Database error: {error}")))?;

        let Some(key_package_bytes) = key_package_bytes else {
            return Err(Status::not_found(format!(
                "No key package found for client {}",
                client_id
            )));
        };

        Ok(Response::new(FetchKeyPackageResponse {
            key_package: Some(grpc::KeyPackage { key_package_bytes }),
        }))
    }
}

impl ChatServiceImpl {
    async fn enqueue_message(
        &self,
        message_id: Uuid,
        recipient: String,
        content: Vec<u8>,
        created_at: DateTime<Utc>,
    ) -> sqlx::Result<()> {
        sqlx::query!(
            "INSERT INTO server_message (
                message_id, recipient, content, created_at
            ) VALUES (?, ?, ?, ?)",
            message_id,
            recipient,
            content,
            created_at,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // async fn dequeue_messages<'a>(
    //     &'a self,
    //     client_id: &'a str,
    // ) -> impl Stream<Item = sqlx::Result<Vec<u8>>> + 'a {
    //     query_scalar!(
    //         "WITH target_messages AS (
    //             SELECT message_id
    //             FROM server_message
    //             WHERE recipient = ?
    //             ORDER BY created_at ASC
    //         )
    //         DELETE FROM server_message
    //         WHERE message_id IN (SELECT message_id FROM target_messages)
    //         RETURNING content",
    //         client_id,
    //     )
    //     .fetch(&self.pool)
    // }
}
