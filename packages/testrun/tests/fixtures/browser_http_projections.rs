//! Independent raw HTTP responses for header objects and ECMAScript JSON values.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub fn cases() -> Vec<(&'static str, Vec<u8>)> {
    let escape = |unit: u16| format!("{}u{unit:04x}", char::from(92));
    vec![
        ("primitives",br#"{"Acme":[null,true,false,1,1.5,"text"]}"#.to_vec()),
        ("integer-rounding",b"[9007199254740993,9007199254740995,18446744073709551615,-9007199254740993]".to_vec()),
        ("overflow-underflow",b"[1e400,-1e400,1e-400,-1e-400,-0,0.0]".to_vec()),
        ("surrogates",format!("[\"{}\",\"{}\",\"{}{}\",\"💩\"]",escape(0xd800),escape(0xdc00),escape(0xd83d),escape(0xdca9)).into_bytes()),
        ("surrogate-keys",format!("{{\"{}\":1,\"{}\":2,\"{}{}\":3,\"💩\":4}}",escape(0xd800),escape(0xdc00),escape(0xd83d),escape(0xdca9)).into_bytes()),
        ("key-order",br#"{"b":1,"2":2,"1":1,"00":0,"4294967294":4,"4294967295":5,"a":2,"b":3,"__proto__":{"Acme":true}}"#.to_vec()),
        ("utf8-invalid-string",vec![34,0xe0,0x80,0x80,0xed,0xa0,0x80,0xf0,0x9f,34]),
        ("utf8-invalid-key",vec![123,34,0xff,34,58,49,125]),
        ("whitespace",b" \r\n\t [1] \n".to_vec()),
        ("bom",b"\xef\xbb\xbf{}".to_vec()),
        ("literal-infinity",b"Infinity".to_vec()),
        ("literal-nan",b"NaN".to_vec()),
        ("leading-zero",b"01".to_vec()),
        ("missing-fraction",b"1.".to_vec()),
        ("missing-exponent",b"1e+".to_vec()),
        ("trailing-comma",b"[1,]".to_vec()),
        ("raw-control",vec![34,10,34]),
        ("trailing-data",b"{} true".to_vec()),
    ]
}
pub struct Site {
    pub url: String,
    pub count: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Site {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        let task = tokio::spawn(async move {
            let mut children = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept(),if children.len()<64=> {
                        let (mut stream,_)=accepted.unwrap();let count=observed.clone();
                        children.spawn(async move {
                            let mut bytes=vec![];let mut buffer=[0;1024];
                            while !bytes.windows(4).any(|part|part==b"\r\n\r\n") {
                                assert!(bytes.len()<16384);
                                let read=stream.read(&mut buffer).await.unwrap();
                                if read==0 {return;}
                                bytes.extend_from_slice(&buffer[..read]);
                            }
                            let request=std::str::from_utf8(&bytes).unwrap();
                            let path=request.split_whitespace().nth(1).unwrap();
                            let index=path.strip_prefix('/').unwrap().parse::<usize>().unwrap();
                            let cases=cases();let (_,body)=&cases[index];
                            count.fetch_add(1,Ordering::SeqCst);
                            let mut response=b"HTTP/1.1 200 Acme\r\nX-Acme: one\r\nx-acme: two\r\nSet-Cookie: AcmeA=one; Path=/\r\nset-cookie: AcmeB=two; Path=/\r\n__proto__: Acme-proto\r\nConstructor: Acme-ctor\r\n2: two\r\n1: one\r\nX-Latin: ".to_vec();
                            response.push(255);
                            response.extend_from_slice(format!("\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",body.len()).as_bytes());
                            response.extend_from_slice(body);
                            stream.write_all(&response).await.unwrap();
                            stream.shutdown().await.unwrap();
                        });
                    },
                    Some(result)=children.join_next(),if !children.is_empty()=>{result.unwrap();},
                }
            }
        });
        Self { url, count, task }
    }
}
impl Drop for Site {
    fn drop(&mut self) {
        self.task.abort();
    }
}
