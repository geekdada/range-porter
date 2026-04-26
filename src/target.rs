use anyhow::{Context, Result, anyhow, bail};
use hickory_resolver::TokioResolver;
use hickory_resolver::config::{NameServerConfigGroup, ResolverConfig};
use hickory_resolver::name_server::TokioConnectionProvider;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

const DNS_POSITIVE_MIN_TTL: Duration = Duration::from_secs(5);
const DNS_POSITIVE_MAX_TTL: Duration = Duration::from_secs(600);
const DNS_FAILURE_REFRESH: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum TargetAddr {
    Static(StaticTarget),
    Dynamic(Arc<DynamicTarget>),
}

#[derive(Debug, Clone)]
pub struct StaticTarget {
    raw: String,
    addr: SocketAddr,
}

#[derive(Debug)]
pub struct DynamicTarget {
    raw: String,
    host: String,
    port: u16,
    resolver: TokioResolver,
    current: RwLock<Arc<Resolved>>,
    refresh_inflight: AtomicBool,
}

#[derive(Debug, Clone, Copy)]
struct Resolved {
    addr: SocketAddr,
    valid_until: Instant,
}

impl TargetAddr {
    pub async fn bind(raw: &str, dns_server: Option<SocketAddr>) -> Result<Self> {
        let (host, port) = split_host_port(raw)?;

        if let Ok(addr) = raw.parse::<SocketAddr>() {
            return Ok(TargetAddr::Static(StaticTarget {
                raw: raw.to_string(),
                addr,
            }));
        }

        let resolver = build_resolver(dns_server)?;
        let resolved = resolve_once(&resolver, &host, port)
            .await
            .with_context(|| format!("failed to resolve target host `{host}`"))?;

        Ok(TargetAddr::Dynamic(Arc::new(DynamicTarget {
            raw: raw.to_string(),
            host,
            port,
            resolver,
            current: RwLock::new(Arc::new(resolved)),
            refresh_inflight: AtomicBool::new(false),
        })))
    }

    pub fn current(&self) -> SocketAddr {
        match self {
            TargetAddr::Static(s) => s.addr,
            TargetAddr::Dynamic(d) => d.current(),
        }
    }

    pub fn display(&self) -> &str {
        match self {
            TargetAddr::Static(s) => &s.raw,
            TargetAddr::Dynamic(d) => &d.raw,
        }
    }
}

impl DynamicTarget {
    fn current(self: &Arc<Self>) -> SocketAddr {
        let resolved = {
            let guard = self.current.read().expect("target cache lock poisoned");
            *guard.as_ref()
        };

        if Instant::now() >= resolved.valid_until {
            self.try_spawn_refresh();
        }

        resolved.addr
    }

    fn try_spawn_refresh(self: &Arc<Self>) {
        if self
            .refresh_inflight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let target = Arc::clone(self);
        tokio::spawn(async move {
            let outcome = resolve_once(&target.resolver, &target.host, target.port).await;
            let next = match outcome {
                Ok(resolved) => {
                    debug!(
                        host = %target.host,
                        addr = %resolved.addr,
                        ttl_secs = resolved.valid_until.saturating_duration_since(Instant::now()).as_secs(),
                        "refreshed target resolution",
                    );
                    resolved
                }
                Err(error) => {
                    warn!(
                        host = %target.host,
                        ?error,
                        "target DNS refresh failed; keeping stale IP",
                    );
                    let stale_addr = {
                        let guard = target.current.read().expect("target cache lock poisoned");
                        guard.addr
                    };
                    Resolved {
                        addr: stale_addr,
                        valid_until: Instant::now() + DNS_FAILURE_REFRESH,
                    }
                }
            };

            {
                let mut guard = target.current.write().expect("target cache lock poisoned");
                *guard = Arc::new(next);
            }

            target.refresh_inflight.store(false, Ordering::Release);
        });
    }
}

fn split_host_port(raw: &str) -> Result<(String, u16)> {
    if let Some(rest) = raw.strip_prefix('[') {
        let end = rest
            .find(']')
            .ok_or_else(|| anyhow!("invalid target `{raw}`: missing `]` in bracketed host"))?;
        let host = &rest[..end];
        let after = &rest[end + 1..];
        let port_part = after
            .strip_prefix(':')
            .ok_or_else(|| anyhow!("invalid target `{raw}`: expected `:port` after `]`"))?;
        let port = parse_port(port_part, raw)?;
        return Ok((host.to_string(), port));
    }

    let (host, port_part) = raw
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("invalid target `{raw}`: expected `host:port`"))?;
    if host.is_empty() {
        bail!("invalid target `{raw}`: host is empty");
    }
    let port = parse_port(port_part, raw)?;
    Ok((host.to_string(), port))
}

fn parse_port(port_part: &str, raw: &str) -> Result<u16> {
    let port: u16 = port_part
        .parse()
        .map_err(|_| anyhow!("invalid target `{raw}`: `{port_part}` is not a valid port"))?;
    if port == 0 {
        bail!("invalid target `{raw}`: port must be non-zero");
    }
    Ok(port)
}

fn build_resolver(dns_server: Option<SocketAddr>) -> Result<TokioResolver> {
    let provider = TokioConnectionProvider::default();

    let builder = match dns_server {
        Some(addr) => {
            let group = NameServerConfigGroup::from_ips_clear(&[addr.ip()], addr.port(), true);
            let config = ResolverConfig::from_parts(None, vec![], group);
            TokioResolver::builder_with_config(config, provider)
        }
        None => {
            TokioResolver::builder(provider).context("failed to initialize system DNS resolver")?
        }
    };

    Ok(builder.build())
}

async fn resolve_once(resolver: &TokioResolver, host: &str, port: u16) -> Result<Resolved> {
    let lookup = resolver
        .lookup_ip(host)
        .await
        .with_context(|| format!("DNS lookup for `{host}` failed"))?;

    let ttl_deadline = lookup.valid_until();
    let addr = lookup
        .iter()
        .next()
        .ok_or_else(|| anyhow!("DNS lookup for `{host}` returned no records"))?;

    let now = Instant::now();
    let raw_ttl = ttl_deadline.saturating_duration_since(now);
    let clamped_ttl = raw_ttl.clamp(DNS_POSITIVE_MIN_TTL, DNS_POSITIVE_MAX_TTL);

    Ok(Resolved {
        addr: SocketAddr::new(addr, port),
        valid_until: now + clamped_ttl,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[tokio::test]
    async fn ip_literal_uses_static_branch() {
        let target = TargetAddr::bind("127.0.0.1:9000", None)
            .await
            .expect("bind static target");
        assert!(matches!(target, TargetAddr::Static(_)));
        assert_eq!(
            target.current(),
            SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 9000)
        );
        assert_eq!(target.display(), "127.0.0.1:9000");
    }

    #[tokio::test]
    async fn ipv6_literal_uses_static_branch() {
        let target = TargetAddr::bind("[::1]:9000", None)
            .await
            .expect("bind ipv6 static target");
        assert!(matches!(target, TargetAddr::Static(_)));
        assert_eq!(target.current().port(), 9000);
    }

    #[test]
    fn rejects_missing_port() {
        let err = split_host_port("example.com").unwrap_err().to_string();
        assert!(err.contains("host:port"));
    }

    #[test]
    fn rejects_zero_port() {
        let err = split_host_port("example.com:0").unwrap_err().to_string();
        assert!(err.contains("non-zero"));
    }

    #[test]
    fn parses_bracketed_ipv6() {
        let (host, port) = split_host_port("[2001:db8::1]:443").expect("split ipv6");
        assert_eq!(host, "2001:db8::1");
        assert_eq!(port, 443);
    }

    #[tokio::test]
    async fn localhost_domain_resolves_to_dynamic() {
        // Uses the system resolver — `localhost` is configured in
        // /etc/hosts on every supported platform. Exercises the
        // happy path without requiring public DNS access in CI.
        let target = TargetAddr::bind("localhost:9000", None)
            .await
            .expect("bind localhost target");
        assert!(matches!(target, TargetAddr::Dynamic(_)));
        let addr = target.current();
        assert_eq!(addr.port(), 9000);
        assert!(addr.ip().is_loopback());
    }
}
