//! What tools make of the proxy Windows names, from injected values only: the machine's own
//! settings are never read here.
use aura_system_proxy::{
    FixedSystemProxy, InternetSettingsValues, SystemProxyLookup, from_values, lookup,
};
use aura_tool_contract::{ToolNetworkProxyProtocolV1 as Protocol, ToolNetworkProxyV1};

fn manual(server: &str) -> InternetSettingsValues {
    InternetSettingsValues {
        proxy_enable: Some(1),
        proxy_server: Some(server.to_owned()),
        auto_config_url: None,
    }
}

fn found(protocol: Protocol, host: &str, port: u16) -> SystemProxyLookup {
    SystemProxyLookup::Found(
        ToolNetworkProxyV1::new(true, protocol, host.to_owned(), port).expect("a loopback proxy"),
    )
}

#[test]
fn one_local_address_for_every_scheme_is_an_http_proxy() {
    assert_eq!(from_values(&manual("127.0.0.1:10809")), found(Protocol::Http, "127.0.0.1", 10809));
    assert_eq!(from_values(&manual(" localhost:7890 ")), found(Protocol::Http, "localhost", 7890));
    assert_eq!(from_values(&manual("[::1]:8080")), found(Protocol::Http, "::1", 8080));
    assert_eq!(
        from_values(&manual("http://127.0.0.1:7890")),
        found(Protocol::Http, "127.0.0.1", 7890),
        "a scheme in front of the address names the proxy's protocol"
    );
}

#[test]
fn https_traffic_takes_the_https_entry_then_socks() {
    assert_eq!(
        from_values(&manual("http=127.0.0.1:7890;https=127.0.0.1:7891;ftp=127.0.0.1:21")),
        found(Protocol::Http, "127.0.0.1", 7891)
    );
    assert_eq!(
        from_values(&manual("http=127.0.0.1:7890;socks=127.0.0.1:10808")),
        found(Protocol::Socks5, "127.0.0.1", 10808),
        "socks covers the schemes without an entry of their own"
    );
    assert_eq!(
        from_values(&manual("socks5://127.0.0.1:1080")),
        found(Protocol::Socks5, "127.0.0.1", 1080)
    );
    assert_eq!(
        from_values(&manual("HTTPS=127.0.0.1:7891")),
        found(Protocol::Http, "127.0.0.1", 7891),
        "entry names ignore case"
    );
}

#[test]
fn a_proxy_elsewhere_or_for_other_schemes_only_is_not_used() {
    assert_eq!(
        from_values(&manual("proxy.corp.example:8080")),
        SystemProxyLookup::NotLocal { server: "proxy.corp.example:8080".into() }
    );
    assert_eq!(
        from_values(&manual("https=192.168.1.2:3128")),
        SystemProxyLookup::NotLocal { server: "192.168.1.2:3128".into() }
    );
    assert_eq!(
        from_values(&manual("http=127.0.0.1:7890;ftp=127.0.0.1:21")),
        SystemProxyLookup::Unusable { server: "http=127.0.0.1:7890;ftp=127.0.0.1:21".into() }
    );
    for text in ["127.0.0.1:0", "127.0.0.1:99999", "127.0.0.1:port", "socks4://127.0.0.1:1080"] {
        assert!(
            matches!(from_values(&manual(text)), SystemProxyLookup::Unusable { .. }),
            "{text} names no proxy Aura can use"
        );
    }
}

#[test]
fn a_switched_off_proxy_or_a_script_leaves_tools_direct() {
    let off = InternetSettingsValues {
        proxy_enable: Some(0),
        proxy_server: Some("127.0.0.1:10809".into()),
        auto_config_url: None,
    };
    assert_eq!(from_values(&off), SystemProxyLookup::NotSet);
    assert_eq!(from_values(&InternetSettingsValues::default()), SystemProxyLookup::NotSet);
    let script = InternetSettingsValues {
        proxy_enable: Some(0),
        proxy_server: None,
        auto_config_url: Some("http://127.0.0.1:1080/pac".into()),
    };
    assert_eq!(from_values(&script), SystemProxyLookup::AutoConfig);
    let both = InternetSettingsValues { proxy_enable: Some(1), ..manual("127.0.0.1:10809") };
    assert_eq!(
        from_values(&InternetSettingsValues { auto_config_url: script.auto_config_url, ..both }),
        found(Protocol::Http, "127.0.0.1", 10809),
        "a manual proxy that is switched on is used beside a script"
    );
    let empty = manual("   ");
    assert_eq!(from_values(&empty), SystemProxyLookup::NotSet);
}

#[test]
fn every_way_of_going_direct_says_why_in_plain_words() {
    let unread = lookup(&FixedSystemProxy(Err("access denied".into())));
    assert_eq!(unread, SystemProxyLookup::Unreadable { reason: "access denied".into() });
    for (lookup, words) in [
        (SystemProxyLookup::AutoConfig, "自动配置脚本"),
        (
            SystemProxyLookup::NotLocal { server: "proxy.corp.example:8080".into() },
            "不在这台电脑上",
        ),
        (SystemProxyLookup::Unusable { server: "ftp=127.0.0.1:21".into() }, "HTTPS 或 SOCKS"),
        (unread, "读不到"),
    ] {
        let issue = lookup.issue().expect("an issue");
        // The way out is the switch beside it; there is no separate choice to follow the system.
        assert!(
            issue.contains(words) && issue.contains("直连") && issue.contains("启用网络代理"),
            "{issue}"
        );
        assert!(!issue.contains("跟随系统代理"), "{issue}");
        assert_eq!(lookup.view().proxy, None);
    }
    // No proxy set is nothing to fix: requests leave the way the system's own do.
    assert_eq!(SystemProxyLookup::NotSet.issue(), None);
    assert_eq!(SystemProxyLookup::NotSet.view().proxy, None);
    let used = found(Protocol::Http, "127.0.0.1", 10809);
    assert_eq!(used.issue(), None);
    assert_eq!(used.view().proxy.map(|proxy| proxy.port), Some(10809));
    assert_eq!(
        lookup(&FixedSystemProxy(Ok(manual("127.0.0.1:10809")))),
        used,
        "a source's values are read through"
    );
}
