use clap::Parser;

#[derive(Debug, Parser)]
pub struct QuarantineCommand {
    #[command(subcommand)]
    pub action: QuarantineAction,
}

#[derive(Debug, clap::Subcommand)]
pub enum QuarantineAction {
    /// List tests currently in the quarantine set.
    List,
}
