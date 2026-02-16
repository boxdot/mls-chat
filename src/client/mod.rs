use std::{path::Path, str::FromStr};

use openmls_sqlx_storage::SqliteStorageProvider;
use sqlx::{
    ConnectOptions, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
};
use tonic::transport::{Channel, Endpoint};
use tracing::info;

use crate::{grpc::chat_service_client::ChatServiceClient, provider::JsonCodec};

pub mod create_group;
pub mod member;
pub mod message;
pub mod register;

pub struct Client {
    pub(crate) client: ChatServiceClient<Channel>,
    pub(crate) connection: SqliteConnection,
}

impl Client {
    pub async fn connect(endpoint: &str, db_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let db_path = db_path.as_ref();
        info!(db_path = %db_path.display(), "Opening client database");
        let mut connection = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .connect()
            .await?;
        sqlx::migrate!().run(&mut connection).await?;
        SqliteStorageProvider::<JsonCodec>::new(&mut connection).run_migrations()?;

        let channel = Endpoint::from_str(endpoint)?.connect_lazy();
        let client = ChatServiceClient::new(channel);
        Ok(Self { client, connection })
    }
}
