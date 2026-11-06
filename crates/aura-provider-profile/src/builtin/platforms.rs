//! Model platforms with a China site and an international one, and the coding plans they sell. A
//! plan's key draws on the plan at the addresses the plan's documentation gives; where those
//! are the platform's own, the plan shares the platform's profile. Discovery is off where a
//! platform documents no models list.
use aura_model_domain::{ChatOutputTokenField, ModelDiscoveryMode};

use super::shape::{alternatives, chat, messages, recommended, responses};
use crate::ProviderProfile;

/// Zhipu AI's platform (bigmodel.cn).
pub(super) fn zhipu() -> ProviderProfile {
    alternatives(
        "zhipuai",
        &["zhipuai"],
        vec![
            recommended(chat(
                "Zhipu AI",
                "https://open.bigmodel.cn/api/paas/v4",
                ModelDiscoveryMode::Disabled,
                ChatOutputTokenField::MaxTokens,
            )),
            messages("Zhipu AI", "https://open.bigmodel.cn/api/anthropic/v1"),
        ],
    )
}

/// Z.AI, Zhipu AI's international platform.
pub(super) fn zai() -> ProviderProfile {
    alternatives(
        "zai",
        &["zai"],
        vec![recommended(chat(
            "Z.AI",
            "https://api.z.ai/api/paas/v4",
            ModelDiscoveryMode::Disabled,
            ChatOutputTokenField::MaxTokens,
        ))],
    )
}

/// The GLM Coding Plan at `origin`, Zhipu AI's or Z.AI's: Chat Completions and Responses at
/// addresses of its own, and Messages at the platform's.
pub(super) fn glm_coding_plan(
    catalog_provider_id: &str,
    provider: &str,
    origin: &str,
) -> ProviderProfile {
    alternatives(
        catalog_provider_id,
        &[catalog_provider_id],
        vec![
            recommended(chat(
                provider,
                &format!("{origin}/api/coding/paas/v4"),
                ModelDiscoveryMode::Disabled,
                ChatOutputTokenField::MaxTokens,
            )),
            messages(provider, &format!("{origin}/api/anthropic/v1")),
            responses(provider, &format!("{origin}/api/v1"), ModelDiscoveryMode::Disabled),
        ],
    )
}

/// Alibaba Cloud Model Studio at `origin`: either site, or the Coding Plan of either, which has
/// a host of its own. Chat Completions is under `chat_path` there, Messages under
/// `/apps/anthropic`.
pub(super) fn alibaba(
    catalog_provider_id: &str,
    provider: &str,
    origin: &str,
    chat_path: &str,
) -> ProviderProfile {
    alternatives(
        catalog_provider_id,
        &[catalog_provider_id],
        vec![
            recommended(chat(
                provider,
                &format!("{origin}{chat_path}"),
                ModelDiscoveryMode::Disabled,
                ChatOutputTokenField::MaxTokens,
            )),
            messages(provider, &format!("{origin}/apps/anthropic/v1")),
        ],
    )
}

/// MiniMax at `origin`, its international site or its China one. A Token Plan's key goes to the
/// same addresses, so each site's plan shares its profile. The catalog's models go by Messages.
pub(super) fn minimax(
    profile_id: &str,
    catalog_provider_ids: &[&str],
    provider: &str,
    origin: &str,
) -> ProviderProfile {
    alternatives(
        profile_id,
        catalog_provider_ids,
        vec![
            recommended(messages(provider, &format!("{origin}/anthropic/v1"))),
            chat(
                provider,
                &format!("{origin}/v1"),
                ModelDiscoveryMode::Disabled,
                ChatOutputTokenField::MaxTokens,
            ),
        ],
    )
}

/// SiliconFlow's international site or its China one.
pub(super) fn siliconflow(
    catalog_provider_id: &str,
    provider: &str,
    base_url: &str,
) -> ProviderProfile {
    alternatives(
        catalog_provider_id,
        &[catalog_provider_id],
        vec![recommended(chat(
            provider,
            base_url,
            ModelDiscoveryMode::Automatic,
            ChatOutputTokenField::MaxTokens,
        ))],
    )
}

pub(super) fn volcengine() -> ProviderProfile {
    alternatives(
        "volcengine",
        &["volcengine"],
        vec![recommended(chat(
            "Volcengine Ark",
            "https://ark.cn-beijing.volces.com/api/v3",
            ModelDiscoveryMode::Disabled,
            ChatOutputTokenField::MaxTokens,
        ))],
    )
}

/// Volcengine Ark's Coding Plan, which draws on the plan only under `/api/coding`.
pub(super) fn volcengine_coding_plan() -> ProviderProfile {
    let provider = "Volcengine Ark Coding Plan";
    alternatives(
        "volcengine-coding-plan",
        &["volcengine-coding-plan"],
        vec![
            recommended(chat(
                provider,
                "https://ark.cn-beijing.volces.com/api/coding/v3",
                ModelDiscoveryMode::Disabled,
                ChatOutputTokenField::MaxTokens,
            )),
            messages(provider, "https://ark.cn-beijing.volces.com/api/coding/v1"),
        ],
    )
}
