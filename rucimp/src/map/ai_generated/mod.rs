use anyhow::Result;
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use reqwest;
use ruci::{
    map::*,
    net::{self, Stream, CID},
    Name,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

mod conn;
use conn::AIConn;

/// AI生成的协议的参数，包含算法描述和OpenAI API配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIProtocolConfig {
    api_key: String,
    model: String,
    algorithm_description: String,
    is_server: bool,      // 添加服务端标识
    api_base_url: String, // 添加API基础URL配置
}

/// AI生成的协议实现
#[derive(Debug, Clone)]
pub struct AIGeneratedMap {
    config: AIProtocolConfig,
    client: reqwest::Client,
    // ext_fields: Option<MapExtFields>,
}

impl AIGeneratedMap {
    pub fn new(config: AIProtocolConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            // ext_fields: None,
        }
    }

    /// 调用OpenAI API处理数据
    async fn process_with_ai(
        &self,
        data: &[u8],
        target_addr: Option<&net::Addr>,
        early_data: Option<&[u8]>,
        is_handshake: bool,
    ) -> Result<(Vec<u8>, Option<net::Addr>)> {
        let data_base64 = BASE64.encode(data);
        let target_addr_str = target_addr.map(|addr| addr.to_string());
        let early_data_base64 = early_data.map(|data| BASE64.encode(data));

        // 构建system提示
        let role = if self.config.is_server {
            "服务端"
        } else {
            "客户端"
        };
        let system_prompt = format!(
            "你是一个网络协议处理器的{}。根据以下算法描述处理数据：\n{}\n\n\
             请按以下格式返回结果：\n\
             PROCESSED_DATA: <base64编码的处理后数据>\n\
             TARGET_ADDR_NETWORK: <网络类型：tcp/udp/ip>\n\
             TARGET_ADDR_HOST: <域名，如果没有则返回空>\n\
             TARGET_ADDR_IP: <IP地址，如果没有则返回空>\n\
             TARGET_ADDR_PORT: <端口号>\n\
             EARLY_DATA: <base64编码的early data>",
            role, self.config.algorithm_description
        );

        let operation_type = if is_handshake {
            if self.config.is_server {
                "服务端握手"
            } else {
                "客户端握手"
            }
        } else {
            if self.config.is_server {
                "服务端普通"
            } else {
                "客户端普通"
            }
        };

        // 构建用户提示
        let user_prompt = match (self.config.is_server, target_addr_str, early_data_base64) {
            (false, Some(addr), Some(early)) => format!(
                "这是{}数据。请处理以下信息：\n目标地址：{}\n早期数据：{}\n主要数据：{}",
                operation_type, addr, early, data_base64
            ),
            (false, Some(addr), None) => format!(
                "这是{}数据。请处理以下信息：\n目标地址：{}\n主要数据：{}",
                operation_type, addr, data_base64
            ),
            (true, _, _) => format!(
                "这是{}数据。请从以下数据中提取目标地址和实际数据：\n{}",
                operation_type, data_base64
            ),
            _ => format!(
                "这是{}数据。请处理以下数据：\n{}",
                operation_type, data_base64
            ),
        };

        let messages = vec![
            json!({
                "role": "system",
                "content": system_prompt
            }),
            json!({
                "role": "user",
                "content": user_prompt
            }),
        ];

        // 发送请求到OpenAI API
        let api_url = format!(
            "{}/v1/chat/completions",
            self.config.api_base_url.trim_end_matches('/')
        );
        let response = self
            .client
            .post(api_url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&json!({
                "model": self.config.model,
                "messages": messages,
                "temperature": 0.0,
            }))
            .send()
            .await?;

        let response_data = response.json::<serde_json::Value>().await?;
        let content = response_data["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid API response format"))?;

        // 解析AI的响应
        if self.config.is_server {
            // 服务端需要从响应中提取目标地址和数据
            let (processed_data, extracted_addr) = parse_server_response(content)?;
            Ok((processed_data, extracted_addr))
        } else {
            // 客户端只需要处理后的数据
            let processed_data = BASE64.decode(content.trim())?;
            Ok((processed_data, None))
        }
    }

    /// 请求AI生成一个新的隐写协议算法
    pub async fn generate_algorithm(&mut self) -> Result<()> {
        let messages = vec![
            json!({
                "role": "system",
                "content": "你是一个网络协议设计专家，专门设计能够隐藏在HTTPS流量中的隐写协议。"
            }),
            json!({
                "role": "user",
                "content": "请设计一个隐写协议算法，要求如下：
                1. 该协议需要完全隐藏在HTTPS流量中，外部观察者无法区分此流量与普通HTTPS流量
                2. 协议需要包含握手阶段和数据传输阶段
                3. 算法必须是确定性的，这样服务端和客户端使用相同的算法描述时可以正确通信
                4. 请详细描述：
                   - 握手阶段如何处理数据
                   - 普通数据传输阶段如何处理数据
                   - 如何确保流量特征与HTTPS相似
                请以结构化的方式描述算法，使得其他AI系统可以准确理解和执行。"
            }),
        ];

        // 构建完整的API URL
        let api_url = format!(
            "{}/v1/chat/completions",
            self.config.api_base_url.trim_end_matches('/')
        );

        // 发送请求到OpenAI API
        let response = self
            .client
            .post(api_url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&json!({
                "model": self.config.model,
                "messages": messages,
                "temperature": 0.2,  // 使用较低的temperature以保持一致性，但允许一定的创造性
            }))
            .send()
            .await?;

        let response_data = response.json::<serde_json::Value>().await?;
        let algorithm_description = response_data["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid API response format"))?
            .to_string();

        self.config.algorithm_description = algorithm_description;
        Ok(())
    }

    /// 创建一个新的AIGeneratedMap实例，自动生成算法描述
    pub async fn create_with_generated_algorithm(
        api_key: String,
        model: String,
        api_base_url: String,
        is_server: bool,
    ) -> Result<Self> {
        // 创建临时实例来生成算法
        let mut result = Self::new(AIProtocolConfig {
            api_key: api_key.clone(),
            model: model.clone(),
            algorithm_description: String::new(), // 临时空字符串
            is_server,
            api_base_url: api_base_url.clone(),
        });

        // 生成算法描述并修改自身配置
        result.generate_algorithm().await?;

        // 创建最终实例
        Ok(result)
    }
}

/// 解析服务端AI响应
/// 期望的响应格式：
/// ```text
/// PROCESSED_DATA: <base64编码的处理后数据>
/// TARGET_ADDR_NETWORK: <网络类型：tcp/udp/ip>
/// TARGET_ADDR_HOST: <域名，如果没有则返回空>
/// TARGET_ADDR_IP: <IP地址，如果没有则返回空>
/// TARGET_ADDR_PORT: <端口号>
/// ```
fn parse_server_response(content: &str) -> Result<(Vec<u8>, Option<net::Addr>)> {
    let mut processed_data = None;
    let mut network = None;
    let mut host = None;
    let mut ip = None;
    let mut port = None;

    // 按行解析响应
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim();
            match key.trim() {
                "PROCESSED_DATA" => {
                    processed_data =
                        Some(BASE64.decode(value).map_err(|e| {
                            anyhow::anyhow!("Invalid processed data base64: {}", e)
                        })?);
                }
                "TARGET_ADDR_NETWORK" => {
                    if !value.is_empty() {
                        network = Some(value.to_string());
                    }
                }
                "TARGET_ADDR_HOST" => {
                    if !value.is_empty() {
                        host = Some(value.to_string());
                    }
                }
                "TARGET_ADDR_IP" => {
                    if !value.is_empty() {
                        ip = Some(
                            value
                                .parse()
                                .map_err(|e| anyhow::anyhow!("Invalid IP address: {}", e))?,
                        );
                    }
                }
                "TARGET_ADDR_PORT" => {
                    if !value.is_empty() {
                        port = Some(
                            value
                                .parse()
                                .map_err(|e| anyhow::anyhow!("Invalid port: {}", e))?,
                        );
                    }
                }
                _ => {} // 忽略未知字段
            }
        }
    }

    // 确保至少有processed_data
    let processed_data =
        processed_data.ok_or_else(|| anyhow::anyhow!("Missing processed data in response"))?;

    // 如果有必要的地址信息，则构造Addr
    let target_addr = if let (Some(network), Some(port)) = (network, port) {
        Some(net::Addr::from(&network, host, ip, port)?)
    } else {
        None
    };

    Ok((processed_data, target_addr))
}

impl Name for AIGeneratedMap {
    fn name(&self) -> &str {
        "ai_generated"
    }
}

#[async_trait]
impl Map for AIGeneratedMap {
    async fn maps(&self, cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match behavior {
            ProxyBehavior::ENCODE => {
                // 客户端：编码目标地址和数据
                let (processed_data, _) = self
                    .process_with_ai(
                        &[], // 空主数据
                        params.a.as_ref(),
                        params.b.as_ref().map(|b| &b[..]),
                        true,
                    )
                    .await
                    .unwrap();

                // 创建新的连接，包装原始连接
                let conn = AIConn::new(
                    params.c.try_unwrap_tcp().unwrap(),
                    self.clone(),
                    processed_data,
                );

                MapResult::builder().c(Stream::Conn(Box::new(conn))).build()
            }
            ProxyBehavior::DECODE => {
                // 服务端：解码出目标地址和数据
                if let Some(data) = params.b {
                    let (processed_data, extracted_addr) =
                        self.process_with_ai(&data, None, None, true).await.unwrap();

                    // 创建新的连接，包装原始连接
                    let conn = AIConn::new(
                        params.c.try_unwrap_tcp().unwrap(),
                        self.clone(),
                        processed_data,
                    );

                    MapResult::builder()
                        .c(Stream::Conn(Box::new(conn)))
                        .a(extracted_addr)
                        .build()
                } else {
                    MapResult::builder().c(params.c).build()
                }
            }
            ProxyBehavior::UNSPECIFIED => {
                MapResult::from_e(anyhow::anyhow!("Unspecified behavior is not supported"))
            }
        }
    }
}
