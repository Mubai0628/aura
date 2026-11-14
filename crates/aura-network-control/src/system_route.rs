//! A proxy running in TUN mode (mihomo, Clash, sing-box, Surge) answers each name it carries with
//! an address from 198.18.0.0/15, a block set aside for benchmarks, and takes the connection to
//! the name itself. Such an answer for a name that is not a local one is the system's route to
//! the public internet, so it is reached as a public address would be. A local name, or an
//! address written out in the URL, keeps the class of its address.
use std::net::IpAddr;

/// Name endings that never reach the public internet; their answers keep their own classes.
const LOCAL_SUFFIXES: [&str; 10] = [
    "local",
    "lan",
    "home",
    "internal",
    "intranet",
    "corp",
    "private",
    "localdomain",
    "localhost",
    "home.arpa",
];

/// The addresses to connect to when `answers` for `host` are a TUN proxy's; none otherwise.
#[must_use]
pub(crate) fn tun_answers(host: &str, answers: &[IpAddr]) -> Option<Vec<IpAddr>> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if !host.contains('.')
        || host.parse::<IpAddr>().is_ok()
        || LOCAL_SUFFIXES
            .iter()
            .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
    {
        return None;
    }
    let mut routed = Vec::new();
    for answer in answers {
        match answer {
            IpAddr::V4(address) if matches!(address.octets(), [198, 18 | 19, _, _]) => {
                routed.push(*answer);
            }
            // The same proxy answers IPv6 with a unique local address; its IPv4 one is used.
            IpAddr::V6(address) if (address.segments()[0] & 0xfe00) == 0xfc00 => {}
            _ => return None,
        }
    }
    (!routed.is_empty()).then_some(routed)
}

#[cfg(test)]
mod tests {
    use super::tun_answers;
    use std::net::IpAddr;

    fn ips(values: &[&str]) -> Vec<IpAddr> {
        values.iter().map(|value| value.parse().expect("IP")).collect()
    }

    #[test]
    fn a_tun_answer_for_a_public_name_is_the_systems_route() {
        assert_eq!(
            tun_answers("api.deepseek.com", &ips(&["198.18.3.32"])),
            Some(ips(&["198.18.3.32"]))
        );
        assert_eq!(
            tun_answers("OpenCode.ai.", &ips(&["198.19.255.1"])),
            Some(ips(&["198.19.255.1"]))
        );
        // The same proxy's unique local IPv6 answer is left out; its IPv4 one is used.
        assert_eq!(
            tun_answers("github.com", &ips(&["198.18.0.7", "fc00::7"])),
            Some(ips(&["198.18.0.7"]))
        );
    }

    #[test]
    fn a_local_name_keeps_its_class() {
        for host in [
            "nas.lan",
            "printer.local",
            "router",
            "intranet.corp",
            "x.home.arpa",
            "localhost",
            "db.internal",
            "box.home",
            "api.localhost",
        ] {
            assert_eq!(tun_answers(host, &ips(&["198.18.0.5"])), None, "{host}");
        }
    }

    #[test]
    fn other_or_mixed_answers_keep_their_class() {
        for answers in [
            ips(&["198.18.0.5", "8.8.8.8"]),
            ips(&["198.20.0.1"]),
            ips(&["10.0.0.5"]),
            ips(&["fc00::7"]),
            ips(&["198.18.0.5", "2001:4860::1"]),
            vec![],
        ] {
            assert_eq!(tun_answers("api.example.com", &answers), None, "{answers:?}");
        }
        assert_eq!(
            tun_answers("198.18.0.5", &ips(&["198.18.0.5"])),
            None,
            "an address written out"
        );
    }
}
