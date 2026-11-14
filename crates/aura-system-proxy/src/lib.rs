//! The proxy Windows names for the current user, which model requests and tools follow while no
//! proxy is switched on in Aura's own settings. Only a proxy on this computer is used, the same
//! rule as a proxy typed into Aura's settings. The system's values are only read, never changed.
mod parse;

use aura_tool_contract::{SystemNetworkProxyV1, ToolNetworkProxyProtocolV1, ToolNetworkProxyV1};
pub use aura_windows_platform::InternetSettingsValues;
pub use parse::from_values;

/// Where the system's values come from: the registry in production, fixed values in tests.
pub trait SystemProxySource: Send + Sync {
    fn read(&self) -> std::io::Result<InternetSettingsValues>;
}

/// The current user's Internet Settings in the registry.
#[derive(Clone, Copy, Debug, Default)]
pub struct CurrentUserProxy;

impl SystemProxySource for CurrentUserProxy {
    fn read(&self) -> std::io::Result<InternetSettingsValues> {
        aura_windows_platform::current_user_internet_settings()
    }
}

/// Values given up front, or a read that fails with `Err`'s text.
#[derive(Clone, Debug)]
pub struct FixedSystemProxy(pub Result<InternetSettingsValues, String>);

impl SystemProxySource for FixedSystemProxy {
    fn read(&self) -> std::io::Result<InternetSettingsValues> {
        self.0.clone().map_err(std::io::Error::other)
    }
}

/// What the system's values mean for a tool's https request to the public internet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SystemProxyLookup {
    /// A proxy on this computer that tools use.
    Found(ToolNetworkProxyV1),
    /// Neither a proxy nor a configuration script is set.
    NotSet,
    /// Only an automatic configuration script (PAC) is set, and Aura does not run scripts.
    AutoConfig,
    /// The proxy is not on this computer.
    NotLocal { server: String },
    /// The proxy text names no proxy for https traffic, or none Aura can speak.
    Unusable { server: String },
    /// The values could not be read.
    Unreadable { reason: String },
}

impl SystemProxyLookup {
    #[must_use]
    pub fn proxy(&self) -> Option<&ToolNetworkProxyV1> {
        match self {
            Self::Found(proxy) => Some(proxy),
            _ => None,
        }
    }

    /// Why a proxy Windows names is not followed, and what to do instead; none when a proxy is
    /// found or none is set, where requests leave the way the system's own do.
    #[must_use]
    pub fn issue(&self) -> Option<String> {
        Some(match self {
            Self::Found(_) | Self::NotSet => return None,
            Self::AutoConfig => "Windows 用的是自动配置脚本（PAC），Aura 读不了这种设置，不经代理直连。可以启用网络代理，填写代理软件的本机端口。".to_owned(),
            Self::NotLocal { server } => format!(
                "Windows 的代理 {server} 不在这台电脑上，Aura 只经本机代理，不经代理直连。可以启用网络代理，填写本机代理端口。"
            ),
            Self::Unusable { server } => format!(
                "Windows 的代理设置“{server}”里没有 Aura 能用的 HTTPS 或 SOCKS 代理，不经代理直连。可以启用网络代理，填写代理软件的本机端口。"
            ),
            Self::Unreadable { reason } => {
                format!(
                    "读不到 Windows 的代理设置（{reason}），不经代理直连。可以启用网络代理，填写代理软件的本机端口。"
                )
            }
        })
    }

    #[must_use]
    pub fn view(&self) -> SystemNetworkProxyV1 {
        SystemNetworkProxyV1 { proxy: self.proxy().cloned(), issue: self.issue() }
    }
}

/// Reads `source` and says what its values mean.
#[must_use]
pub fn lookup(source: &dyn SystemProxySource) -> SystemProxyLookup {
    match source.read() {
        Ok(values) => from_values(&values),
        Err(error) => SystemProxyLookup::Unreadable { reason: error.to_string() },
    }
}

/// The protocol word a proxy is shown with.
#[must_use]
pub const fn protocol_label(protocol: ToolNetworkProxyProtocolV1) -> &'static str {
    match protocol {
        ToolNetworkProxyProtocolV1::Http => "HTTP",
        ToolNetworkProxyProtocolV1::Socks5 => "SOCKS5",
    }
}
