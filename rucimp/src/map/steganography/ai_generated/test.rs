use std::sync::Arc;

use super::*;
use conn::GeneralConn;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::debug;
use tracing_subscriber;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// 添加测试初始化函数
fn init_tracing() {
    let _subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();
}

#[tokio::test]
async fn test_wiremock_get() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    // 启动 MockServer
    let mock_server = MockServer::start().await;

    // 设置一个 GET Mock
    Mock::given(method("GET"))
        .and(path("/hello"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Hello, world!"))
        .mount(&mock_server)
        .await;

    // 构造一个 GET 请求
    let response = no_proxy_client()
        .get(format!("{}/hello", mock_server.uri()))
        .send()
        .await?;

    // 检查响应状态和内容
    assert_eq!(response.status(), 200);
    let body = response.text().await?;
    assert_eq!(body, "Hello, world!");

    debug!("Test passed!");
    Ok(())
}

#[tokio::test]
async fn test_wiremock_post() -> Result<()> {
    init_tracing();
    let mock_server = MockServer::start().await;
    let url = mock_server.uri();
    debug!("mock_server: {}", url);

    Mock::given(method("POST"))
        .and(path("/test-path")) // 添加路径匹配
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "content": "\
                        WRITE_PACKETS: dGVzdF9wYWNrZXRfMQ==,dGVzdF9wYWNrZXRfMg==\n\
                        READ_LENGTHS: 10,10\n\
                        TARGET_ADDR_NETWORK: tcp\n\
                        TARGET_ADDR_HOST: example.com\n\
                        TARGET_ADDR_PORT: 443"
                }
            }]
        })))
        .mount(&mock_server)
        .await;

    let response = no_proxy_client()
        .post(format!("{}/test-path", url)) // 修改为 POST 并添加路径
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "test",
            "messages": vec![""],
            "temperature": 0.0,
        }))
        .send()
        .await;

    debug!("Response status: {:?}", response);
    let response = response?;
    let response_data = response.json::<serde_json::Value>().await?;
    debug!("Response data: {}", response_data);
    Ok(())
}

#[tokio::test]
async fn test_wiremock() -> Result<()> {
    init_tracing();
    let mock_server = MockServer::start().await;

    let url = mock_server.uri();
    debug!("mock_server: {}", url);

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("Authorization", "Bearer test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "content": "\
                        WRITE_PACKETS: dGVzdF9wYWNrZXRfMQ==,dGVzdF9wYWNrZXRfMg==\n\
                        READ_LENGTHS: 10,10\n\
                        TARGET_ADDR_NETWORK: tcp\n\
                        TARGET_ADDR_HOST: example.com\n\
                        TARGET_ADDR_PORT: 443"
                }
            }]
        })))
        .expect(1) // 期望被调用一次
        .mount(&mock_server)
        .await;

    let response = no_proxy_client()
        .post(format!("{}/v1/chat/completions", url))
        .header("Authorization", format!("Bearer {}", "test")) // 须与 mock 的 Authorization 一致
        .json(&json!({
            "model": "test",
            "messages": vec![""],
            "temperature": 0.0,
        }))
        .send()
        .await;

    debug!("Response status: {:?}", response);
    let response = response?;
    let response_data = response.json::<serde_json::Value>().await?;
    debug!("Response data: {}", response_data);

    mock_server.verify().await;
    Ok(())
}

async fn server_handle_write(
    mut server_tcp: tokio::io::DuplexStream,
) -> Result<tokio::io::DuplexStream> {
    let mut buf = vec![0u8; 1024];

    debug!("server_handle start");

    // 读取第一个包
    let n = server_tcp.read(&mut buf).await?;
    assert_eq!(&buf[..n], b"test_packet_1");
    debug!("server_handle read packet 1 done");

    // 发送 r1 响应
    let r1_data = vec![1u8; 10];
    server_tcp.write_all(&r1_data).await?;
    debug!("server_handle write r1 done");

    // 读取第二个包
    let n = server_tcp.read(&mut buf).await?;
    assert_eq!(&buf[..n], b"test_packet_2");
    debug!("server_handle read packet 2 done");

    // 发送 r2 响应
    let r2_data = vec![1u8; 10];
    server_tcp.write_all(&r2_data).await?;
    debug!("server_handle write r2 done");

    Ok::<_, anyhow::Error>(server_tcp)
}

async fn server_handle_read(
    mut server_tcp: tokio::io::DuplexStream,
) -> Result<tokio::io::DuplexStream> {
    debug!("server_handle start");

    // 发送初始数据触发读取操作
    server_tcp.write_all(b"initial_test_data").await?;

    debug!("server_handle write initial_test_data done");

    // 等待一小段时间确保数据被处理
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    debug!("server_handle sleep done");

    // 发送第一个预期的数据包 (15 bytes)
    // 需要对应 mock 的 choices.message.content 中的 READ_LENGTHS

    server_tcp.write_all(&vec![2u8; 15]).await?;

    debug!("server_handle write r1 done");

    // 读取第一个响应包
    let mut buf = vec![0u8; 1024];
    let n = server_tcp.read(&mut buf).await?;
    assert_eq!(&buf[..n], b"packet_1");

    debug!("server_handle read r1 done");

    // 发送第二个预期的数据包
    server_tcp.write_all(&vec![3u8; 15]).await?;

    debug!("server_handle write r2 done");

    // 读取第二个响应包
    let mut buf = vec![0u8; 1024];
    let n = server_tcp.read(&mut buf).await?;
    assert_eq!(&buf[..n], b"packet_2");

    debug!("server_handle read r2 done");

    Ok::<_, anyhow::Error>(server_tcp)
}

async fn mock_set_post_generate_sequence_for_read(mock_server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("Authorization", "Bearer test"))
        .and(body_string_contains(
            "You need to decode a read data sequence for a steganography protocol",
        ))
        // packet_1, packet_2
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "content": "\
                        WRITE_PACKETS: cGFja2V0XzE=,cGFja2V0XzI=\n\
                        READ_LENGTHS: 15,15\n\
                        TARGET_ADDR_NETWORK: tcp\n\
                        TARGET_ADDR_HOST: example.com\n\
                        TARGET_ADDR_PORT: 443"
                }
            }]
        })))
        // .expect(2) // 期望被调用两次，因为会有两轮读写
        .mount(&mock_server)
        .await;
}

async fn mock_set_post_generate_sequence_for_write(mock_server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("Authorization", "Bearer test"))
        .and(body_string_contains(
            "You need to decode a write data sequence",
        ))
        // test_packet_1, test_packet_2
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "content": "\
                        WRITE_PACKETS: dGVzdF9wYWNrZXRfMQ==,dGVzdF9wYWNrZXRfMg==\n\
                        READ_LENGTHS: 10,10\n\
                        TARGET_ADDR_NETWORK: tcp\n\
                        TARGET_ADDR_HOST: example.com\n\
                        TARGET_ADDR_PORT: 443"
                }
            }]
        })))
        // .expect(1) // 期望被调用一次
        .mount(&mock_server)
        .await;
}

async fn mock_set_post_decrypt_data(mock_server: &MockServer) {
    // 设置解密数据的 Mock
    Mock::given(method("POST"))
    .and(path("/v1/chat/completions"))
    .and(header("Authorization", "Bearer test"))
    .and(body_string_contains("You need to decrypt data read from the steganography protocol according to the following algorithm"))
    .and(body_string_contains("ENCRYPTED_DATA:"))
    .respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "choices": [{
            "message": {
                "content": "DECRYPTED_DATA: ZGVjcnlwdGVkX2RhdGE="
            }
        }]
    })))
    // .expect(2) // 期望被调用两次，对应两次读取操作
    .mount(&mock_server)
    .await;
}

#[tokio::test]
async fn test_basic_write_sequence() -> Result<()> {
    init_tracing();
    debug!("test_basic_write_sequence start");
    // 启动模拟服务器
    let mock_server = MockServer::start().await;

    debug!("mock_server: {}", mock_server.uri());

    mock_set_post_generate_sequence_for_write(&mock_server).await;

    debug!("mock_server.mount done");

    // 创建一对连接的 TCP 流
    let (client_tcp, server_tcp) = tokio::io::duplex(1024);

    // 创建使用模拟服务器的 AIGeneratedMap
    let map = AIGeneratedProcessor {
        config: AIProtocolConfig {
            api_key: "test".to_string(),
            model: "test".to_string(),
            algorithm_description: "test".to_string(),
            is_server: false,
            api_base_url: mock_server.uri(),
        },
        client: no_proxy_client(),
    };

    let mut conn = GeneralConn::new(
        Box::new(client_tcp),
        GeneralMap::from_processor(false, Arc::new(Box::new(map))),
        None,
    );

    // 在另一个任务中处理服务端
    let server_handle = tokio::spawn(async move {
        server_handle_write(server_tcp).await?;

        Ok::<_, anyhow::Error>(())
    });

    debug!("write_all");
    // 写入测试数据
    conn.write_all(b"test_data").await?;
    debug!("write_all done");

    // 等待一段时间确保所有操作完成
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // 正常关闭连接
    conn.shutdown().await?;

    // 等待服务端处理完成
    server_handle.await??;

    // 验证所有预期的 mock 调用都发生了
    mock_server.verify().await;

    debug!("mock_server.verify done");

    Ok(())
}

#[tokio::test]
async fn test_basic_read_sequence() -> Result<()> {
    init_tracing();
    debug!("test_basic_read_sequence start");

    let mock_server = MockServer::start().await;
    debug!("mock_server: {}", mock_server.uri());

    mock_set_post_generate_sequence_for_read(&mock_server).await;
    mock_set_post_decrypt_data(&mock_server).await;

    // 创建一对连接的 TCP 流
    let (client_tcp, server_tcp) = tokio::io::duplex(1024);

    // 创建使用模拟服务器的 AIGeneratedMap
    let map = AIGeneratedProcessor {
        config: AIProtocolConfig {
            api_key: "test".to_string(),
            model: "test".to_string(),
            algorithm_description: "test".to_string(),
            is_server: false,
            api_base_url: mock_server.uri(),
        },
        client: no_proxy_client(),
    };

    let mut conn = GeneralConn::new(
        Box::new(client_tcp),
        GeneralMap::from_processor(false, Arc::new(Box::new(map))),
        None,
    );

    // 在另一个任务中处理服务端
    let server_handle = tokio::spawn(async move { server_handle_read(server_tcp).await });

    // 读取数据
    let mut read_buf = vec![0u8; 1024];
    let n = AsyncReadExt::read(&mut conn, &mut read_buf).await?;

    // 验证解密后的数据
    assert_eq!(&read_buf[..n], b"decrypted_data");

    // 等待服务端处理完成
    server_handle.await??;

    // 验证所有预期的 mock 调用都发生了
    mock_server.verify().await;

    Ok(())
}

#[tokio::test]
async fn test_multiple_read_write_sequence() -> Result<()> {
    init_tracing();
    debug!("test_multiple_read_write_sequence start");
    let mock_server = MockServer::start().await;

    mock_set_post_generate_sequence_for_write(&mock_server).await;

    mock_set_post_generate_sequence_for_read(&mock_server).await;
    mock_set_post_decrypt_data(&mock_server).await;

    let (client_tcp, mut server_tcp) = tokio::io::duplex(1024);
    let map = AIGeneratedProcessor {
        config: AIProtocolConfig {
            api_key: "test".to_string(),
            model: "test".to_string(),
            algorithm_description: "test".to_string(),
            is_server: false,
            api_base_url: mock_server.uri(),
        },
        client: no_proxy_client(),
    };

    let mut conn = GeneralConn::new(
        Box::new(client_tcp),
        GeneralMap::from_processor(false, Arc::new(Box::new(map))),
        None,
    );

    // 在另一个任务中处理服务端
    let server_handle = tokio::spawn(async move {
        debug!("server_handle start");

        server_tcp = server_handle_read(server_tcp).await?;
        server_tcp = server_handle_write(server_tcp).await?;

        server_tcp = server_handle_read(server_tcp).await?;
        server_handle_write(server_tcp).await?;

        Ok::<_, anyhow::Error>(())
    });

    // 客户端操作
    // 第一轮读写
    let mut read_buf = vec![0u8; 1024];
    let n = AsyncReadExt::read(&mut conn, &mut read_buf).await?;
    assert_eq!(&read_buf[..n], b"decrypted_data");

    conn.write_all(b"test_data_1").await?;

    // 第二轮读写
    let n = AsyncReadExt::read(&mut conn, &mut read_buf).await?;
    assert_eq!(&read_buf[..n], b"decrypted_data");

    conn.write_all(b"test_data_2").await?;

    // 等待服务端处理完成
    server_handle.await??;

    // 验证所有预期的 mock 调用都发生了
    mock_server.verify().await;

    Ok(())
}
