/*!
provide facilities to filter http1.1

See https://datatracker.ietf.org/doc/html/rfc2616

移植verysimple 的 httpLayer/h1_requestfilter.go

verysimple 上的实现有个大问题, iota 不是 enum , 导致函数返回的数可能有不同含义.
rust上用enum就好了

*/

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// used by various Mappers in ruci that has a http layer
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommonConfig {
    pub host: String,
    pub path: String,
    pub headers: Option<BTreeMap<String, String>>,

    pub use_early_data: Option<bool>,
    pub can_fallback: Option<bool>,
}

pub const MAX_PARSE_URL_LEN: usize = 3000;
const HEADER_ENDING_BYTES: &[u8] = b"\r\n\r\n";
pub const HEADER_ENDING_STR: &str = "\r\n\r\n";
const HEADER_SPLIT_STR: &str = "\r\n";
const HEADER_ENDING_BYTES_LEN: usize = HEADER_ENDING_BYTES.len();

#[derive(Debug)]
pub struct Header {
    pub head: String,
    pub value: String,
}

#[derive(Debug)]
pub struct ParsedHttpRequest {
    pub version: String,
    pub method: Method,
    pub path: String,
    pub headers: Vec<Header>,
    pub parse_result: Result<(), ParseError>,
    pub last_checked_index: usize,
}
impl Default for ParsedHttpRequest {
    fn default() -> Self {
        Self {
            version: Default::default(),
            method: Default::default(),
            path: Default::default(),
            headers: Default::default(),
            parse_result: Ok(()),
            last_checked_index: Default::default(),
        }
    }
}

impl ParsedHttpRequest {
    pub fn get_first_header_by(&self, key: &str) -> &str {
        for h in &self.headers {
            if h.head == key {
                return &h.value;
            }
        }
        ""
    }
}

#[derive(PartialEq, Debug)]
pub enum ParseError {
    TooShort,
    NotForH2c,
    MethodLenWrong,
    UnexpectedProxy,
    SpaceIndexWrong,
    ExpectCONNECTButNot,
    NoSlash,
    EarlyLinefeed,
    FirstLineLessThan10,
    StrHttpNotFoundInRightPlace,
    NoEndMark,
    NoEndMark2,
    HeaderNoColonOrColonNotFollowedBySpace,
}

pub const FAIL_NO_END_MARK: i32 = -12;

#[derive(Default, PartialEq, Debug)]
pub enum Method {
    #[default]
    Unspecified,

    GET,
    PUT,
    POST,
    HEAD,
    DELETE,
    OPTIONS,
    CONNECT,
    Other(String),
}

///  <https://stackoverflow.com/questions/25047905/http-request-minimum-size-in-bytes/25065089>
///
///minimum valid request:
///
///```plaintext
/// GET / HTTP/1.1<CR><LF>
/// Host:x<CR><LF>
/// <CR><LF>
///```
pub fn parse_h1_request(bs: &[u8], is_proxy: bool) -> ParsedHttpRequest {
    let mut request = ParsedHttpRequest::default();

    if bs.len() < 16 {
        request.parse_result = Err(ParseError::TooShort);
        return request;
    }

    if bs[4] == b'*' {
        request.parse_result = Err(ParseError::NotForH2c);
        return request;
    }

    let mut should_space_index = 0;

    match bs[0] {
        b'G' => {
            if bs[1..=2] == b"ET"[..] {
                request.method = Method::GET;
                should_space_index = 3;
            }
        }
        b'P' => {
            if bs[1..=2] == b"UT"[..] {
                request.method = Method::PUT;
                should_space_index = 3;
            } else if bs[1..=3] == b"OST"[..] {
                request.method = Method::POST;
                should_space_index = 4;
            }
        }
        b'H' => {
            if bs[1..=3] == b"EAD"[..] {
                request.method = Method::HEAD;
                should_space_index = 4;
            }
        }
        b'D' => {
            if bs[1..=5] == b"ELETE"[..] {
                request.method = Method::DELETE;
                should_space_index = 6;
            }
        }
        b'O' => {
            if bs[1..=6] == b"PTIONS"[..] {
                request.method = Method::OPTIONS;
                should_space_index = 7;
            }
        }
        b'C' => {
            if bs[1..=6] == b"ONNECT"[..] {
                request.method = Method::CONNECT;
                should_space_index = 7;

                if !is_proxy {
                    request.parse_result = Err(ParseError::UnexpectedProxy);
                    return request;
                }
            }
        }
        _ => {}
    }

    if should_space_index == 0 || bs[should_space_index] != b' ' {
        request.parse_result = Err(ParseError::SpaceIndexWrong);
        return request;
    }

    let should_slash_index = should_space_index + 1;

    if is_proxy {
        if request.method == Method::CONNECT {
            //https
        } else {
            //http
            if bs[should_slash_index..should_slash_index + 7] != b"http://"[..] {
                request.parse_result = Err(ParseError::ExpectCONNECTButNot);
                return request;
            }
        }
    } else if bs[should_slash_index] != b'/' {
        request.parse_result = Err(ParseError::NoSlash);
        return request;
    }

    //一般请求样式类似 GET /some_path.html HTTP/1.1
    //所以找到第二个空格的位置即可，

    let mut last = bs.len();
    if !is_proxy {
        //如果是代理，则我们要判断整个请求，不能漏掉任何部分
        if last > MAX_PARSE_URL_LEN {
            last = MAX_PARSE_URL_LEN
        }
    }

    for i in should_slash_index..last {
        let b = bs[i];
        if b == b'\r' || b == b'\n' {
            request.parse_result = Err(ParseError::EarlyLinefeed);
            return request;
        }
        if b == b' ' {
            // 空格后面至少还有 HTTP/1.1\r\n 这种字样，也就是说空格后长度至少为 10

            if bs.len() - i - 1 < 10 {
                request.parse_result = Err(ParseError::FirstLineLessThan10);
                return request;
            }

            request.path = String::from_utf8_lossy(&bs[should_slash_index..i]).to_string();

            if &bs[i + 1..i + 5] != b"HTTP" {
                request.parse_result = Err(ParseError::StrHttpNotFoundInRightPlace);
                return request;
            }
            request.version = String::from_utf8_lossy(&bs[i + 6..i + 9]).to_string();
            if bs[i + 9] != b'\r' || bs[i + 10] != b'\n' {
                request.parse_result = Err(ParseError::NoEndMark);
                return request;
            }

            let left_bs = &bs[i + 11..];

            if let Some(index_of_ending) = left_bs
                .windows(HEADER_ENDING_BYTES_LEN)
                .position(|x| x == HEADER_ENDING_BYTES)
            {
                let header_bytes = &left_bs[..index_of_ending];

                let header_string = String::from_utf8_lossy(header_bytes);

                let header_str_list: Vec<&str> = header_string.split(HEADER_SPLIT_STR).collect();

                for header in header_str_list {
                    let hs = header.to_string();
                    let ss: Vec<&str> = hs.splitn(2, ": ").collect();

                    if ss.len() != 2 {
                        request.parse_result =
                            Err(ParseError::HeaderNoColonOrColonNotFollowedBySpace);
                        return request;
                    }
                    request.headers.push(Header {
                        head: ss[0].to_string(),
                        value: ss[1].to_string(),
                    });
                }
            } else {
                request.parse_result = Err(ParseError::NoEndMark2);
            }

            return request;
        } //b = ' '
    }
    request.last_checked_index = last;
    request
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split() {
        let header_string = "1234\r\n\r\n2345\r\n\r\n7890\n\r\n\r1234\n\n\r\raabb".to_string();
        let header_str_list: Vec<&str> = header_string.split("\r\n\r\n").collect();
        println!("{:?}", header_str_list);

        assert_eq!(header_str_list.len(), 3);

        unsafe {
            assert_eq!(
                std::str::from_utf8_unchecked(HEADER_ENDING_BYTES),
                HEADER_ENDING_STR
            );
        }
    }

    #[test]
    fn test_invalid_too_short() {
        let request = parse_h1_request(b"HTTP/", false);
        assert_eq!(request.parse_result, Err(ParseError::TooShort));

        let request = parse_h1_request(b"GETHTTP/", false);
        assert_eq!(request.parse_result, Err(ParseError::TooShort));

        let request = parse_h1_request(b"GET HTTP/1.1\r\n", false);
        assert_eq!(request.parse_result, Err(ParseError::TooShort));
    }

    #[test]
    fn test_invalid_no_end_mark2() {
        let request = parse_h1_request(b"GET / HTTP/1.1\r\n", false);
        assert_eq!(request.parse_result, Err(ParseError::NoEndMark2));

        let request = parse_h1_request(b"GET / HTTP/1.1\r\nHeader: Value", false);
        assert_eq!(request.parse_result, Err(ParseError::NoEndMark2));

        let request = parse_h1_request(b"GET / HTTP/1.1\r\nHeader: Value\r\n", false);
        assert_eq!(request.parse_result, Err(ParseError::NoEndMark2));
    }

    #[test]
    fn test_invalid_no_end_mark() {
        let request = parse_h1_request(b"GET / HTTPX/1.1\r\n", false);
        assert_eq!(request.parse_result, Err(ParseError::NoEndMark));
    }

    #[test]
    fn test_invalid_header_no_colon() {
        let request = parse_h1_request(b"GET / HTTP/1.1\r\nHeaderValue\r\n\r\n", false);
        assert_eq!(
            request.parse_result,
            Err(ParseError::HeaderNoColonOrColonNotFollowedBySpace)
        );
    }

    #[test]
    fn test_invalid_unexpected_proxy() {
        let request = parse_h1_request(b"CONNECT example.com:80 HTTP/1.1\r\n\r\n", false);
        assert_eq!(request.parse_result, Err(ParseError::UnexpectedProxy));
    }

    #[test]
    fn test_valid_request() {
        let request = parse_h1_request(b"GET / HTTP/1.1\r\nHost:x\r\n\r\n", false);
        assert_ne!(request.parse_result, Ok(()));

        let request = parse_h1_request(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n", false);
        assert_eq!(request.parse_result, Ok(()));
    }
}
