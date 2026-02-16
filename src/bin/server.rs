use std::{net::SocketAddr, time::Duration};

use mls_chat::{grpc::chat_service_server::ChatServiceServer, server::ChatServiceImpl};
use tracing::{Span, info};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::fmt().init();
    let listen: SocketAddr = "[::]:50051".parse()?;
    info!(%listen, "Starting server");
    let service = ChatServiceServer::new(ChatServiceImpl::new("db/server.db").await?);
    tonic::transport::Server::builder()
        .layer(
            tower_http::trace::TraceLayer::new_for_grpc()
                .make_span_with(|request: &http::Request<_>| {
                    tracing::info_span!(
                        "request",
                        path = request.uri().path(),
                        status_code = tracing::field::Empty,
                    )
                })
                .on_request(|_request: &http::Request<_>, _span: &_| {
                    info!("request");
                })
                .on_response(
                    |response: &http::Response<_>, latency: Duration, span: &Span| {
                        span.record("status_code", response.status().as_u16());
                        info!(?latency, status = %response.status(), "response");
                    },
                ),
        )
        .add_service(service)
        .serve(listen)
        .await?;
    Ok(())
}
