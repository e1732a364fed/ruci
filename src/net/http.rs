/*!

移植verysimple 的 httpLayer/h1_requestfilter.go

verysimple 上的实现有个大问题, iota 不是 enum , 导致函数返回的数可能有不同含义.
rust上就好了

*/

const MAX_PARSE_URL_LEN: usize = 3000;
const HEADER_ENDING_BYTES: &[u8] = b"\r\n\r\n";
const HEADER_ENDING_BYTES_LEN: usize = HEADER_ENDING_BYTES.len();

#[derive(Default, Debug)]
pub struct ParsedHttpRequest {
    pub version: String,
    pub method: Method,
    pub path: String,
    pub headers: Vec<RawHeader>,
    pub fail_reason: FailReason,
    pub last_checked_index: usize,
}

#[derive(PartialEq, Debug, Default)]
pub enum FailReason {
    #[default]
    None,
    TooShort,
    NotForH2c,
    MethodLenWrong,
    UnexpectedProxy,
    SpaceIndexWrong,
    Is11ProxyButNot11,
    NoSlash,
    EarlyLinefeed,
    FirstlineLessThan10,
    StrHttpNotFoundInRightPlace,
    NoEndMark,
    NoEndMark2,
    HeaderNoColon,
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

#[derive(Debug)]
pub struct RawHeader {
    pub head: String,
    pub value: String,
}

///  https://stackoverflow.com/questions/25047905/http-request-minimum-size-in-bytes/25065089
///
///minimum valid request:
///
///```
/// GET / HTTP/1.1<CR><LF>
/// Host:x<CR><LF>
/// <CR><LF>
///```
pub fn parse_h1_request(bs: &[u8], is_proxy: bool) -> ParsedHttpRequest {
    let mut request = ParsedHttpRequest::default();
    request.fail_reason = FailReason::TooShort;

    if bs.len() < 16 {
        return request;
    }

    if bs[4] == b'*' {
        request.fail_reason = FailReason::NotForH2c;
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
                    request.fail_reason = FailReason::UnexpectedProxy;
                    return request;
                }
            }
        }
        _ => {}
    }

    if should_space_index == 0 || bs[should_space_index] != b' ' {
        request.fail_reason = FailReason::SpaceIndexWrong;
        return request;
    }

    let should_slash_index = should_space_index + 1;

    if is_proxy {
        if request.method == Method::CONNECT {
            //https
        } else {
            //http
            if bs[should_slash_index..should_slash_index + 7] != b"http://"[..] {
                request.fail_reason = FailReason::Is11ProxyButNot11;
                return request;
            }
        }
    } else {
        if bs[should_slash_index] != b'/' {
            request.fail_reason = FailReason::NoSlash;
            return request;
        }
    }

    //一般请求样式类似 GET /sdfdsffs.html HTTP/1.1
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
            request.fail_reason = FailReason::EarlyLinefeed;
            return request;
        }
        if b == b' ' {
            // 空格后面至少还有 HTTP/1.1\r\n 这种字样，也就是说空格后长度至少为 10

            if bs.len() - i - 1 < 10 {
                request.fail_reason = FailReason::FirstlineLessThan10;
                return request;
            }

            request.path = String::from_utf8_lossy(&bs[should_slash_index..i]).to_string();

            if &bs[i + 1..i + 5] != b"HTTP" {
                request.fail_reason = FailReason::StrHttpNotFoundInRightPlace;
                return request;
            }
            request.version = String::from_utf8_lossy(&bs[i + 6..i + 9]).to_string();
            if bs[i + 9] != b'\r' || bs[i + 10] != b'\n' {
                request.fail_reason = FailReason::NoEndMark;
                return request;
            }

            let left_bs = &bs[i + 11..];

            if let Some(index_of_ending) = left_bs
                .windows(HEADER_ENDING_BYTES_LEN)
                .position(|x| x == HEADER_ENDING_BYTES)
            {
                let header_bytes = &left_bs[..index_of_ending];

                let header_string = String::from_utf8_lossy(header_bytes);
                let header_str_list: Vec<&str> = header_string.split("\r\n\r\n").collect();

                for header in header_str_list {
                    let hs = header.to_string();
                    let ss: Vec<&str> = hs.splitn(2, ":").collect();

                    if ss.len() != 2 {
                        request.fail_reason = FailReason::HeaderNoColon;
                        return request;
                    }
                    request.headers.push(RawHeader {
                        head: ss[0].to_string(),
                        value: ss[1].to_string(),
                    });
                    return request;
                }
            } else {
                request.fail_reason = FailReason::NoEndMark2;
                return request;
            }
        }
    }
    request.last_checked_index = last;
    request
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_too_short() {
        let request = parse_h1_request(b"HTTP/", false);
        assert_eq!(request.fail_reason, FailReason::TooShort);

        let request = parse_h1_request(b"GETHTTP/", false);
        assert_eq!(request.fail_reason, FailReason::TooShort);

        let request = parse_h1_request(b"GET HTTP/1.1\r\n", false);
        assert_eq!(request.fail_reason, FailReason::TooShort);
    }

    #[test]
    fn test_invalid_no_end_mark2() {
        let request = parse_h1_request(b"GET / HTTP/1.1\r\n", false);
        assert_eq!(request.fail_reason, FailReason::NoEndMark2);

        let request = parse_h1_request(b"GET / HTTP/1.1\r\nHeader: Value", false);
        assert_eq!(request.fail_reason, FailReason::TooShort);

        let request = parse_h1_request(b"GET / HTTP/1.1\r\nHeader: Value\r\n", false);
        assert_eq!(request.fail_reason, FailReason::TooShort);
    }

    #[test]
    fn test_invalid_no_end_mark() {
        let request = parse_h1_request(b"GET / HTTPX/1.1\r\n", false);
        assert_eq!(request.fail_reason, FailReason::NoEndMark);
    }

    #[test]
    fn test_invalid_header_no_colon() {
        let request = parse_h1_request(b"GET / HTTP/1.1\r\nHeaderValue\r\n\r\n", false);
        assert_eq!(request.fail_reason, FailReason::HeaderNoColon);
    }

    #[test]
    fn test_invalid_unexpected_proxy() {
        let request = parse_h1_request(b"CONNECT example.com:80 HTTP/1.1\r\n\r\n", false);
        assert_eq!(request.fail_reason, FailReason::UnexpectedProxy);
    }

    #[test]
    fn test_valid_request() {
        let request = parse_h1_request(b"GET / HTTP/1.1\r\nHost:x\r\n\r\n", false);
        assert_eq!(request.fail_reason, FailReason::TooShort);
    }
}
