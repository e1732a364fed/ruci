/*!
Defines a steganography protocol example1 "spe1".

使用 html, 用 post 表示 client 向 server 上传数据，

称一对问答为一个QA，128个QA为一个Token。2个Token可表示1bit信息。

server 收到后，以 对应的 answer 回答，之后 提供client 一串新问题， 声称表示“相关问题”，
实际为 server 向 client 发送的数据。
client 不回答这些相关问题，而是继续 post.

不过，如果上传数据或者下载数据太长的话，就会成为明显的特征，因此如果数据比较长，需要进行
随机截断。

因此本协议对于一个tcp长连接，会依次产生多个 短连接。

因此在服务端，需要方法来判断一个请求到底是一个新请求还是某个旧请求的下一部分。
因此在中断处，会以一个中断标识来注明。若一个连接最后没有中断标识，那就是结束。

因此需要在首部进行标识。首部以32个Token(=16bit=2字节)表示本次请求的id;
每个id在一段时间内（如1小时内）只能用一次. 该id的连接结束后，同一客户端上的下一个id为上一个
id+1; 如此便可区别不同的客户端 以及 不同的请求连接。

同一id重新出现一次时，若之前接到过同id的中断标识，即表示这是该连接的下一个片段。而若没接到，则
说明该请求可能不来自我们的隐写协议，此时依然以与原来相同的方式反回 答案，以及随机的 “相关问题”。


目前暂未实现分段
*/

use std::io::Result;
use std::{
    collections::HashMap,
    fmt::Write,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use anyhow::bail;
use bytes::BytesMut;
use itertools::Itertools;
use macro_map::*;
use rand::Rng;
use ruci::net::http::HeaderContainer;
use ruci::{
    map::{self, MapParams, MapResult, ProxyBehavior},
    net::CID,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tracing::{trace, warn};
// use tracing::trace;

pub type QA = (String, String);
pub type Token = [QA; 128]; //每个 Token都有128种可能, 128个问答同时表示同一种信息

/// 选择的方式按 转移方阵中所指定的概率来
#[derive(Debug)]
pub struct QaData {
    //对应 0 和 1 的 问答 组合 各 128 个
    pub qa_set: [Token; 2],

    // 用于快速用String 查找对应 Question
    q_hash_map: [HashMap<String, u8>; 2],
    // 用于快速用String 查找对应 Answer
    a_hash_map: [HashMap<String, u8>; 2],

    // 首部id标识
    // head_marker_set: [Token; 32],

    // 中断标识
    // interrupt_set: Token,

    //所有的 QA的转移方阵，其维度为 qa_set中的 Vec的 长度.每行总和均为1
    //也可以不提供，不提供则所问的问题完全随机化
    pub transformation_matrix: Option<[Vec<Vec<f32>>; 2]>,
}

fn simple_qa(from: usize, plus: usize) -> QA {
    let n = from as u16 + plus as u16;
    (format!("question {n}"), format!("answer {n}"))
}

// fn simple_token(from: usize) -> Token {
//     let array: Token = array_init::array_init(|i| simple_qa(from, i));
//     array
// }

fn simple_token_vec(count: usize) -> Vec<Token> {
    let mut r = vec![];
    for i in 0..count {
        let from = i * 128;
        let array: Token = array_init::array_init(|ii| simple_qa(from, ii));
        r.push(array);
    }

    r
}

type BytesAndIndexVec = (BytesMut, Option<Vec<(bool, u8)>>);

impl QaData {
    pub fn new(qa_set: [Token; 2]) -> QaData {
        let q_hash_map: [HashMap<String, u8>; 2] = array_init::array_init(|i| {
            qa_set[i]
                .iter()
                .enumerate()
                .map(|(index, qa)| (qa.0.clone(), index as u8))
                .collect::<HashMap<String, u8>>()
        });

        let a_hash_map: [HashMap<String, u8>; 2] = array_init::array_init(|i| {
            qa_set[i]
                .iter()
                .enumerate()
                .map(|(index, qa)| (qa.1.clone(), index as u8))
                .collect::<HashMap<String, u8>>()
        });
        QaData {
            qa_set,
            q_hash_map,
            a_hash_map,
            // head_marker_set: <[Token; 32]>::try_from(simple_token_vec(32))
            //     .expect("Conversion failed"),
            // interrupt_set: simple_token(384),
            transformation_matrix: None,
        }
    }
    pub fn new_simple() -> QaData {
        let qa_set = <[Token; 2]>::try_from(simple_token_vec(2)).expect("Conversion failed");

        Self::new(qa_set)
    }

    /// 由给定的 question-answer 对 的Vec 来初始化 QaData.
    /// qas.len() 须为偶数。
    /// 若 qas.len() > 256, 则会truncate.
    /// 若 qas.len() < 256, 则会truncate到2的幂后分一半 duplicate 对应次数；
    ///
    /// 前一半的问答代表0， 后一半 代表1.
    /// 约定 每个question-answer的权重都是相等的，即问任一问题
    /// 的机会相等。
    ///
    pub fn from(mut qas: Vec<QA>) -> QaData {
        let len = qas.len();

        let qa_set = {
            match len.cmp(&256) {
                std::cmp::Ordering::Equal => {}
                std::cmp::Ordering::Greater => {
                    qas.truncate(256);
                }
                std::cmp::Ordering::Less => {
                    let target_n = nearest_power_of_two(len as u32) as usize;

                    qas.truncate(target_n);

                    let half = target_n / 2;

                    let mut first_part = qas.split_off(half);
                    let mut second_part = qas;

                    for _ in 1..128 / half {
                        first_part.extend_from_within(0..half);
                        second_part.extend_from_within(0..half);
                    }

                    assert_eq!(first_part.len(), second_part.len());
                    assert_eq!(first_part.len(), 128);

                    first_part.append(&mut second_part);

                    qas = first_part;
                }
            };
            let first_part = qas.split_off(128);
            let second_part = qas;

            [
                <Token>::try_from(first_part).expect("array from vec same size is ok"),
                <Token>::try_from(second_part).expect("array from vec same size is ok"),
            ]
        };

        Self::new(qa_set)
    }

    // select question randomly
    pub fn bytes_to_questions_text(&self, buf: &[u8]) -> String {
        let mut rng = rand::thread_rng();

        let mut s = String::new();

        buf.iter().for_each(|byte| {
            for i in 0..8 {
                let mask = 1 << (7 - i); // 从最高位到最低位
                let mut bit = byte & mask; // 检查位是否为 1
                if bit > 0 {
                    bit = 1;
                }

                let number = rng.gen_range(0..128);

                let qa = &self.qa_set[bit as usize][number];
                s.push_str(qa.0.as_str());
                s.push('\n');
            }
        });
        s.pop();
        s
    }

    // u8 表示 问题索引
    //true 代表1，false 代表0，错误代表 其不在 本QA表中。
    pub fn match_question(&self, s: &str) -> anyhow::Result<(bool, u8)> {
        let o = self.q_hash_map[0].get(s);
        match o {
            Some(i) => Ok((false, *i)),
            None => {
                let o = self.q_hash_map[1].get(s);
                match o {
                    Some(i) => Ok((true, *i)),
                    None => bail!("match_question err: there is no question, string is: {s}"),
                }
            }
        }
    }

    pub fn match_answer(&self, s: &str) -> anyhow::Result<(bool, u8)> {
        let o = self.a_hash_map[0].get(s);
        match o {
            Some(i) => Ok((false, *i)),
            None => {
                let o = self.a_hash_map[1].get(s);
                match o {
                    Some(i) => Ok((true, *i)),
                    None => bail!("there is no answer: {s}"),
                }
            }
        }
    }

    // 包含转换后的 数据 以及对应的 问题索引
    pub fn questions_to_bytes(
        &self,
        str: &str,
        with_answer_index: bool,
        has_header: bool,
    ) -> anyhow::Result<BytesAndIndexVec> {
        let lines: Vec<&str> = str.split('\n').collect();

        let mut bools: Vec<bool> = vec![];
        let mut ai: Vec<(bool, u8)> = vec![];

        let mut dealed_first = false;
        if !has_header {
            dealed_first = true;
        }

        for l in lines {
            if !dealed_first {
                if l != "Questions:" {
                    bail!("questions_to_bytes: no 'Questions:' header");
                }
                dealed_first = true;
            } else {
                match self.match_question(l) {
                    Ok(r) => {
                        bools.push(r.0);
                        if with_answer_index {
                            ai.push(r);
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        let bytes = bools_to_bytes(bools);

        let bm = BytesMut::from(bytes.as_slice());

        match with_answer_index {
            true => Ok((bm, Some(ai))),
            false => Ok((bm, None)),
        }
    }

    pub fn answers_to_bytes(&self, str: &str) -> anyhow::Result<BytesMut> {
        let lines: Vec<&str> = str.split('\n').collect();

        let mut bools: Vec<bool> = vec![];

        for l in lines {
            match self.match_answer(l) {
                Ok(r) => {
                    bools.push(r.0);
                }
                Err(e) => return Err(e),
            }
        }

        Ok(BytesMut::from(bools_to_bytes(bools).as_slice()))
    }
}

fn nearest_power_of_two(mut n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    n |= n >> 1;
    n |= n >> 2;
    n |= n >> 4;
    n |= n >> 8;
    n |= n >> 16;
    n - (n >> 1) // 返回小于或等于 n 的最大 2 的幂
}

// 大端序，即 vec![true] 会被转成 vec![128]
pub fn bools_to_bytes(bools: Vec<bool>) -> Vec<u8> {
    // bools
    //     .chunks(8) // 每 8 个布尔值分成一组
    //     .map(|chunk| {
    //         chunk.iter().enumerate().fold(0u8, |byte, (i, &b)| {
    //             if b {
    //                 byte | (1 << (7 - i)) // 将对应的比特位设置为 1
    //             } else {
    //                 byte // 不改变比特位
    //             }
    //         })
    //     })
    //     .collect()

    let mut result = Vec::new();
    let mut byte = 0u8;
    for (i, &b) in bools.iter().enumerate() {
        if b {
            byte |= 1 << (7 - (i % 8));
        }
        if i % 8 == 7 || i == bools.len() - 1 {
            result.push(byte);
            byte = 0;
        }
    }
    result
}

#[test]
fn q2t() {
    let x = QaData::new_simple();
    println!(
        "{}",
        x.bytes_to_questions_text(&[1, 4, 3, 2, 5, 7, 8, 33, 241])
    )
}

#[test]
fn b2b() {
    let x = vec![];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![false];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, false];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, false, true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, true, true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, true, true, true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, true, true, true, true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, true, true, true, true, true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, true, true, true, true, true, true];
    println!("{:?}", bools_to_bytes(x));
    let x = vec![true, true, true, true, true, true, true, true];
    println!("{:?}", bools_to_bytes(x));

    let x = vec![
        true, false, true, true, false, false, true, false, false, true, true, false, false, true,
        false, true,
    ];
    println!("{:?}", bools_to_bytes(x));
}

#[derive(Default)]
pub enum WriteState {
    #[default]
    ReadyForNew,
    Previous(usize),
}

#[derive(Default)]
pub enum ReadState {
    #[default]
    ReadyForNew,
    ContinueReadRemote(usize, usize, usize), //Content-Length, body_start_index, filled_data_len
    ContinueReadLocalCache(usize, usize),    // from, to
    Closed,
}

pub struct Conn {
    cid: CID,
    is_server: bool,
    hasnt_written: bool,

    // 缓存的 已解析后的 answer 的索引
    // bool 为 true 对应 1，为 false 对应 0
    server_cached_answers: Vec<(bool, u8)>,
    base: Pin<ruci::net::Conn>,
    qa: Arc<QaData>,

    read_cache: Option<BytesMut>,
    write_cache: Option<BytesMut>,

    write_state: WriteState,
    read_state: ReadState,
}

pub const READ_CAP: usize = 1024 * 1024;
const SERVER_QUESTION_PROMPT: &str = "\nYou can also ask these questions:\n";

fn get_pr_header_content_length<T: HeaderContainer>(pr: &T) -> Result<usize> {
    match pr.get_header("Content-Length") {
        Some(h) => {
            let clr: usize = h.value.parse().map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("content length 无法解析为整数 ,{e}"),
                )
            })?;
            Ok(clr)
        }
        None => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "no content length",
        )),
    }
}

impl Conn {
    fn server_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<()>> {
        let mut rc = self.as_mut().read_cache.take();
        if rc.is_none() {
            rc = Some(BytesMut::zeroed(READ_CAP));
        }
        let mut rc = rc.unwrap();

        match self.read_state {
            ReadState::Closed => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "closed",
            ))),

            ReadState::ReadyForNew => {
                unsafe {
                    rc.set_len(READ_CAP);
                }

                let mut rb = ReadBuf::new(&mut rc);
                match self.base.as_mut().poll_read(cx, &mut rb) {
                    Poll::Pending => {
                        let _ = self.read_cache.insert(rc);

                        trace!(cid=%self.cid, "spe1server read pending");

                        Poll::Pending
                    }
                    Poll::Ready(r) => match r {
                        Err(e) => {
                            let _ = self.read_cache.insert(rc);

                            trace!(cid=%self.cid, "spe1server read err");

                            Poll::Ready(Err(e))
                        }
                        Ok(_) => {
                            let l = rb.filled().len();
                            if l == 0 {
                                self.read_state = ReadState::Closed;

                                trace!(cid=%self.cid, "spe1server read ok empty(EOF), will close");

                                let _ = self.poll_shutdown(cx);

                                return Poll::Ready(Ok(()));
                            }

                            self.server_read_header(cx, rc, 0, l, buf)
                        }
                    },
                }
            }
            ReadState::ContinueReadRemote(content_len, body_start_index, filled_data_len) => {
                unsafe {
                    rc.set_len(READ_CAP);
                }
                let mut rb = ReadBuf::new(&mut rc);

                rb.set_filled(filled_data_len);

                match self.base.as_mut().poll_read(cx, &mut rb) {
                    Poll::Pending => {
                        let _ = self.read_cache.insert(rc);
                        Poll::Pending
                    }
                    Poll::Ready(r) => match r {
                        Err(e) => {
                            let _ = self.read_cache.insert(rc);
                            Poll::Ready(Err(e))
                        }
                        Ok(_) => {
                            let data = rb.filled();
                            let dl = data.len();

                            if dl == filled_data_len {
                                trace!("spe1server ContinueReadRemote got empty(EOF), will close");
                                self.read_state = ReadState::Closed;
                                let _ = self.poll_shutdown(cx);
                                return Poll::Ready(Ok(()));
                            }

                            let real_len = data[body_start_index..].len();

                            if real_len < content_len {
                                trace!(
                                    cid=%self.cid,
                                    "spe1server partial read2: {} {content_len}", real_len
                                );

                                self.read_state = ReadState::ContinueReadRemote(
                                    content_len,
                                    body_start_index,
                                    dl,
                                );

                                let _ = self.read_cache.insert(rc);

                                self.server_read(cx, buf)
                            } else {
                                if tracing::enabled!(tracing::Level::TRACE) {
                                    let real_data =
                                        &data[body_start_index..body_start_index + content_len];
                                    let real_string = String::from_utf8_lossy(real_data);

                                    trace!( cid=%self.cid,
                                        "spe1server partial read2 finish, {}, {}, {}",
                                        real_len,
                                        content_len,
                                        real_string.len()
                                    );
                                }

                                let r =
                                    self.server_real_read(body_start_index, content_len, buf, data);
                                let _ = self.read_cache.insert(rc);

                                if real_len > content_len {
                                    self.read_state = ReadState::ContinueReadLocalCache(
                                        body_start_index + content_len,
                                        dl,
                                    )
                                } else {
                                    self.read_state = ReadState::ReadyForNew;
                                }

                                r
                            }
                        }
                    },
                }
            }
            ReadState::ContinueReadLocalCache(from, to) => {
                trace!(cid=%self.cid,"ContinueReadLocalCache, {}, {}",from, to);

                self.server_read_header(cx, rc, from, to, buf)
            }
        }
    }

    fn server_read_header(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        rc: BytesMut,
        from: usize,
        to: usize,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<()>> {
        let data = &rc[from..to];

        let pr = ruci::net::http::parse_h1_request(data, false);

        let content_len = get_pr_header_content_length(&pr)?;
        let si = pr.body_start_index;

        let real_len = data[si..].len();

        match content_len.cmp(&real_len) {
            std::cmp::Ordering::Less => {
                let r = self.server_real_read(si, content_len, buf, data);

                self.read_state = ReadState::ContinueReadLocalCache(from + si + content_len, to);
                let _ = self.read_cache.insert(rc);

                r
            }
            std::cmp::Ordering::Equal => {
                trace!(cid=%self.cid, "spe1server read ok {content_len} {real_len}");

                let r = self.server_real_read(si, content_len, buf, data);
                let _ = self.read_cache.insert(rc);
                self.read_state = ReadState::ReadyForNew;
                r
            }
            std::cmp::Ordering::Greater => {
                trace!("spe1server partial read: {} {content_len}", real_len);

                let mut new_rc = BytesMut::with_capacity(READ_CAP);

                new_rc.extend_from_slice(data);

                self.read_state = ReadState::ContinueReadRemote(content_len, si, data.len());

                let _ = self.read_cache.insert(new_rc);

                self.server_read(cx, buf)
            }
        }
    }

    fn server_real_read(
        &mut self,
        si: usize,
        content_len: usize,

        buf: &mut ReadBuf<'_>,
        data: &[u8],
    ) -> Poll<Result<()>> {
        let real_data = &data[si..si + content_len];
        let real_string = String::from_utf8_lossy(real_data).to_string();

        self.read_state = ReadState::ReadyForNew;

        Poll::Ready(match self.qa.questions_to_bytes(&real_string, true, true) {
            Ok(bm) => {
                trace!(cid=%self.cid, "spe1server server_real_read {}, {}",bm.0.len(),&data[..si + content_len].len() );

                buf.put_slice(&bm.0);
                self.server_cached_answers = bm.1.unwrap();

                Ok(())
            }
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("spe1: question string to bytes err: {e}"),
            )),
        })
    }

    fn server_response_to_2_parts(s: &str) -> Option<(&str, &str)> {
        s.split_once(SERVER_QUESTION_PROMPT)
            .map(|parts| (parts.0, parts.1))
    }

    fn client_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<()>> {
        let mut rc = self.as_mut().read_cache.take();
        if rc.is_none() {
            rc = Some(BytesMut::zeroed(READ_CAP));
        }
        let mut rc = rc.unwrap();

        trace!("client_read called"); //buf size 一般为 8192

        match self.read_state {
            ReadState::Closed => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "closed",
            ))),
            ReadState::ReadyForNew => {
                unsafe {
                    rc.set_len(READ_CAP);
                }

                let mut rb = ReadBuf::new(&mut rc);
                match self.base.as_mut().poll_read(cx, &mut rb) {
                    Poll::Pending => {
                        let _ = self.read_cache.insert(rc);
                        Poll::Pending
                    }
                    Poll::Ready(r) => match r {
                        Err(e) => {
                            let _ = self.read_cache.insert(rc);
                            Poll::Ready(Err(e))
                        }
                        Ok(_) => {
                            let dl = rb.filled().len();
                            if dl == 0 {
                                self.read_state = ReadState::Closed;
                                let _ = self.poll_shutdown(cx);
                                return Poll::Ready(Ok(()));
                            }

                            self.client_read_header(cx, rc, 0, dl, buf)
                        }
                    },
                }
            }
            ReadState::ContinueReadRemote(content_len, body_start_index, filled_data_len) => {
                trace!(cid=%self.cid,"spe1client ContinueReadRemote called, {content_len}, {body_start_index}, {filled_data_len}");

                unsafe {
                    rc.set_len(READ_CAP);
                }
                let mut rb = ReadBuf::new(&mut rc);

                rb.set_filled(filled_data_len);
                let before_len = filled_data_len;

                match self.base.as_mut().poll_read(cx, &mut rb) {
                    Poll::Pending => {
                        let _ = self.read_cache.insert(rc);
                        trace!(cid=%self.cid,"spe1client ContinueReadRemote got pending");
                        Poll::Pending
                    }
                    Poll::Ready(r) => match r {
                        Err(e) => {
                            let _ = self.read_cache.insert(rc);
                            self.read_state = ReadState::ReadyForNew;
                            Poll::Ready(Err(e))
                        }
                        Ok(_) => {
                            let data = rb.filled();

                            let dl = data.len();

                            if dl == before_len {
                                self.read_state = ReadState::Closed;
                                let _ = self.poll_shutdown(cx);
                                return Poll::Ready(Ok(()));
                            }

                            let real_len = data[body_start_index..].len();

                            match content_len.cmp(&real_len) {
                                std::cmp::Ordering::Less => {
                                    trace!("client_read get more, {content_len}, {real_len}");
                                    let r = self.client_real_read(
                                        body_start_index,
                                        content_len,
                                        data,
                                        buf,
                                    );

                                    self.read_state = ReadState::ContinueReadLocalCache(
                                        body_start_index + content_len,
                                        dl,
                                    );
                                    let _ = self.read_cache.insert(rc);

                                    r
                                }
                                std::cmp::Ordering::Equal => {
                                    let r = self.client_real_read(
                                        body_start_index,
                                        content_len,
                                        data,
                                        buf,
                                    );

                                    self.read_state = ReadState::ReadyForNew;

                                    let _ = self.read_cache.insert(rc);

                                    r
                                }
                                std::cmp::Ordering::Greater => {
                                    trace!( cid=%self.cid,"spe1client partial read2: {real_len} {content_len}");

                                    self.read_state = ReadState::ContinueReadRemote(
                                        content_len,
                                        body_start_index,
                                        dl,
                                    );

                                    let _ = self.read_cache.insert(rc);

                                    self.client_read(cx, buf)
                                }
                            }
                        }
                    },
                }
            }
            ReadState::ContinueReadLocalCache(from, to) => {
                trace!(cid=%self.cid,"ContinueReadLocalCache, {from}, {to}");

                self.client_read_header(cx, rc, from, to, buf)
            }
        }
    }

    fn client_read_header(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        rc: BytesMut,
        from: usize,
        to: usize,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<()>> {
        let data = &rc[from..to];

        trace!("client_read_header called");
        let pr = ruci::net::http::parse_h1_response(data);
        match pr.parse_result {
            Err(e) => {
                let io_e = std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("spe1: parse http1.1 response err: {e:?}"),
                );

                let _ = self.read_cache.insert(rc);
                Poll::Ready(Err(io_e))
            }
            Ok(_) => {
                let content_len = get_pr_header_content_length(&pr)?;
                let si = pr.body_start_index;

                let real_len = data[si..].len();

                trace!(
                    "client_read_header got header content_len {}, real_len {}",
                    content_len,
                    real_len
                );

                if real_len < content_len {
                    trace!( cid=%self.cid,"spe1client partial read: {real_len} {content_len}");

                    let mut new_rc = BytesMut::with_capacity(READ_CAP);

                    new_rc.extend_from_slice(data);

                    self.read_state = ReadState::ContinueReadRemote(content_len, si, data.len());

                    let _ = self.read_cache.insert(new_rc);

                    self.client_read(cx, buf)
                } else {
                    let r = self.client_real_read(si, content_len, data, buf);

                    if real_len > content_len {
                        self.read_state =
                            ReadState::ContinueReadLocalCache(from + si + content_len, to);
                    } else {
                        self.read_state = ReadState::ReadyForNew;
                    }
                    let _ = self.read_cache.insert(rc);

                    r
                }
            }
        }
    }

    fn client_real_read(
        &mut self,
        si: usize,
        content_len: usize,
        data: &[u8],
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<()>> {
        let real_data = &data[si..si + content_len];
        let real_string = String::from_utf8_lossy(real_data);

        trace!( cid=%self.cid,"spe1client client_real_read called ");

        match Self::server_response_to_2_parts(&real_string) {
            None => {
                warn!("server_response_to_2_parts failed");
                Poll::Ready(Ok(()))
            }

            Some(answers_questions) => {
                //舍弃 part1, 只使用 part2

                match self
                    .qa
                    .questions_to_bytes(answers_questions.1, false, false)
                {
                    Ok(bm) => {
                        buf.put_slice(&bm.0);
                        trace!( cid=%self.cid,"spe1client client_real_read put_slice {} ",bm.0.len());

                        Poll::Ready(Ok(()))
                    }
                    Err(e) => {
                        let io_e = std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("client_real_read: questions string to bytes err: {e}"),
                        );

                        Poll::Ready(Err(io_e))
                    }
                }
            }
        }
    }
}

impl AsyncRead for Conn {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<()>> {
        if self.is_server {
            self.server_read(cx, buf)
        } else {
            self.client_read(cx, buf)
        }
    }
}
impl AsyncWrite for Conn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize>> {
        if matches!(self.read_state, ReadState::Closed) {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "spe1: writing when read end closed",
            )));
        }

        // if self.is_server || !self.hasnt_written {
        //     //一次最多只传送 少数 字节，以防出问题
        //     // 客户端的第一次传送除外
        //     if buf.len() > 1500 {
        //         buf = &buf[..1500];
        //     }
        // }

        if self.is_server {
            match self.write_state {
                WriteState::ReadyForNew => {
                    trace!("server ready write called {}", buf.len());
                    let mut content_buf = BytesMut::with_capacity(READ_CAP);
                    let s = self
                        .server_cached_answers
                        .iter()
                        .map(|pair| match pair.0 {
                            true => self.qa.qa_set[1][pair.1 as usize].1.as_str(),
                            false => self.qa.qa_set[0][pair.1 as usize].1.as_str(),
                        })
                        .join("\n");

                    content_buf.extend_from_slice(s.as_bytes());

                    let _ = content_buf.write_str(SERVER_QUESTION_PROMPT);

                    let s = self.qa.bytes_to_questions_text(buf);
                    content_buf.extend_from_slice(s.as_bytes());

                    let mut write_cache = self.write_cache.take().unwrap();

                    if write_cache.capacity() < READ_CAP {
                        write_cache.resize(READ_CAP, 0);
                    }
                    write_cache.clear();

                    trace!(
                        "server ready will write with content_len {}",
                        content_buf.len()
                    );

                    let _ =write_cache.write_str("http/1.1 200 OK\r\nConnection: keep-alive\r\nContent-Type: text/plain\r\nContent-Length: ");
                    let _ = write_cache.write_str(&content_buf.len().to_string());
                    let _ = write_cache.write_str("\r\n\r\n");

                    write_cache.extend_from_slice(&content_buf[..]);

                    let wl = write_cache.len();

                    match self.base.as_mut().poll_write(cx, &write_cache[..]) {
                        Poll::Pending => {
                            let _ = self.write_cache.insert(write_cache);

                            Poll::Pending
                        }

                        Poll::Ready(r) => match r {
                            Err(e) => {
                                let _ = self.write_cache.insert(write_cache);

                                Poll::Ready(Err(e))
                            }

                            Ok(n) => match n.cmp(&wl) {
                                std::cmp::Ordering::Less => {
                                    trace!( cid=%self.cid,
                                        "spe1server partial write {n}, {}, {}",
                                        wl, buf.len()
                                    );

                                    self.write_state = WriteState::Previous(n);
                                    let _ = self.write_cache.insert(write_cache);

                                    self.poll_write(cx, buf)
                                }
                                std::cmp::Ordering::Equal => {
                                    let _ = self.write_cache.insert(write_cache);

                                    trace!( cid=%self.cid,"spe1server  write ok {n}, {}",buf.len());

                                    Poll::Ready(Ok(buf.len()))
                                }
                                std::cmp::Ordering::Greater => {
                                    let cl = write_cache.len();
                                    let _ = self.write_cache.insert(write_cache);

                                    Poll::Ready(Err(io::Error::other(format!(
                                        "write len not right, {n}, {cl}",
                                    ))))
                                }
                            },
                        },
                    }
                }
                WriteState::Previous(head) => {
                    let write_cache = self.write_cache.take().unwrap();

                    let write_data = &write_cache[head..];

                    match self.base.as_mut().poll_write(cx, write_data) {
                        Poll::Pending => {
                            let _ = self.write_cache.insert(write_cache);

                            trace!( cid=%self.cid,
                                "spe1server partial write2  continue got pending, {}",buf.len(),
                            );
                            Poll::Pending
                        }

                        Poll::Ready(r) => match r {
                            Err(e) => {
                                self.write_state = WriteState::ReadyForNew;
                                let _ = self.write_cache.insert(write_cache);

                                Poll::Ready(Err(e))
                            }
                            Ok(n) => match n.cmp(&write_data.len()) {
                                std::cmp::Ordering::Less => {
                                    trace!( cid=%self.cid,
                                        "spe1server partial write2 {head} {n}, {}, {}",
                                        write_cache.len(),buf.len()
                                    );

                                    let _ = self.write_cache.insert(write_cache);

                                    self.write_state = WriteState::Previous(head + n);

                                    self.poll_write(cx, buf)
                                }
                                std::cmp::Ordering::Equal => {
                                    let _ = self.write_cache.insert(write_cache);
                                    self.write_state = WriteState::ReadyForNew;

                                    trace!( cid=%self.cid,"spe1server partial write2 finish, {}",buf.len());

                                    Poll::Ready(Ok(buf.len()))
                                }
                                std::cmp::Ordering::Greater => {
                                    let cl = write_cache.len();
                                    let _ = self.write_cache.insert(write_cache);
                                    self.write_state = WriteState::ReadyForNew;

                                    Poll::Ready(Err(io::Error::other(format!(
                                        "write len not right, {n}, {cl}",
                                    ))))
                                }
                            },
                        },
                    }
                }
            }
        } else {
            // is client
            match self.write_state {
                WriteState::ReadyForNew => {
                    let mut content_buf = BytesMut::new();
                    let _ = content_buf.write_str("Questions:\n");
                    let s = self.qa.bytes_to_questions_text(buf);
                    content_buf.extend_from_slice(s.as_bytes());

                    let mut write_cache = self.write_cache.take().unwrap();

                    if write_cache.capacity() < READ_CAP {
                        write_cache.resize(READ_CAP, 0);
                    }
                    write_cache.clear();

                    let content_len = content_buf.len();

                    let _ = write_cache.write_str("POST /ask HTTP/1.1\r\nHost: httpbin.org\r\nConnection: keep-alive\r\nContent-Length: ");
                    let _ = write_cache.write_str(&content_len.to_string());
                    let _ = write_cache.write_str("\r\nContent-Type: text/plain\r\n\r\n");

                    write_cache.extend_from_slice(&content_buf[..]);

                    let r = self.base.as_mut().poll_write(cx, &write_cache[..]);

                    let wcl = write_cache.len();

                    match r {
                        Poll::Pending => {
                            let _ = self.write_cache.insert(write_cache);

                            trace!(cid=%self.cid, "spe1client write pending");
                            Poll::Pending
                        }

                        Poll::Ready(r) => match r {
                            Err(e) => {
                                let _ = self.write_cache.insert(write_cache);
                                trace!(cid=%self.cid, "spe1client write err");

                                Poll::Ready(Err(e))
                            }

                            Ok(n) => {
                                if n == wcl {
                                    let _ = self.write_cache.insert(write_cache);

                                    trace!(cid=%self.cid, "spe1client write ok, {}, {n}, {content_len}",buf.len());

                                    self.hasnt_written = false;

                                    Poll::Ready(Ok(buf.len()))
                                } else {
                                    let _ = self.write_cache.insert(write_cache);

                                    self.write_state = WriteState::Previous(n);

                                    self.poll_write(cx, buf)
                                }
                            }
                        },
                    }
                }
                WriteState::Previous(head) => {
                    let write_cache = self.write_cache.take().unwrap();

                    match self.base.as_mut().poll_write(cx, &write_cache[head..]) {
                        Poll::Pending => {
                            let _ = self.write_cache.insert(write_cache);

                            Poll::Pending
                        }

                        Poll::Ready(r) => match r {
                            Err(e) => {
                                self.write_state = WriteState::ReadyForNew;
                                let _ = self.write_cache.insert(write_cache);

                                Poll::Ready(Err(e))
                            }
                            Ok(n) => match n.cmp(&write_cache[head..].len()) {
                                std::cmp::Ordering::Less => {
                                    trace!( cid=%self.cid,
                                        "spe1client partial write2 {n}, {}",
                                        write_cache.len()
                                    );

                                    self.write_state = WriteState::Previous(n + head);
                                    let _ = self.write_cache.insert(write_cache);

                                    self.poll_write(cx, buf)
                                }
                                std::cmp::Ordering::Equal => {
                                    let _ = self.write_cache.insert(write_cache);
                                    self.write_state = WriteState::ReadyForNew;

                                    trace!( cid=%self.cid,"spe1client partial write2 finish");

                                    Poll::Ready(Ok(buf.len()))
                                }
                                std::cmp::Ordering::Greater => {
                                    let cl = write_cache.len();
                                    let _ = self.write_cache.insert(write_cache);
                                    self.write_state = WriteState::ReadyForNew;

                                    Poll::Ready(Err(io::Error::other(format!(
                                        "write len not right, {n}, {cl}",
                                    ))))
                                }
                            },
                        },
                    }
                }
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        self.base.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        self.base.as_mut().poll_shutdown(cx)
    }
}

#[map_ext_fields]
#[derive(Debug, Clone, MapExt)]
pub struct ClientOrServer {
    pub qa: Arc<QaData>,
    pub is_server: bool,
}

impl ruci::Name for ClientOrServer {
    fn name(&self) -> &'static str {
        "spe1"
    }
}
#[async_trait::async_trait]
impl map::Map for ClientOrServer {
    async fn maps(&self, cid: CID, _behavior: ProxyBehavior, params: MapParams) -> MapResult {
        match params.c {
            ruci::net::Stream::Conn(base) => {
                let c = Conn {
                    hasnt_written: true,
                    cid,
                    qa: self.qa.clone(),
                    is_server: self.is_server,
                    server_cached_answers: vec![],
                    base: Box::pin(base),
                    read_cache: None,
                    write_cache: Some(BytesMut::with_capacity(READ_CAP)),
                    write_state: WriteState::default(),
                    read_state: ReadState::default(),
                };

                return MapResult::new_c(Box::new(c))
                    .a(params.a)
                    .b(params.b)
                    .build();
            }
            _ => MapResult::err_str("spe1 only support tcplike stream"),
        }
    }
}
