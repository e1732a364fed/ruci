/*!
 * Defines an AI generated steganography protocol.
 */

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
use tracing::debug;

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

fn no_proxy_client() -> reqwest::Client {
    reqwest::ClientBuilder::new().no_proxy().build().unwrap()
}

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
                "content": "You are a network protocol design expert, specializing in steganography protocols that can hide within HTTPS traffic."
            }),
            json!({
                "role": "user",
                "content": "Please design a steganography protocol algorithm with the following requirements:
                1. Steganography Principle:
                   - Each actual write operation (W) is converted into a series of alternating write and read operations (w1,r1,w2,r2,...,wN,rN)
                   - wi contains the actual information to be transmitted, ri is padding data for steganography
                   - The first packet w1 contains metadata for the entire sequence, enabling the receiver to understand subsequent interaction patterns
                2. Protocol Characteristics:
                   - After sending w1, the sender waits for r1 from the receiver before sending w2
                   - After receiving w1, the receiver can parse the structure of the entire sequence and know when to send ri
                   - This alternating read-write pattern helps hide the true data flow direction
                   - Special case: when ri length is 0, skip that read step and directly send wi+1
                3. HTTPS Camouflage Requirements:
                   - All packets must conform to TLS format
                   - Packet length distribution must match typical HTTPS traffic
                   - Read-write operation timing patterns must mimic HTTP over TLS characteristics
                   - Ensure overall traffic characteristics (packet size distribution, read-write frequency, burstiness) match normal HTTPS traffic
                4. Handshake Phase Special Requirements:
                   - Client handshake: input contains target address information (network type, domain, IP, port), must encode this in handshake packets
                   - Server handshake: must be able to parse complete target address information from handshake packets
                   - Handshake packets must follow the w1,r1,w2,r2,...,wN,rN sequence format
                
                Please describe the algorithm in a structured way that other AI systems can accurately understand and execute.
                Algorithm description must include:
                1. How to encode sequence information in w1
                2. How to ensure packets conform to TLS format
                3. How to control packet size and timing distribution
                4. How to handle target address information during handshake"
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
        is_read: bool,
    ) -> Result<AIResult> {
        let data_base64 = BASE64.encode(data);
        let target_addr_str = target_addr.map(|addr| addr.to_string());
        let early_data_base64 = early_data.map(|data| BASE64.encode(data));

        // 构建system提示
        let role = if self.config.is_server {
            "server"
        } else {
            "client"
        };
        let operation = if is_read { "read" } else { "write" };
        let system_prompt = format!(
            "You are the {} of a network protocol processor. You need to decode a {} data sequence for a steganography protocol according to the following algorithm description:\n\
             {}\n\n\
             Response Format:\n\
             For write sequences, please return:\n\
             WRITE_PACKETS: [base64 encoded actual packet sequence w1,w2,...,wN]\n\
             READ_LENGTHS: [expected received packet length sequence r1,r2,...,rN]\n\
             For read sequences, please return:\n\
             WRITE_PACKETS: [base64 encoded response packet sequence w1,w2,...,wN]\n\
             READ_LENGTHS: [expected received packet length sequence r1,r2,...,rN]\n\
             TARGET_ADDR_*: [address information, only needed for server handshake]",
            role, operation, self.config.algorithm_description
        );

        let operation_type = if is_handshake {
            if self.config.is_server {
                "Server Handshake"
            } else {
                "Client Handshake"
            }
        } else {
            if self.config.is_server {
                "Server Normal"
            } else {
                "Client Normal"
            }
        };

        // 构建用户提示
        let user_prompt = match (self.config.is_server, target_addr_str, early_data_base64) {
            (false, Some(addr), Some(early)) => format!(
                "This is {} data. Please process the following information:\nTarget address: {}\nEarly data: {}\nMain data: {}",
                operation_type, addr, early, data_base64
            ),
            (false, Some(addr), None) => format!(
                "This is {} data. Please process the following information:\nTarget address: {}\nMain data: {}",
                operation_type, addr, data_base64
            ),
            (true, _, _) => format!(
                "This is {} data. Please extract the target address and actual data from the following data:\n{}",
                operation_type, data_base64
            ),
            _ => format!(
                "This is {} data. Please process the following data:\n{}",
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

        debug!("Sending request to OpenAI API: {}", api_url);
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

        // debug!("Response data: {}", response_data);
        let content = response_data["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid API response format"))?;

        if is_handshake && self.config.is_server {
            // 握手阶段的服务端需要解析地址信息
            let (sequence, addr) = parse_ai_response_to_read_sequence(content)?;
            Ok(AIResult::Read { sequence, addr })
        } else if is_read {
            // 普通读取操作
            let (sequence, _) = parse_ai_response_to_read_sequence(content)?;
            Ok(AIResult::Read {
                sequence,
                addr: None,
            })
        } else {
            // 普通写入操作
            let sequence = parse_ai_response_to_write_sequence(content)?;
            Ok(AIResult::Write(sequence))
        }
    }
    /// 创建一个新的AIGeneratedMap实例，自动生成算法描述
    pub async fn from_generated_algorithm(
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
            "You are a network protocol processor. You need to decrypt data read from the steganography protocol according to the following algorithm description:\n\
             {}\n\n\
             Please decrypt the following data and return the original data.\n\
             Response format:\n\
             DECRYPTED_DATA: [base64 encoded decrypted data]",
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

        Err(anyhow::anyhow!(
            "No decrypted data found in response, {}",
            content
        ))
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
                    debug!("unknown key1: {}", key);
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
                    debug!("unknown key2: {}", key);
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
