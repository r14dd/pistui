use clap::Parser;

#[derive(Parser)]
#[command(
    name = "pistui",
    version,
    about = "Interactive UI for exploring Langfuse traces"
)]
pub struct Args {
    /// Langfuse host URL (e.g. https://cloud.langfuse.com)
    #[arg(long, env = "LANGFUSE_HOST")]
    pub host: String,

    /// Langfuse public key
    #[arg(long, env = "LANGFUSE_PUBLIC_KEY")]
    pub public_key: String,

    /// Langfuse secret key
    #[arg(long, env = "LANGFUSE_SECRET_KEY")]
    pub secret_key: String,

    /// Port to bind the local server (0 = auto-pick)
    #[arg(long, default_value = "0")]
    pub port: u16,

    /// Don't open the browser automatically
    #[arg(long)]
    pub no_open: bool,
}
