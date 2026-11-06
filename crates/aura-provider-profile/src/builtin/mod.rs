//! The profiles compiled into Aura.
use std::sync::LazyLock;

use crate::ProviderProfile;

mod gateways;
mod hosts;
mod local;
mod model_makers;
mod platforms;
mod shape;

static BUILTIN: LazyLock<Vec<ProviderProfile>> = LazyLock::new(|| {
    vec![
        model_makers::openai(),
        model_makers::anthropic(),
        model_makers::deepseek(),
        model_makers::moonshot(),
        model_makers::kimi_for_coding(
            "kimi-for-coding-cn",
            "kimi-code-plan-cn",
            "https://api.kimi.com/coding/v1",
        ),
        model_makers::kimi_for_coding(
            "kimi-for-coding-global",
            "kimi-code-plan-global",
            "https://api.kimi.ai/coding/v1",
        ),
        gateways::opencode("opencode", "opencode", "OpenCode", "https://opencode.ai/inference"),
        gateways::opencode(
            "opencode-go",
            "opencode-go",
            "OpenCode Go",
            "https://opencode.ai/inference/go",
        ),
        hosts::bedrock(),
        local::local_responses_gateway(),
        model_makers::moonshot_cn(),
        platforms::zhipu(),
        platforms::glm_coding_plan(
            "zhipuai-coding-plan",
            "Zhipu AI Coding Plan",
            "https://open.bigmodel.cn",
        ),
        platforms::zai(),
        platforms::glm_coding_plan("zai-coding-plan", "Z.AI Coding Plan", "https://api.z.ai"),
        platforms::alibaba(
            "alibaba",
            "Alibaba",
            "https://dashscope-intl.aliyuncs.com",
            "/compatible-mode/v1",
        ),
        platforms::alibaba(
            "alibaba-cn",
            "Alibaba (China)",
            "https://dashscope.aliyuncs.com",
            "/compatible-mode/v1",
        ),
        platforms::alibaba(
            "alibaba-coding-plan",
            "Alibaba Coding Plan",
            "https://coding-intl.dashscope.aliyuncs.com",
            "/v1",
        ),
        platforms::alibaba(
            "alibaba-coding-plan-cn",
            "Alibaba Coding Plan (China)",
            "https://coding.dashscope.aliyuncs.com",
            "/v1",
        ),
        platforms::minimax(
            "minimax",
            &["minimax", "minimax-coding-plan"],
            "MiniMax",
            "https://api.minimax.io",
        ),
        platforms::minimax(
            "minimax-cn",
            &["minimax-cn", "minimax-cn-coding-plan"],
            "MiniMax (China)",
            "https://api.minimax.cn",
        ),
        platforms::siliconflow("siliconflow", "SiliconFlow", "https://api.siliconflow.com/v1"),
        platforms::siliconflow(
            "siliconflow-cn",
            "SiliconFlow (China)",
            "https://api.siliconflow.cn/v1",
        ),
        platforms::volcengine(),
        platforms::volcengine_coding_plan(),
        model_makers::google(),
        model_makers::xai(),
        model_makers::mistral(),
        hosts::groq(),
        hosts::fireworks(),
        hosts::together(),
        gateways::openrouter(),
        gateways::vercel(),
        local::lm_studio(),
    ]
});

/// The profiles compiled into Aura. A catalog Provider's routes are offered in this order.
#[must_use]
pub fn builtin_profiles() -> &'static [ProviderProfile] {
    &BUILTIN
}
