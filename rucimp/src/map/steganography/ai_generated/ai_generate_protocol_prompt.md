We need to design a steganography protocol mimicking http protocol that can be used to send data between two endpoints(client and server).

# **Requirements**

The final implementation language is lua.

## **General Steganography Principle**

Steganography protocol for each data packet, generates a write sequence and a read sequence.

That is, for a write w, generate a read-write sequence (w1, r1, w2, r2, ..., wn, rn).
For a read r, generate a read-write sequence (r1, w1, r2, w2, ..., rn, wn).

For a write w, and the generated read-write sequence (w1, r1, w2, r2, ..., wn, rn), only the w_* are the real data. 
The r_* are the returned dummy data from the other side, which is used for steganography.

The logic for the read r is similar, but the r_* are the real data, and the w_* are the returned dummy data.

As the dummy data is not used, the actual algorithm should only returns the lengths of each dummy data, without generating the content of the dummy data.

The length of the dummy data will be used to check if the protocol is working correctly.

For example, for a write w, if the algorithm returns ( abc, 120, def, 132), the actual data will be "abc" and "def", and the dummy data lengths are 120 and 132. 
And in the real transmission, the client will send "abc" to the server, and the server will send any packet with length 120 to the client. And then the client will send "def" to the server, and the server will send any packet with length 132 to the client.

This principle is applied to both the handshake phase and the data transmission phase. 

This progress formula is crucial to the protocol, so please remember it and make sure that you fully understand it.


## **Example For HTTP Steganography**
Because we will be targeting the http protocol, the read-write sequence should be able to be encoded in the http request and response.

So if the client send a request that contains the covert data "hello", the client should not send it in a single request, but send it in a sequence of requests.

Like this:

```
w1: GET / HTTP/1.1 h
r1: 200 OK
w2: GET / HTTP/1.1 e
r2: 200 OK
w3: GET / HTTP/1.1 l
r3: 200 OK
w4: GET / HTTP/1.1 l
r4: 200 OK
w5: GET / HTTP/1.1 o
r5: 200 OK
```

This sequence is in fact (w1, r1, w2, r2, w3, r3, w4, r4, w5, r5),and the r_* are in fact the lengths of the dummy data, which is the length of the string `200 OK`, which is 6.
So the real sequence is (w1, 6, w2, 6, w3, 6, w4, 6, w5, 6), with the w_* being the `GET / HTTP/1.1 *`.

Remember, any request should be split into multiple httprequests, and the server should be able to decode the request into the original data.

Do not use a single html body that contains the splitted data in several tags. Use entierly new http requests to carray the splitted data.

Also, do not treat the covert data as a string, but treat it as an arbitrary binary data.

## **Use Cases**
- Covert communication in censored networks.
- Hiding sensitive information to bypass network monitoring.
- Secure data transmission over public networks without raising suspicion.
- Real-World Scenarios:
Example 1: A journalist communicating from a censored country.
Example 2: A whistleblower sending sensitive documents.

## **Steganography in HTTP Body**
- Most content of the http body should be of the html format. And the html content should be a simple page with a lot of realistic text.
- MIME types can be utilized. For example, the content-type of the http response can be text/html. And if the content-type is json, the body should be a json object. If the content-type is image/jpeg, the body should be a real jpeg image.
- All contents carries hidden data using steganography techniques, so that they looks like normal http traffic, but actually carries hidden data.
- There's no need to implement error correction or encryption mechanisms, because if we want error correction, we wrap this protocol with tls protocol.
- If you are using http2, use http2 plaintext mode.
- Multi-Layer Steganography encoding. This involves embedding steganographic content at multiple protocol layers. Consider embedding hidden data simultaneously in  both headers, cookies, and query parameters to increase stealth.
- **HTTP Headers Steganography**: Embed hidden data in custom but not sensitive headers (e.g., `custom-header`), cookies, and query parameters.
- **HTML Comment Steganography**: Use invisible HTML comments to carry hidden data.
- **Data Sharding**: Split hidden data into multiple smaller chunks and embed them across multiple http requests and responses.
- **Dynamic Encoding**: Randomly switch between different encoding schemes (e.g., base64, hex, URL encoding) to increase stealthiness.

### **Important Note**

#### **Do not fill only in headers**
Do not fill the hidden data only inside the headers, because that's too easy to be detected by DPI systems.
Most data should be hidden in the http body, and the http body should be of some mime types.

This requirement is also applied to the handshake phase.

#### **Avoid Sensitive Words**
Any headers or any html tags shoudn't contain texts like "hidden-data","Stego", "stego-data", "stego-response", etc. Because that's
too sensitive to be detected by DPI systems. Completely mimic the real daily html content.

#### **Do not present the target address in plaintext**
Presenting the target address in plaintext is sensitive and should be avoided.  Use steganography techniques to hide the target address.

#### **Header Realism**
Use actual realtime date/time in the http headers, and the date/time should be in the format of "Mon, 07 Jan 2025 12:00:00 GMT".

Use a real user agent in the http headers, and the user agent should be in the format of "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36".

Use a real referer in the http headers, and the referer should be in the format of "https://www.google.com".

Use a real accept-language in the http headers, and the accept-language should be in the format of "en-US".

Use a real accept-encoding in the http headers, and the accept-encoding should be in the format of "gzip, deflate, br".

Use a real connection in the http headers, and the connection should be in the format of "keep-alive".

Use a real host in the http headers, and the host should be in the format of "www.google.com".

Use a real accept in the http headers, and the accept should be in the format of "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.9".

Use a real accept-charset in the http headers, and the accept-charset should be in the format of "utf-8".

## **Example Steganographic Techniques per MIME Type**

These are some examples of steganographic techniques for different MIME types, but you can use other techniques.

- text/html: Text watermarking and invisible HTML elements.
- application/json: Hide data within random keys or base64-encoded strings.
- image/jpeg: LSB (Least Significant Bit) steganography.

### Image Steganography
When using image steganography, the image should be a real jpeg image, and the steganography should be invisible to the human eye.
The final code will read an directory of jpeg images, and encode the hidden data into the images.

## **Invent a new steganographic technique**
Don't just only use these examples, because using techs which is not mentioned or invented can probide extra security.
Inventing a new steganographic technique is highly appreciated, and that will be a tremendous contribution to the field of network steganography.


## **Traffic Feature Control Strategies**
- Packet size: Use randomization techniques to avoid fixed-size patterns.For example, randomize content within legitimate responses (e.g., inserting random comments in HTML).
- Timing patterns: Introduce random delays to mimic real-world browsing behavior.
- Focus on balancing high stealthiness and performance efficiency.

## **Security Measures Against Detection**
1. **Randomized traffic patterns**: Randomize packet sizes, timings, and sequences to avoid traffic fingerprinting.
2. **Anti-traffic analysis**: Use techniques to evade traffic fingerprinting and flow analysis.

## **HTTP Camouflage Techniques**
Explain how does the steganography protocol disguise packets to look like normal HTTP traffic:
1. **HTTP format conformity**: Ensure all packets conform to standard HTTP request and response formats.
2. **Packet length distribution**: Use Gaussian or other realistic distributions for packet sizes (e.g., 500-1500 bytes).
3. **Timing patterns**: Mimic real HTTP traffic by introducing realistic read-write timing delays.
4. **HTTP header simulation**: Embed typical HTTP headers in payloads to enhance camouflage.
5. **Anti-DPI techniques**: Use padding and encryption to prevent DPI systems from recognizing the hidden protocol.


With the above requirements, please design the steganography protocol with an answer that follows the following structure:

# **Answer**

## **Phase 0: Protocol Name**
Provide the protocol's name, which should be cool, creative and unique. The name shouldn't contain words like "steganography", "stego", "camouflage", etc. Use some friendly and safe name.

## **Phase 1: Protocol Overview**
Describe the protocol’s overall architecture and communication workflow, ensuring that it allows for covert, bidirectional communication within HTTP traffic.

## **Phase 2: Steganography Principle**
Explain the steganography principle, step-by-step, first the handshake phase, then the data transmission phase.

(a) Detail of the packet splitting , encoding and decoding mechanism:
- Describe how each actual data transmission (W) is split into alternating write (w_i) and read (r_i) operations, in which case r_i is the length of the dummy data and w_i is the encoded splitted data.
- Describe how to use the "length of the dummy data" to 1. generate the dummy data with the exact length, 2. check in the other side that if the dummy data really has the exact length.
- Describe how to merge the read-write sequence and decode it into the original data.


(b) Detail of the handshake process:
- **Client Handshake**: How to encode the target address (domain/IP +port, tcp/udp) in the initial HTTP request headers and body. Provides the http request packet sample. Remember to use steganography techniques to hide the target address.
- **Server Handshake**: How the server parses the target address from the handshake packets. Provides the http response packet sample.
- Explain how the data is encoded in html or other mime types with steganography techniques.
- Provide example handshake packets that follow the (w_1, r_1, w_2, r_2) sequence format, in which case r_i is the length of the dummy data and w_i is the encoded splitted data.


(c) Detail of the data transmission phase:
- Explain how w_1 encode the metadata(packets count, dummy data length list) needed to parse the entire sequence.
- Explain how w_1 also transmit the first packet of the data.
- Explain how the data is encoded in html or other mime types with steganography techniques.
- How to dynamically adjust packet size to avoid detection.

## **Phase 3: Traffic Feature Control**
- Explain how the protocol controls traffic features (packet size, timing patterns, HTTP header simulation) to avoid detection by DPI systems.
- Provide practical examples of anti-traffic analysis measures.

## **Phase 4: Invent a new steganographic technique**
- Invent a new steganographic technique, explain how it works. Provide the sample code of the technique.
- Do not repeat the mentioned steganographic techniques, but invent a different one.

## **Phase 5: The final lua code of the protocol**

Code Requirements:

- Follow the above requirements.
- Include encoding and decoding functions for each MIME type.
- Provide a sample client-server communication script.
- **The Newly Invented Steganographic Technique**: Provide a function to encode/decode the hidden data by the newly invented steganographic technique.
- **The Splitting Function**: Functions to split the data into multiple packets.
- **The Randomization Function**: Functions to provide a random index of the mime-types list.
- **MIME Encoding Functions**: Functions to encode hidden data for each MIME type.
- **MIME Decoding Functions**: Functions to extract hidden data from each MIME type.
- **Final Encoding/Decoding Functions**: utilize the Split, Randomize and MIME encode/decode functions to encode/decode the hidden data.
- **Error Handling**: Functions to handle potential errors during encoding, decoding, or transmission.
- Do not provide the `def main()` function, not needed.
- the http request should use "\r\n" to split the lines, not "\n".

### **Important Note**

#### **Final Encoding/Decoding Function Format**
The final encoding/decoding function should be a function that takes a string as input, and returns a string as output.
The pseudo code of these 2 functions:

```lua
function encode(data: string, is_client: boolean) -> (encoded_data_sequence: ([request], [response]))
function decode(encoded_data_sequence: [data], is_client: boolean) -> (decoded_data: string)
```

Note that the passed data is a string, but the returned data is a tuple of 2 lists. One of the element of the tuple will be 
the data part of the encoded_data_sequence, and the other element will be the length of the dummy data.
Which one will be decided by the is_client parameter.


And also provide two extra function for handshake phase, the pseudo code of these 2 functions:

```lua
function encode_handshake_request(target_address: string, request: string) -> (encoded_data:  ([request], [response_length]))
function decode_handshake_response(response: [data]) -> (target_address: string, decoded_data: string)
```


#### **Provide the whole code**
Provide the whole code, do not leave the code unfinished, omitted or require further extensions. Because this generated
code will be used directly in the working project.


#### **Do not contain network communication code**
The code shoult not contain any network communication code, just the steganography protocol code. Because the network communication code is already implemented in an existing project.

#### **How the protocol will be used in an existing project**
The existing project is a rust project, reading and writing the tcp data. When the rust code got an tcp data, it will call the steganography protocol code to encode/decode the data.

