//! The failures the model service's start, refreshes and new-session binding answer with, each
//! `code: 中文句`. A failed start leaves the model capability unavailable and records the text in
//! Core's log; a refresh that fails leaves the settings page as it was.

pub(crate) const SESSION_CONFIG_UNREADABLE: &str = "model.session_config_unreadable: 读不到模型设置，新会话没有选好模型；请重试，仍然如此请到模型设置里检查。";
pub(crate) const SESSION_DEFAULT_UNREADABLE: &str = "model.session_default_unreadable: 读不到默认模型，新会话没有选好模型；请到模型设置里重新选一次默认模型。";
pub(crate) const SESSION_BINDING_FAILED: &str =
    "model.session_binding_failed: 没能给新会话选好模型；请到模型设置里检查默认模型后再试。";
pub(crate) const RESPONSE_CEILING_INVALID: &str = "model.response_ceiling_invalid: 模型回答的大小上限算出来是 0，模型功能没有启动；请重启 Aura，仍然如此请重新安装。";
pub(crate) const RESPONSE_CEILING_UNSUPPORTED: &str = "model.response_ceiling_unsupported: 模型回答的大小上限超出了这台电脑能处理的范围，模型功能没有启动；请重新安装 Aura。";
pub(crate) const TRANSPORT_STORE_UNAVAILABLE: &str = "model.transport_store_unavailable: 模型请求的记录打不开，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const CLEAN_ACCEPTANCE_FLAG_INVALID: &str = "model.clean_acceptance_flag_invalid: 环境变量 AURA_P05_CLEAN_ACCEPTANCE 只能是 1，模型功能没有启动；请改正或删掉这个变量后重启 Aura。";
pub(crate) const CLEAN_ACCEPTANCE_NAMESPACE: &str = "model.clean_acceptance_namespace: 环境变量 AURA_P05_CLEAN_ACCEPTANCE 只能用于开发版，模型功能没有启动；请删掉这个变量后重启 Aura。";
pub(crate) const CLEAN_ACCEPTANCE_INTEGRITY: &str = "model.clean_acceptance_integrity_failed: 开发用的干净验收要求数据从空白开始，这份数据不是，模型功能没有启动；请换一个空的数据文件夹，或删掉 AURA_P05_CLEAN_ACCEPTANCE 后重启 Aura。";
pub(crate) const CATALOG_UNAVAILABLE: &str = "model.catalog_unavailable: 服务商目录打不开，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const CONFIG_JOURNALS_UNREADABLE: &str = "model.config_journals_unreadable: 上次没改完的模型设置记录读不出来，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const INTERRUPTED_OPERATIONS_UNSETTLED: &str = "model.interrupted_operations_unsettled: 上次退出时没做完的模型操作没能收尾，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const CREDENTIAL_JOURNAL_UNRECOVERED: &str = "model.credential_journal_unrecovered: 上次没保存完的模型密钥没能收尾，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const INITIAL_SETTINGS_UNAVAILABLE: &str = "model.initial_settings_unavailable: 启动时没能整理出模型设置页的内容，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const INITIAL_CATALOG_STATUS_UNREADABLE: &str = "model.initial_catalog_status_unreadable: 启动时读不到服务商目录的状态，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const INITIAL_CONFIG_UNREADABLE: &str = "model.initial_config_unreadable: 启动时读不到模型设置，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const INITIAL_CATALOG_UNAVAILABLE: &str = "model.initial_catalog_unavailable: 启动时没能整理出服务商目录，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const INITIAL_PROJECTIONS_INCOMPLETE: &str = "model.initial_projections_incomplete: 启动时没能整理完模型设置页的内容，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const INITIAL_PROJECTIONS_UNSAVED: &str = "model.initial_projections_unsaved: 启动时没能存下模型设置页的内容，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const INITIAL_REFRESH_UNAVAILABLE: &str = "model.initial_refresh_unavailable: 启动时没能开始刷新服务商目录，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const INITIAL_REFRESH_STATUS_UNREADABLE: &str = "model.initial_refresh_status_unreadable: 启动时读不到服务商目录的刷新状态，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const INITIAL_REFRESH_CONFIG_UNREADABLE: &str = "model.initial_refresh_config_unreadable: 启动时为刷新服务商目录读模型设置没有成功，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const INITIAL_REFRESH_VIEW_UNAVAILABLE: &str = "model.initial_refresh_view_unavailable: 启动时没能整理出服务商目录的刷新状态，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const INITIAL_REFRESH_VIEW_UNPUBLISHED: &str = "model.initial_refresh_view_unpublished: 启动时没能把服务商目录的刷新状态送到设置页，模型功能暂时不能用；请重启 Aura，仍然如此请到设置里查看恢复选项。";
pub(crate) const SETTINGS_REFRESH_UNAVAILABLE: &str = "model.settings_refresh_unavailable: 没能整理出新的模型设置页内容，设置页这次没有更新；下次修改模型设置或重启 Aura 时会再更新。";
pub(crate) const SETTINGS_REFRESH_UNREADABLE: &str = "model.settings_refresh_unreadable: 读不到上一次的模型设置页内容，设置页这次没有更新；下次修改模型设置或重启 Aura 时会再更新。";
pub(crate) const SETTINGS_REFRESH_INCOMPLETE: &str = "model.settings_refresh_incomplete: 没能整理完模型设置页的内容，设置页这次没有更新；下次修改模型设置或重启 Aura 时会再更新。";
pub(crate) const SETTINGS_REFRESH_UNSAVED: &str = "model.settings_refresh_unsaved: 没能存下新的模型设置页内容，设置页这次没有更新；下次修改模型设置或重启 Aura 时会再更新。";
pub(crate) const SETTINGS_REFRESH_UNPUBLISHED: &str = "model.settings_refresh_unpublished: 新的模型设置页内容没能送到设置页，设置页这次没有更新；下次修改模型设置或重启 Aura 时会再更新。";
pub(crate) const CATALOG_REFRESH_UNAVAILABLE: &str = "model.catalog_refresh_unavailable: 没能开始定时刷新服务商目录，这次没有刷新；约一分钟后会再试。";
pub(crate) const CATALOG_REFRESH_STATUS_UNREADABLE: &str = "model.catalog_refresh_status_unreadable: 读不到服务商目录的刷新状态，这次没有刷新；约一分钟后会再试。";
pub(crate) const CATALOG_REFRESH_CONFIG_UNREADABLE: &str = "model.catalog_refresh_config_unreadable: 读不到模型设置，服务商目录这次没有刷新；约一分钟后会再试。";
pub(crate) const CATALOG_REFRESH_VIEW_UNAVAILABLE: &str = "model.catalog_refresh_view_unavailable: 没能整理出服务商目录的刷新状态，这次没有刷新；约一分钟后会再试。";
pub(crate) const CATALOG_REFRESH_VIEW_UNSAVED: &str = "model.catalog_refresh_view_unsaved: 没能存下服务商目录的刷新状态，这次没有刷新；约一分钟后会再试。";
pub(crate) const CATALOG_REFRESH_VIEW_UNPUBLISHED: &str = "model.catalog_refresh_view_unpublished: 服务商目录的刷新状态没能送到设置页，这次没有刷新；约一分钟后会再试。";
