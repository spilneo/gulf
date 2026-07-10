use crate::{config::ProviderConfig, model::QuoteRequest};

pub(super) fn supports_route_by_name(provider: &ProviderConfig, request: &QuoteRequest) -> bool {
    if provider.supported_chains.is_empty() {
        return true;
    }

    let from = request.from_chain.to_ascii_lowercase();
    let to = request.to_chain.to_ascii_lowercase();
    provider
        .supported_chains
        .iter()
        .any(|chain| chain.eq_ignore_ascii_case(&from))
        && provider
            .supported_chains
            .iter()
            .any(|chain| chain.eq_ignore_ascii_case(&to))
}
