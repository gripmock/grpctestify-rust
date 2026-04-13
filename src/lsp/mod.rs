pub mod document_symbols;
pub mod folding_ranges;
pub mod handlers;
pub mod inlay_hints;
pub mod semantic_tokens;
pub mod server;
pub mod variable_definition;

pub use document_symbols::build_section_children_for_doc;
pub use folding_ranges::build_folding_ranges;
pub use inlay_hints::build_inlay_hints;
pub use semantic_tokens::build_semantic_tokens;

pub use server::GrpctestifyLsp;
pub use server::start_lsp_server;
pub use variable_definition::{
    VariableLocation, find_variable_definition, find_variable_references, get_all_variables,
    variable_location_to_lsp,
};
