/*! see https://github.com/e1732a364fed/geosite-gfw
*/
use anyhow::{bail, Context};
use async_trait::async_trait;
use ruci::map::fold::DMIterBox;
use ruci::map::Data;
use ruci::net;
use ruci::relay::route;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GeositeGfwConfig {
    // "http://127.0.0.1:5000/check";
    pub api_url: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,

    #[serde(default)]
    pub only_proxy: bool,
    pub ok_ban_out_tag: (String, String),
}
pub struct GeositeGfwOutSelector {
    pub config: GeositeGfwConfig,

    pub outbounds_map: Arc<HashMap<String, DMIterBox>>, //out_tag -> outbound
}
#[async_trait]
impl route::OutSelector for GeositeGfwOutSelector {
    async fn select(
        &self,
        _is_fallback: bool,
        addr: &net::Addr,
        _in_chain_tag: &str,
        _params: &[Option<Box<dyn Data>>],
    ) -> Option<DMIterBox> {
        let domain_or_ip = addr.get_name().or(addr.get_ip().map(|ip| ip.to_string()))?;

        let r = check_api(&self.config, &domain_or_ip).await;
        match r {
            Ok(r) => {
                let r = is_prediction_ok(&r.head_prediction, &r.body_prediction);

                let ot = match r {
                    true => &self.config.ok_ban_out_tag.0,
                    false => &self.config.ok_ban_out_tag.1,
                };
                self.outbounds_map.get(ot).cloned()
            }
            Err(e) => {
                tracing::debug!("geosite-gfw api got e: {e}");
                None
            }
        }
    }
}

/// 定义请求体结构
#[derive(Serialize, Default)]
pub struct CheckRequest<'a> {
    pub domain: &'a str,
    pub socks5_proxy: Option<&'a str>,

    #[serde(default)]
    pub only_proxy: bool,
}

/// 定义 API 响应结构
#[derive(Deserialize, Debug)]
pub struct CheckResponse {
    pub domain: String,
    pub head_prediction: String,
    pub body_prediction: String,
    pub cached: Option<bool>,
}

/// 异步访问 API
pub async fn check_api(config: &GeositeGfwConfig, domain: &str) -> anyhow::Result<CheckResponse> {
    let client = Client::new();

    let request_data = CheckRequest {
        domain,
        socks5_proxy: config.proxy.as_deref(),
        only_proxy: config.only_proxy,
    };

    let response = client
        .post(&config.api_url)
        .json(&request_data)
        .send()
        .await
        .context("send failed")?;

    let full_body = response.bytes().await?;

    let response = serde_json::from_slice(&full_body);

    let response = match response {
        Ok(r) => r,
        Err(e) => {
            bail!(
                "parse body err: {e}, body is {}",
                String::from_utf8_lossy(&full_body)
            )
        }
    };

    debug!("geosite_gfw got response: {:?}", response);
    Ok(response)
}

pub fn is_prediction_ok(head: &str, body: &str) -> bool {
    head.eq("ok") && body.eq("ok")
}
