use hbb_common::{
    config::{self, Config},
    log, tokio, ResultType,
};
use serde_json::Value;

const DYNAMIC_CONFIG_INTERVAL_SECS: u64 = 30 * 60;

// 动态端口：每 30 分钟拉取固定配置端点，仅 hbbs/hbbr 变化时更新并重启注册
#[tokio::main(flavor = "current_thread")]
async fn dynamic_config_async() {
    loop {
        let url = Config::get_option("rd-config-url");
        if url.is_empty() {
            tokio::time::sleep(std::time::Duration::from_secs(DYNAMIC_CONFIG_INTERVAL_SECS))
                .await;
            continue;
        }
        if let Err(e) = pull_and_apply(&url).await {
            log::warn!("动态端口拉取失败（保留旧配置）: {}: {}", url, e);
        }
        tokio::time::sleep(std::time::Duration::from_secs(DYNAMIC_CONFIG_INTERVAL_SECS)).await;
    }
}

async fn pull_and_apply(url: &str) -> ResultType<()> {
    let client =
        crate::hbbs_http::create_http_client_async(hbb_common::tls::TlsType::Rustls, false);
    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(12))
        .send()
        .await?;
    let text = resp.text().await?;
    let v: Value = serde_json::from_str(&text)?;
    let hbbs = v
        .get("hbbs")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_owned();
    let hbbr = v
        .get("hbbr")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_owned();
    if hbbs.is_empty() && hbbr.is_empty() {
        return Ok(());
    }
    let old_hbbs = Config::get_option("custom-rendezvous-server");
    let old_hbbr = Config::get_option("relay-server");
    let mut changed = false;
    if !hbbs.is_empty() && hbbs != old_hbbs {
        Config::set_option("custom-rendezvous-server".to_owned(), hbbs.clone());
        changed = true;
    }
    if !hbbr.is_empty() && hbbr != old_hbbr {
        Config::set_option("relay-server".to_owned(), hbbr.clone());
        changed = true;
    }
    if changed {
        log::info!("动态端口更新: hbbs={} hbbr={}", hbbs, hbbr);
        crate::rendezvous_mediator::RendezvousMediator::restart();
    }
    Ok(())
}

// 动态端口任务启动（rd-config-url 为空则跳过）
pub fn start() {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        if config::Config::get_option("rd-config-url").is_empty() {
            log::info!("未配置 rd-config-url，动态端口未启用");
            return;
        }
        std::thread::spawn(|| {
            dynamic_config_async();
        });
    }
}

#[cfg(test)]
mod tests {
    // 配置端点返回的 "host:port" 必须能被 check_port 原样解析（含 IPv6）
    #[test]
    fn test_dynamic_config_host_port() {
        for s in [
            "10.0.0.1:21116",
            "rd.example.com:21117",
            "[::1]:21116",
            "[2001:db8::1]:21116",
        ] {
            assert_eq!(hbb_common::socket_client::check_port(s, 0), s);
        }
    }

    #[test]
    fn test_dynamic_config_interval() {
        assert_eq!(super::DYNAMIC_CONFIG_INTERVAL_SECS, 30 * 60);
    }
}

