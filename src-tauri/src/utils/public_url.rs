pub fn is_public_https(url: &reqwest::Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');

    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(ip) => is_public_v4(ip),
            std::net::IpAddr::V6(ip) => {
                if let Some(mapped) = ip.to_ipv4_mapped() {
                    is_public_v4(mapped)
                } else {
                    !(ip.is_loopback() || ip.is_unspecified() || ip.is_unique_local() || ip.is_unicast_link_local())
                }
            }
        };
    }

    let name = host.trim_end_matches('.').to_ascii_lowercase();
    !(name == "localhost"
        || name.ends_with(".localhost")
        || name.ends_with(".local")
        || name.ends_with(".internal")
        || name.ends_with(".home.arpa"))
}

pub const fn is_public_v4(ip: std::net::Ipv4Addr) -> bool {
    !(ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast())
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::is_public_https;

    #[test]
    fn a_redirect_into_the_local_network_is_refused() {
        let allowed = [
            "https://cdn.example.com/logo.png",
            "https://1.2.3.4/logo.png",
            "https://[2606:4700::6810:84e5]/logo.png",
        ];
        for raw in allowed {
            let url = reqwest::Url::parse(raw).expect("test url");
            assert!(is_public_https(&url), "{raw}");
        }

        let refused = [
            "http://cdn.example.com/logo.png",
            "https://127.0.0.1:9090/configs",
            "https://localhost/logo.png",
            "https://192.168.1.1/logo.png",
            "https://10.0.0.5/logo.png",
            "https://169.254.169.254/latest/meta-data",
            "https://[::1]/logo.png",
            "https://[fd00::1]/logo.png",
            "https://[fe80::1]/logo.png",
            "https://[::ffff:127.0.0.1]/logo.png",
            "https://[::ffff:192.168.1.1]/logo.png",
            "https://router.local/logo.png",
        ];
        for raw in refused {
            let url = reqwest::Url::parse(raw).expect("test url");
            assert!(!is_public_https(&url), "{raw}");
        }
    }
}
