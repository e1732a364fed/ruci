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

#[cfg(test)]
mod tests;
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
            client: no_proxy_client(),
            // ext_fields: None,
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
                1. 隐写原理：
                   - 每个实际的写入操作(W)都会被转换为一系列交替的写和读操作(w1,r1,w2,r2,...,wN,rN)
                   - 其中wi包含实际需要传输的信息，ri是用于隐写的填充数据
                   - 第一个包w1包含整个序列的元信息，使得接收方知道后续的交互模式
                2. 协议特性：
                   - 发送方在发送w1后，等待接收方返回r1才能发送w2
                   - 接收方在收到w1后，能解析出整个序列的结构，知道何时发送ri
                   - 这种交替的读写模式帮助隐藏真实的数据流向
                   - 特殊情况：当ri长度为0时，表示跳过该读取步骤，直接发送下一个wi+1
                3. HTTPS伪装要求：
                   - 所有数据包必须符合TLS格式
                   - 数据包长度分布要与典型的HTTPS流量相匹配
                   - 读写操作的时序模式要模仿HTTP over TLS的特征
                   - 确保整体流量特征（包大小分布、读写频率、突发性等）与正常HTTPS流量一致
                4. 握手阶段特殊要求：
                   - 客户端握手：输入包含目标地址信息（网络类型、域名、IP、端口），需要将这些信息编码在握手包中
                   - 服务端握手：需要能从握手包中解析出完整的目标地址信息
                   - 握手包同样需要遵循w1,r1,w2,r2,...,wN,rN的序列格式
                
                请以结构化的方式描述算法，使得其他AI系统可以准确理解和执行。
                算法描述中必须包含：
                1. 如何在w1中编码序列信息
                2. 如何确保数据包符合TLS格式
                3. 如何控制数据包大小和时序分布
                4. 如何在握手阶段处理目标地址信息"
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

    /// 调用OpenAI API处理数据
    ///
    /// 在客户端，处理目标地址和数据，生成写序列
    /// 在服务端，处理读到的客户端握手的写序列中的第一个包，生成读序列
    async fn generate_sequence_with_ai(
        &self,
        data: &[u8],
        target_addr: Option<&net::Addr>,
        early_data: Option<&[u8]>,
        is_handshake: bool,
    ) -> Result<AIResult> {
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
            "你是一个网络协议处理器的{}。你需要按照以下算法描述实现一个隐写协议：\n\
             {}\n\n\
             响应格式：\n\
             对于写序列，请返回：\n\
             WRITE_PACKETS: [base64编码的实际数据包序列 w1,w2,...,wN]\n\
             READ_LENGTHS: [期望接收的数据包长度序列 r1,r2,...,rN]\n\
             对于读序列，请返回：\n\
             WRITE_PACKETS: [base64编码的响应数据包序列 w1,w2,...,wN]\n\
             READ_LENGTHS: [期望接收的数据包长度序列 r1,r2,...,rN]\n\
             TARGET_ADDR_*: [地址信息，仅在服务端握手时需要]",
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

        tracing::debug!("Sending request to OpenAI API: {}", api_url);
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

        // tracing::debug!("Response data: {}", response_data);
        let content = response_data["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid API response format"))?;

        // 解析AI的响应
        if self.config.is_server {
            // 服务端：解析客户端的写序列第一个包，生成读序列
            let (sequence, addr) = parse_ai_response_to_read_sequence(content)?;
            Ok(AIResult::Read { sequence, addr })
        } else {
            // 客户端：生成写序列
            let sequence = parse_ai_response_to_write_sequence(content)?;
            Ok(AIResult::Write(sequence))
        }
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

    /// 解密从隐写协议中读取的数据
    async fn decrypt_read_sequence(&self, combined_data: Vec<u8>) -> Result<Vec<u8>> {
        let data_base64 = BASE64.encode(&combined_data);

        // 构建system提示
        let system_prompt = format!(
            "你是一个网络协议处理器。你需要按照以下算法描述解密从隐写协议中读取的数据：\n\
             {}\n\n\
             请解密以下数据并返回原始数据。\n\
             响应格式：\n\
             DECRYPTED_DATA: [base64编码的解密后数据]",
            self.config.algorithm_description
        );

        let messages = vec![
            json!({
                "role": "system",
                "content": system_prompt
            }),
            json!({
                "role": "user",
                "content": format!("ENCRYPTED_DATA: {}", data_base64)
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

        // 解析响应
        for line in content.lines() {
            let line = line.trim();
            if let Some((key, value)) = line.split_once(':') {
                if key.trim() == "DECRYPTED_DATA" {
                    return Ok(BASE64.decode(value.trim())?);
                }
            }
        }

        Err(anyhow::anyhow!("No decrypted data found in response"))
    }
}

/// 解析服务端AI响应，生成读序列
fn parse_ai_response_to_read_sequence(content: &str) -> Result<(ReadSequence, Option<net::Addr>)> {
    let mut write_packets = Vec::new();
    let mut read_lengths = Vec::new();
    let mut network = None;
    let mut host = None;
    let mut ip = None;
    let mut port = None;

    for line in content.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim();
            match key.trim() {
                "WRITE_PACKETS" => {
                    // 解析逗号分隔的base64编码数据包
                    for packet in value.split(',') {
                        let packet = packet.trim();
                        if !packet.is_empty() {
                            write_packets.push(BASE64.decode(packet)?);
                        }
                    }
                }
                "READ_LENGTHS" => {
                    // 解析逗号分隔的长度值
                    for len in value.split(',') {
                        let len = len.trim();
                        if !len.is_empty() {
                            read_lengths.push(len.parse()?);
                        }
                    }
                }
                "TARGET_ADDR_NETWORK" => {
                    if !value.is_empty() {
                        network = Some(value.to_string())
                    }
                }
                "TARGET_ADDR_HOST" => {
                    if !value.is_empty() {
                        host = Some(value.to_string())
                    }
                }
                "TARGET_ADDR_IP" => {
                    if !value.is_empty() {
                        ip = Some(value.parse()?)
                    }
                }
                "TARGET_ADDR_PORT" => {
                    if !value.is_empty() {
                        port = Some(value.parse()?)
                    }
                }
                _ => {
                    tracing::debug!("unknown key1: {}", key);
                } // 忽略未知字段
            }
        }
    }

    // 构造目标地址（如果有必要的信息）
    let target_addr = if let (Some(network), Some(port)) = (network, port) {
        Some(net::Addr::from(&network, host, ip, port)?)
    } else {
        None
    };

    Ok((
        ReadSequence {
            write_packets,
            read_lengths,
            read_packets: Vec::new(),
            current_step: ReadStep {
                is_read: true,
                index: 0,
            },
        },
        target_addr,
    ))
}

/// 解析服务端AI响应，生成写序列
fn parse_ai_response_to_write_sequence(content: &str) -> Result<WriteSequence> {
    let mut write_packets = Vec::new();
    let mut read_lengths = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim();
            match key.trim() {
                "WRITE_PACKETS" => {
                    // 解析逗号分隔的base64编码数据包
                    for packet in value.split(',') {
                        let packet = packet.trim();
                        if !packet.is_empty() {
                            write_packets.push(BASE64.decode(packet)?);
                        }
                    }
                }
                "READ_LENGTHS" => {
                    // 解析逗号分隔的长度值
                    for len in value.split(',') {
                        let len = len.trim();
                        if !len.is_empty() {
                            read_lengths.push(len.parse()?);
                        }
                    }
                }
                _ => {
                    tracing::debug!("unknown key2: {}", key);
                } // 忽略未知字段
            }
        }
    }

    Ok(WriteSequence {
        write_packets,
        read_lengths,
        current_step: WriteStep {
            is_write: true,
            index: 0,
        },
    })
}

impl Name for AIGeneratedMap {
    fn name(&self) -> &str {
        "ai_generated"
    }
}

#[async_trait]
impl Map for AIGeneratedMap {
    async fn maps(&self, _cid: CID, behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match behavior {
            ProxyBehavior::ENCODE => {
                // 创建新的连接，包装原始连接
                let conn = AIConn::new(params.c.try_unwrap_tcp().unwrap(), self.clone());
                MapResult::builder().c(Stream::Conn(Box::new(conn))).build()
            }
            ProxyBehavior::DECODE => {
                // 服务端：直接创建连接
                if params.b.is_some() {
                    let conn = AIConn::new(params.c.try_unwrap_tcp().unwrap(), self.clone());
                    MapResult::builder().c(Stream::Conn(Box::new(conn))).build()
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

/// 写序列状态
#[derive(Debug)]
pub struct WriteStep {
    pub is_write: bool, // true: 执行 w* 操作, false: 执行 r* 操作
    pub index: usize,   // 对应 write_packets 或 read_lengths 的索引
}

#[derive(Debug)]
pub struct WriteSequence {
    pub write_packets: Vec<Vec<u8>>, // 改为 Vec
    pub read_lengths: Vec<usize>,    // 改为 Vec
    pub current_step: WriteStep,
}

/// 读序列状态
#[derive(Debug)]
pub struct ReadStep {
    pub is_read: bool, // true: 执行 r* 操作, false: 执行 w* 操作
    pub index: usize,  // 对应 read_packets 或 write_packets 的索引
}

#[derive(Debug)]
pub struct ReadSequence {
    pub write_packets: Vec<Vec<u8>>, // 改为 Vec
    pub read_lengths: Vec<usize>,    // 已经是 Vec
    pub read_packets: Vec<Vec<u8>>,  // 保持不变
    pub current_step: ReadStep,
}

impl ReadSequence {
    fn advance_to_next_write(&mut self) {
        self.current_step.is_read = false;
    }

    fn advance_to_next_read(&mut self) {
        self.current_step.is_read = true;
        self.current_step.index += 1;
    }
}

impl WriteSequence {
    fn advance_to_next_read(&mut self) {
        self.current_step.is_write = false;
    }

    fn advance_to_next_write(&mut self) {
        self.current_step.is_write = true;
        self.current_step.index += 1;
    }
}

/// AI处理结果
#[derive(Debug)]
pub enum AIResult {
    Write(WriteSequence),
    Read {
        sequence: ReadSequence,
        addr: Option<net::Addr>,
    },
}

pub fn no_proxy_client() -> reqwest::Client {
    reqwest::ClientBuilder::new().no_proxy().build().unwrap()
}
