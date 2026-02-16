use clap::{Parser, Subcommand};
use mls_chat::client::Client;
use tracing::info;
use uuid::Uuid;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    user: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Register a new user
    Register {},
    /// Create a new group
    CreateGroup {},
    /// Update own key material in the group
    UpdateGroup {
        #[arg(short, long)]
        group: Uuid,
    },
    /// Add a member to a group
    AddMember {
        #[arg(short, long)]
        group: Uuid,
        #[arg(short, long)]
        member: String,
    },
    /// Remove a member from a group
    RemoveMember {
        #[arg(short, long)]
        group: Uuid,
        #[arg(short, long)]
        member: String,
    },
    /// Send a message to a group
    Send {
        #[arg(short, long)]
        group: Uuid,
        message: String,
    },
    /// Receive messages
    Receive {},
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = init();

    let db_path = format!("db/client-{}.db", args.user);

    let mut client = Client::connect("http://localhost:50051", db_path).await?;

    match args.command {
        Commands::Register {} => {
            info!(user = args.user, "Registering user");
            client.register(args.user).await?;
        }
        Commands::CreateGroup {} => {
            info!("Creating group");
            let group_id = client.create_group(args.user).await?;
            println!("{group_id}");
        }
        Commands::UpdateGroup { group } => {
            info!(%group, "Updating group key material");
            client.update_group(args.user, group).await?;
        }
        Commands::Send { group, message } => {
            info!(%group, "Sending message to group");
            client.send(args.user, group, message).await?;
        }
        Commands::Receive {} => {
            info!("Receiving messages");
            client.receive(args.user).await?;
        }
        Commands::AddMember { group, member } => {
            info!("Adding user {} to group: {}", member, group);
            client.add_member(args.user, group, member).await?;
        }
        Commands::RemoveMember { group, member } => {
            info!("Removing user {} from group: {}", member, group);
            client.remove_member(args.user, group, member).await?;
        }
    }

    Ok(())
}

fn init() -> Args {
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing::metadata::LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();
    Args::parse()
}
