//! Owned raw HTTP bytes for request URL/header normalization and credential precedence.
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub struct Site {
    pub url: String,
    pub records: Arc<Mutex<Vec<Value>>>,
    held: Arc<Mutex<Option<String>>>,
    release: Arc<tokio::sync::Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl Site {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let records = Arc::new(Mutex::new(vec![]));
        let observed = records.clone();
        let held = Arc::new(Mutex::new(None::<String>));
        let holding = held.clone();
        let release = Arc::new(tokio::sync::Notify::new());
        let releasing = release.clone();
        let task = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept(),if requests.len()<64=> {
                        let (mut stream,_)=accepted.unwrap();let records=observed.clone();let held=holding.clone();let release=releasing.clone();
                        requests.spawn(async move {
                            let mut data=vec![];let mut buffer=[0;1024];
                            while !data.windows(4).any(|part|part==b"\r\n\r\n") {
                                assert!(data.len()<16384);let read=stream.read(&mut buffer).await.unwrap();
                                if read==0{return;}data.extend_from_slice(&buffer[..read]);
                            }
                            let end=data.windows(4).position(|part|part==b"\r\n\r\n").unwrap()+4;
                            let head=data[..end].iter().copied().map(char::from).collect::<String>();
                            let mut headers=serde_json::Map::new();
                            for line in head.lines().skip(1) {
                                if let Some((name,value))=line.split_once(':') {headers.insert(name.to_ascii_lowercase(),json!(value.trim_matches([' ','\t'])));}
                            }
                            let length=headers.get("content-length").and_then(Value::as_str).map(|value|value.parse::<usize>().unwrap()).unwrap_or(0);
                            assert!(length<=65536);
                            while data.len()<end+length {let read=stream.read(&mut buffer).await.unwrap();assert!(read>0);data.extend_from_slice(&buffer[..read]);}
                            let path=head.split_whitespace().nth(1).unwrap();
                            let record=json!({"url":path,"method":head.split_whitespace().next().unwrap(),"headers":headers,"bodyBase64":base64::Engine::encode(&base64::engine::general_purpose::STANDARD,&data[end..])});
                            records.lock().unwrap().push(record.clone());
                            let paused=held.lock().unwrap().as_deref()==Some(path);
                            if paused {release.notified().await;}
                            let (status,extra)=if path=="/redirect-credentials" {
                                (302,format!("Location: http://Acme2:changed@{}/after-redirect#Acme\r\n",stream.local_addr().unwrap()))
                            }else if path=="/auth-fallback"&&record["headers"]["authorization"]!="Basic QWNtZUNvbmZpZ3VyZWQ6Y29ycmVjdA==" {
                                (401,"WWW-Authenticate: Basic realm=Acme\r\n".into())
                            }else{(200,String::new())};
                            let fixture=path=="/fixture";
                            let body=if fixture {"<!doctype html><title>Acme Request source</title>Acme".into()}else{record.to_string()};
                            let content_type=if fixture {"text/html"}else{"application/json"};
                            let response=format!("HTTP/1.1 {status} Acme\r\n{extra}Content-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                            stream.write_all(response.as_bytes()).await.unwrap();
                        });
                    },
                    Some(result)=requests.join_next(),if !requests.is_empty()=>{result.unwrap();},
                }
            }
        });
        Self {
            url,
            records,
            held,
            release,
            task,
        }
    }
    pub fn hold(&self, path: &str) {
        *self.held.lock().unwrap() = Some(path.into());
    }
    pub fn release(&self) {
        self.release.notify_one();
    }
}
impl Drop for Site {
    fn drop(&mut self) {
        self.task.abort();
    }
}
