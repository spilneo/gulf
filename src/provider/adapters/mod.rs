pub mod chainflip;
pub mod changenow;
pub mod circle_cctp;
pub mod meson;
pub mod rhino;
pub mod squid;
pub mod thorchain;

pub(super) fn is_circle_cctp_provider(provider_id: &str) -> bool {
    matches!(
        provider_id,
        "circle-cctp-standard" | "circle-cctp-fast-v2" | "circle-cctp-sandbox-fees"
    )
}

pub(super) fn is_squid_provider(provider_id: &str) -> bool {
    provider_id == "squid-router"
}

pub(super) fn is_thorchain_provider(provider_id: &str) -> bool {
    provider_id == "thorchain"
}

pub(super) fn is_chainflip_broker_provider(provider_id: &str) -> bool {
    provider_id == "chainflip-broker"
}

pub(super) fn is_changenow_provider(provider_id: &str) -> bool {
    provider_id == "changenow"
}

pub(super) fn is_rhino_fi_provider(provider_id: &str) -> bool {
    provider_id == "rhino-fi"
}

pub(super) fn is_meson_provider(provider_id: &str) -> bool {
    provider_id == "meson"
}
