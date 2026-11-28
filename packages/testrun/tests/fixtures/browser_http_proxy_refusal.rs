//! Scripted refusing proxy with an independently counted destination sentinel.
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct Refusal {
    pub proxy: String,
    pub port: u16,
    origin_connections: Arc<AtomicUsize>,
    records: Arc<Mutex<Vec<Value>>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
impl Drop for Refusal {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
impl Refusal {
    pub async fn start(responses: Vec<Vec<u8>>) -> Self {
        let origin = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = origin.local_addr().unwrap().port();
        let origin_connections = Arc::new(AtomicUsize::new(0));
        let observed = origin_connections.clone();
        let origin_task = tokio::spawn(async move {
            loop {
                let (stream, _) = origin.accept().await.unwrap();
                observed.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let proxy = format!("http://{}", listener.local_addr().unwrap());
        let records: Arc<Mutex<Vec<Value>>> = Arc::default();
        let captured = records.clone();
        let mut responses = std::collections::VecDeque::from(responses);
        let proxy_task = tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept(),if tasks.len()<64=>{
                        let (mut stream,_)=accepted.unwrap();
                        let response=responses.pop_front().expect("No unexpected CONNECT replay");
                        let captured=captured.clone();
                        tasks.spawn(async move {
                            let mut bytes=Vec::new();let mut buffer=[0;1024];
                            while bytes.len()<=32768 && !bytes.windows(4).any(|part|part==b"\r\n\r\n") {
                                let count=stream.read(&mut buffer).await.unwrap();assert!(count>0);
                                bytes.extend_from_slice(&buffer[..count]);
                            }
                            let end=bytes.windows(4).position(|part|part==b"\r\n\r\n").unwrap()+4;
                            assert!(end<=32768);
                            let head=String::from_utf8(bytes[..end].to_vec()).unwrap();
                            assert!(head.starts_with(&format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\n")));
                            let headers=head.lines().skip(1).filter_map(|line|line.split_once(':')).map(|(key,value)|{
                                let value=if key.eq_ignore_ascii_case("host") {value.trim().replace(&port.to_string(),"PORT")} else {value.trim().to_owned()};
                                (key.to_ascii_lowercase(),json!(value))
                            }).collect::<serde_json::Map<_,_>>();
                            let index={let mut rows=captured.lock().unwrap();let index=rows.len();rows.push(json!({"headers":headers,"trailingBytes":bytes.len()-end,"settled":false}));index};
                            stream.write_all(&response).await.unwrap();
                            stream.shutdown().await.unwrap();
                            loop {
                                match stream.read(&mut buffer).await {
                                    Ok(0)|Err(_)=>break,
                                    Ok(count)=>{
                                        let mut rows=captured.lock().unwrap();
                                        let old=rows[index]["trailingBytes"].as_u64().unwrap();
                                        rows[index]["trailingBytes"]=json!(old+count as u64);
                                    }
                                }
                            }
                            captured.lock().unwrap()[index]["settled"]=json!(true);
                        });
                    },
                    Some(result)=tasks.join_next(),if !tasks.is_empty()=>{result.unwrap();},
                }
            }
        });
        Self {
            proxy,
            port,
            origin_connections,
            records,
            tasks: vec![origin_task, proxy_task],
        }
    }
    pub fn url(&self, scheme: &str, path: &str) -> String {
        format!("{scheme}://127.0.0.1:{}{path}", self.port)
    }
    pub async fn settled(&self, count: usize) -> Vec<Value> {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let rows = self.records.lock().unwrap().clone();
                if rows.len() == count && rows.iter().all(|row| row["settled"] == true) {
                    assert_eq!(self.origin_connections.load(Ordering::SeqCst), 0);
                    return rows
                        .into_iter()
                        .map(|mut row| {
                            row.as_object_mut().unwrap().remove("settled");
                            row
                        })
                        .collect();
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("Original proxy connections must settle without origin traffic")
    }
}
pub fn response(status: u16, headers: &str, body: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status} Acme refusal\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).into_bytes()
}
