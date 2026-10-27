use clap::Args;

#[derive(Args, Default)]
pub(crate) struct LocalArgs {
    /// Workspace for the local agent (defaults to the current directory).
    #[arg(long)]
    pub workspace: Option<std::path::PathBuf>,
    /// Resume a saved local session UUID in the same workspace.
    #[arg(long)]
    pub resume: Option<uuid::Uuid>,
    /// Local model provider (currently anthropic).
    #[arg(long)]
    pub provider: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    /// Full Anthropic Messages URL; HTTP is allowed only on loopback.
    #[arg(long)]
    pub endpoint: Option<String>,
    /// Local execution policy. Full access uses your OS permissions.
    #[arg(long, value_enum)]
    pub permission: Option<Permission>,
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    clap::ValueEnum,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Permission {
    ReadOnly,
    #[default]
    Ask,
    FullAccess,
}
