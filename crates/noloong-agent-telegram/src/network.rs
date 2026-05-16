use futures_util::{StreamExt, stream::FuturesUnordered};
use reqwest::{Client, ClientBuilder, Proxy};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};
use thiserror::Error;

const TELEGRAM_API_HOST: &str = "api.telegram.org";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TelegramNetworkConfig {
    pub api_base_url: Option<String>,
    pub proxy_url: Option<String>,
    pub resolved_addrs: Vec<SocketAddr>,
    #[serde(default)]
    pub fallback_ips: Vec<String>,
    #[serde(default)]
    pub disable_fallback_ips: bool,
    #[serde(default)]
    pub disable_env_proxy: bool,
    #[serde(default)]
    pub doh_endpoints: Vec<String>,
    #[serde(default = "default_seed_fallback_ips")]
    pub seed_fallback_ips: Vec<String>,
    pub timeout: Option<Duration>,
}

#[derive(Debug, Error)]
pub enum TelegramNetworkError {
    #[error("telegram fallback IP is not public IPv4: {0}")]
    InvalidFallbackIp(String),
    #[error("telegram proxy config failed: {0}")]
    Proxy(String),
    #[error("telegram http client config failed: {0}")]
    Client(String),
    #[error("telegram DoH lookup failed: {0}")]
    Doh(String),
}

pub fn build_telegram_http_client(
    config: &TelegramNetworkConfig,
) -> Result<Client, TelegramNetworkError> {
    let mut builder = ClientBuilder::new();
    if let Some(timeout) = config.timeout {
        builder = builder.timeout(timeout);
    }
    if let Some(proxy_url) = &config.proxy_url {
        builder = builder.proxy(
            Proxy::all(proxy_url)
                .map_err(|error| TelegramNetworkError::Proxy(error.to_string()))?,
        );
    } else if !use_env_proxy(config) {
        builder = builder.no_proxy();
        if !config.resolved_addrs.is_empty() {
            builder = builder.resolve_to_addrs(TELEGRAM_API_HOST, &config.resolved_addrs);
        }
    }
    builder
        .build()
        .map_err(|error| TelegramNetworkError::Client(error.to_string()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelegramNetworkResolutionMode {
    Proxy,
    EnvProxy,
    StaticResolve,
    SystemDns,
}

pub fn network_resolution_mode(config: &TelegramNetworkConfig) -> TelegramNetworkResolutionMode {
    if config.proxy_url.is_some() {
        TelegramNetworkResolutionMode::Proxy
    } else if use_env_proxy(config) {
        TelegramNetworkResolutionMode::EnvProxy
    } else if !config.resolved_addrs.is_empty() {
        TelegramNetworkResolutionMode::StaticResolve
    } else {
        TelegramNetworkResolutionMode::SystemDns
    }
}

pub async fn discover_fallback_addrs(
    config: &TelegramNetworkConfig,
    client: &Client,
) -> Result<Vec<SocketAddr>, TelegramNetworkError> {
    if config.proxy_url.is_some() || use_env_proxy(config) || config.disable_fallback_ips {
        return Ok(Vec::new());
    }

    let mut ips = Vec::new();
    let mut seen = BTreeSet::new();
    for raw in config
        .fallback_ips
        .iter()
        .chain(config.seed_fallback_ips.iter())
    {
        push_unique_ip(&mut ips, &mut seen, parse_public_ipv4(raw)?);
    }
    let mut doh_results = query_doh_endpoints(config, client).await?;
    for index in 0..config.doh_endpoints.len() {
        let Some(endpoint_ips) = doh_results.remove(&index) else {
            continue;
        };
        for ip in endpoint_ips {
            push_unique_ip(&mut ips, &mut seen, ip);
        }
    }
    Ok(ips
        .into_iter()
        .map(|ip| SocketAddr::new(IpAddr::V4(ip), 443))
        .collect())
}

fn push_unique_ip(ips: &mut Vec<Ipv4Addr>, seen: &mut BTreeSet<Ipv4Addr>, ip: Ipv4Addr) {
    if seen.insert(ip) {
        ips.push(ip);
    }
}

pub fn parse_fallback_ips(values: &[String]) -> Result<Vec<Ipv4Addr>, TelegramNetworkError> {
    values
        .iter()
        .map(|value| parse_public_ipv4(value))
        .collect()
}

fn parse_public_ipv4(value: &str) -> Result<Ipv4Addr, TelegramNetworkError> {
    let ip = value
        .trim()
        .parse::<Ipv4Addr>()
        .map_err(|_| TelegramNetworkError::InvalidFallbackIp(value.into()))?;
    if is_public_ipv4(ip) {
        Ok(ip)
    } else {
        Err(TelegramNetworkError::InvalidFallbackIp(value.into()))
    }
}

async fn query_doh_endpoint(
    client: &Client,
    endpoint: &str,
) -> Result<Vec<Ipv4Addr>, TelegramNetworkError> {
    let separator = if endpoint.contains('?') { '&' } else { '?' };
    let url = format!("{endpoint}{separator}name={TELEGRAM_API_HOST}&type=A");
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| TelegramNetworkError::Doh(error.to_string()))?;
    let body = response
        .json::<DohResponse>()
        .await
        .map_err(|error| TelegramNetworkError::Doh(error.to_string()))?;
    Ok(body
        .answer
        .unwrap_or_default()
        .into_iter()
        .filter(|answer| answer.record_type == 1)
        .filter_map(|answer| parse_public_ipv4(&answer.data).ok())
        .collect())
}

async fn query_doh_endpoints(
    config: &TelegramNetworkConfig,
    client: &Client,
) -> Result<BTreeMap<usize, Vec<Ipv4Addr>>, TelegramNetworkError> {
    let mut queries = FuturesUnordered::new();
    for (index, endpoint) in config.doh_endpoints.iter().enumerate() {
        queries.push(async move { (index, query_doh_endpoint(client, endpoint).await) });
    }

    let mut results = BTreeMap::new();
    while let Some((index, result)) = queries.next().await {
        results.insert(index, result?);
    }
    Ok(results)
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast())
}

fn default_seed_fallback_ips() -> Vec<String> {
    Vec::new()
}

fn use_env_proxy(config: &TelegramNetworkConfig) -> bool {
    !config.disable_env_proxy && ambient_proxy_configured()
}

fn ambient_proxy_configured() -> bool {
    [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ]
    .into_iter()
    .any(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()))
}

#[derive(Clone, Debug, Deserialize)]
struct DohResponse {
    #[serde(default, rename = "Answer")]
    answer: Option<Vec<DohAnswer>>,
}

#[derive(Clone, Debug, Deserialize)]
struct DohAnswer {
    #[serde(rename = "type")]
    record_type: u16,
    data: String,
}

#[cfg(test)]
mod tests {
    use super::{
        TelegramNetworkConfig, TelegramNetworkResolutionMode, discover_fallback_addrs,
        network_resolution_mode, parse_fallback_ips,
    };
    use axum::{Json, Router, routing::get};
    use serde_json::json;
    use tokio::net::TcpListener;

    #[test]
    fn network_rejects_internal_fallback_ips() {
        let values = vec![
            "127.0.0.1".into(),
            "10.0.0.1".into(),
            "169.254.1.1".into(),
            "0.0.0.0".into(),
        ];

        assert!(parse_fallback_ips(&values).is_err());
    }

    #[test]
    fn network_uses_proxy_without_dns_override() {
        let config = TelegramNetworkConfig {
            proxy_url: Some("socks5h://127.0.0.1:1080".into()),
            resolved_addrs: vec!["149.154.167.220:443".parse().unwrap()],
            ..Default::default()
        };

        assert_eq!(
            network_resolution_mode(&config),
            TelegramNetworkResolutionMode::Proxy
        );
    }

    #[test]
    fn network_defaults_to_system_dns_without_static_fallback() {
        let config = TelegramNetworkConfig::default();

        assert_eq!(
            network_resolution_mode(&config),
            TelegramNetworkResolutionMode::SystemDns
        );
        assert!(config.seed_fallback_ips.is_empty());
    }

    #[tokio::test]
    async fn network_discovers_doh_fallback_ips() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/resolve",
                    get(|| async {
                        Json(json!({
                            "Answer": [
                                {"type": 1, "data": "149.154.167.220"},
                                {"type": 1, "data": "10.0.0.1"},
                                {"type": 28, "data": "2001:db8::1"}
                            ]
                        }))
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let config = TelegramNetworkConfig {
            doh_endpoints: vec![format!("http://{address}/resolve")],
            seed_fallback_ips: Vec::new(),
            disable_env_proxy: true,
            ..Default::default()
        };

        let addrs = discover_fallback_addrs(&config, &reqwest::Client::new())
            .await
            .unwrap();

        task.abort();
        assert_eq!(addrs, vec!["149.154.167.220:443".parse().unwrap()]);
    }

    #[tokio::test]
    async fn network_default_discovers_no_fallback_addrs() {
        let config = TelegramNetworkConfig {
            disable_env_proxy: true,
            ..Default::default()
        };

        let addrs = discover_fallback_addrs(&config, &reqwest::Client::new())
            .await
            .unwrap();

        assert!(addrs.is_empty());
    }

    #[tokio::test]
    async fn network_preserves_fallback_ip_order() {
        let config = TelegramNetworkConfig {
            fallback_ips: vec!["149.154.167.220".into()],
            seed_fallback_ips: vec!["91.108.56.130".into(), "149.154.167.220".into()],
            disable_env_proxy: true,
            ..Default::default()
        };

        let addrs = discover_fallback_addrs(&config, &reqwest::Client::new())
            .await
            .unwrap();

        assert_eq!(
            addrs,
            vec![
                "149.154.167.220:443".parse().unwrap(),
                "91.108.56.130:443".parse().unwrap(),
            ]
        );
    }
}
