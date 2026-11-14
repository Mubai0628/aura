//! A provider's failure of a model step in words: in English for the stop note the model reads of
//! a turn the provider ended, in Chinese for a sub-agent's report and progress, which the user
//! reads too.
use aura_model_domain::{ModelFailureCategory, NormalizedModelFailure};

/// What the provider did, as a clause.
#[must_use]
pub fn cause(failure: &NormalizedModelFailure) -> &'static str {
    match failure.code.as_str() {
        "model_transport_streamidle_timeout" => {
            "it sent nothing for too long in the middle of an answer"
        }
        "model_transport_firstevent_timeout" => "it did not begin to answer in time",
        "model_transport_total_timeout" => "the answer took longer than the connection allows",
        "model_transport_connect_timeout" => "it could not be reached in time",
        "model_reasoning_loop" => "its reasoning kept repeating itself",
        _ => match failure.category {
            ModelFailureCategory::Authentication => "it did not accept the connection's key",
            ModelFailureCategory::Authorization => {
                "the connection's key has no access to this model"
            }
            ModelFailureCategory::RateLimit => "its rate limit was reached",
            ModelFailureCategory::QuotaOrBalance => "the account's quota or balance ran out",
            ModelFailureCategory::InvalidRequest
            | ModelFailureCategory::UnsupportedParameter
            | ModelFailureCategory::UnsupportedModel => "it refused the request as invalid",
            ModelFailureCategory::ContextWindowExceeded => {
                "the request was larger than the model's context window"
            }
            ModelFailureCategory::Transport => "the connection to it failed",
            ModelFailureCategory::StreamInterrupted => "its answer broke off",
            ModelFailureCategory::Protocol => "its answer could not be read",
            ModelFailureCategory::ContentFilter | ModelFailureCategory::Refusal => {
                "it refused to answer"
            }
            ModelFailureCategory::EmptyOutput => "it answered with nothing",
            ModelFailureCategory::InvalidToolCall => {
                "its answer held a tool call that could not be read"
            }
            ModelFailureCategory::Persistence => "Aura could not record its answer",
            ModelFailureCategory::Cancelled => "the request was cancelled",
            ModelFailureCategory::Unknown => "it failed without saying why",
        },
    }
}

/// What the provider did, as a Chinese clause with its subject.
#[must_use]
pub fn cause_zh(failure: &NormalizedModelFailure) -> &'static str {
    match failure.code.as_str() {
        "model_transport_streamidle_timeout" => "模型服务回答到一半时太久没有再发来内容",
        "model_transport_firstevent_timeout" => "模型服务没有按时开始回答",
        "model_transport_total_timeout" => "模型服务的回答超过了连接允许的总时长",
        "model_transport_connect_timeout" => "没能按时连上模型服务",
        "model_reasoning_loop" => "模型的推理一直在重复",
        _ => match failure.category {
            ModelFailureCategory::Authentication => "模型服务没有接受连接的密钥",
            ModelFailureCategory::Authorization => "连接的密钥没有这个模型的权限",
            ModelFailureCategory::RateLimit => "模型服务的速率限制到了",
            ModelFailureCategory::QuotaOrBalance => "模型服务账户的额度或余额用完了",
            ModelFailureCategory::InvalidRequest
            | ModelFailureCategory::UnsupportedParameter
            | ModelFailureCategory::UnsupportedModel => "模型服务认为请求无效，拒绝了",
            ModelFailureCategory::ContextWindowExceeded => "请求超过了模型的上下文窗口",
            ModelFailureCategory::Transport => "到模型服务的连接失败了",
            ModelFailureCategory::StreamInterrupted => "模型服务的回答中途断了",
            ModelFailureCategory::Protocol => "模型服务的回答无法解读",
            ModelFailureCategory::ContentFilter | ModelFailureCategory::Refusal => {
                "模型服务拒绝回答"
            }
            ModelFailureCategory::EmptyOutput => "模型服务回答的是空内容",
            ModelFailureCategory::InvalidToolCall => "模型回答里的工具调用无法解读",
            ModelFailureCategory::Persistence => "Aura 没能记下模型的回答",
            ModelFailureCategory::Cancelled => "请求被取消了",
            ModelFailureCategory::Unknown => "模型服务失败了，没有说明原因",
        },
    }
}
