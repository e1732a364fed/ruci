/*!
 * Defines an AI generated steganography protocol.
 */

use anyhow::Result;
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rainbow::NetworkSteganographyProcessor;
use reqwest;

use ruci::Name;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[cfg(test)]
mod test;
use tracing::debug;

use crate::map::steganography::general::*;

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
pub struct AIGeneratedProcessor {
    config: AIProtocolConfig,
    client: reqwest::Client,
}

const YOU_ARE_PROMPT: &str = "You are a cybersecurity expert and network protocol designer specializing in network steganography. ";

const GENERATE_ALGO_SYSTEM_PROMPT: &str = "Your goal is to design a covert HTTP steganography protocol that is undetectable by modern DPI systems while maintaining efficient data transmission.";

impl AIGeneratedProcessor {
    pub fn new(config: AIProtocolConfig) -> Self {
        Self {
            config,
            client: no_proxy_client(),
        }
    }

    /// 请求AI生成一个新的隐写协议算法, 并存在 self.config.algorithm_description 中
    pub async fn generate_algorithm(&mut self) -> Result<()> {
        let messages = vec![
            json!({
                "role": "system",
                "content": format!("{} {}", YOU_ARE_PROMPT, GENERATE_ALGO_SYSTEM_PROMPT)
            }),
            json!({
                "role": "user",
                "content":  include_str!("ai_generate_protocol_prompt.md")
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

    pub async fn new_by_generate_algorithm(
        api_key: String,
        model: String,
        api_base_url: String,
        is_server: bool,
    ) -> Result<Self> {
        let mut result = Self::new(AIProtocolConfig {
            api_key: api_key.clone(),
            model: model.clone(),
            algorithm_description: String::new(), // 临时空字符串
            is_server,
            api_base_url: api_base_url.clone(),
        });

        result.generate_algorithm().await?;

        // 创建最终实例
        Ok(result)
    }
}

impl Name for AIGeneratedProcessor {
    fn name(&self) -> &str {
        "AIGeneratedProcessor"
    }
}

#[async_trait]
impl NetworkSteganographyProcessor for AIGeneratedProcessor {
    /// 调用OpenAI API处理数据
    ///
    /// 在客户端，处理目标地址和数据，生成写序列
    /// 在服务端，处理读到的客户端握手的写序列中的第一个包，生成读序列
    async fn encode_write(
        &self,
        plain_data: &[u8],
        is_client: bool,
        mime_type: Option<String>,
    ) -> Result<ParsedResult> {
        let data_base64 = BASE64.encode(data);

        let role = if self.config.is_server {
            "server"
        } else {
            "client"
        };
        let read_or_write = if is_read { "read" } else { "write" };
        let system_prompt = format!(
            "{YOU_ARE_PROMPT} You need to decode a {read_or_write} data sequence for a steganography protocol of the {role} endpoint according to the following algorithm description:\n\
             {}\n\n\
             Response Format:\n\
             For write sequences, please return:\n\
             WRITE_PACKETS: [base64 encoded actual packet sequence w1,w2,...,wN]\n\
             READ_LENGTHS: [expected received packet length sequence r1,r2,...,rN]\n\
             For read sequences, please return:\n\
             WRITE_PACKETS: [base64 encoded response packet sequence w1,w2,...,wN]\n\
             READ_LENGTHS: [expected received packet length sequence r1,r2,...,rN]\n",
             self.config.algorithm_description
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
        let user_prompt = match self.config.is_server {
            false => format!(
                "This is {} data. Please process the following information:\nMain data: {}",
                operation_type, data_base64
            ),
            true => format!(
                "This is {} data. Please extract the actual data from the following data:\n{}",
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

        if is_read {
            // 普通读取操作
            let sequence = parse_ai_response_to_read_sequence(content)?;
            Ok(ParsedResult::Read(sequence))
        } else {
            // 普通写入操作
            let sequence = parse_ai_response_to_write_sequence(content)?;
            Ok(ParsedResult::Write(sequence))
        }
    }

    /// 解密从隐写协议中读取的数据
    async fn decrypt_single_read(
        &self,
        cipher_data: Vec<u8>,
        packet_index: usize,
        is_client: bool,
    ) -> Result<Vec<u8>> {
        let data_base64 = BASE64.encode(&combined_data);

        // 构建system提示
        let system_prompt = format!(
            "{YOU_ARE_PROMPT} You need to decrypt data read from the steganography protocol according to the following algorithm description:\n\
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
fn parse_ai_response_to_read_sequence(content: &str) -> Result<ReadSequence> {
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
                    debug!("unknown key1: {}", key);
                } // 忽略未知字段
            }
        }
    }

    Ok(ReadSequence {
        write_packets,
        read_lengths,
        read_packets: Vec::new(),
        current_step: ReadStep {
            is_read: true,
            index: 0,
        },
    })
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
