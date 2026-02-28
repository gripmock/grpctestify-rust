// LSP command - start language server

use crate::cli::args::LspArgs;
use crate::lsp::server::start_lsp_server;
use anyhow::Result;

pub async fn handle_lsp(_args: &LspArgs) -> Result<()> {
    start_lsp_server().await
}
