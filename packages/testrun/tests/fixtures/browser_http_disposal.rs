//! Owned loopback disposal fixture records exact arrivals and socket EOF.
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, oneshot},
    task::{JoinHandle, JoinSet},
};
pub struct Site {
    pub url: String,
    arrivals: mpsc::Receiver<(String, oneshot::Sender<()>)>,
    closed: mpsc::Receiver<String>,
    pub count: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}
impl Site {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (arrived, arrivals) = mpsc::channel(16);
        let (eof, closed) = mpsc::channel(16);
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        let task = tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept()=>{
                        let (mut stream,_)=accepted.unwrap();
                        let arrived=arrived.clone();let eof=eof.clone();let observed=observed.clone();
                        tasks.spawn(async move {
                            let mut header=Vec::new();
                            while !header.ends_with(b"\r\n\r\n") {
                                assert!(header.len()<16384);
                                let Ok(byte)=stream.read_u8().await else {return};
                                header.push(byte);
                            }
                            let path=String::from_utf8(header).unwrap().split_whitespace().nth(1).unwrap().to_owned();
                            if path!="/favicon.ico" {observed.fetch_add(1,Ordering::SeqCst);}
                            if path.starts_with("/hold") {
                                let (grant,wait)=oneshot::channel();
                                arrived.send((path.clone(),grant)).await.unwrap();
                                let mut byte=[0];
                                tokio::select! {
                                    granted=wait=>if granted.is_err() {return;},
                                    result=stream.read(&mut byte)=>{
                                        assert!(matches!(result,Ok(0))||result.is_err());
                                        let _=eof.send(path).await;return;
                                    }
                                }
                            }
                            let status=if matches!(path.as_str(),"/complete"|"/response-dispose") {201} else if path.starts_with("/hold") {202} else {200};
                            let response=format!("HTTP/1.1 {status} Acme\r\nX-Acme: one\r\nContent-Length: 4\r\nConnection: close\r\n\r\nAcme");
                            let _=stream.write_all(response.as_bytes()).await;
                        });
                    },
                    result=tasks.join_next(),if !tasks.is_empty()=>{result.unwrap().unwrap();},
                }
            }
        });
        Self {
            url,
            arrivals,
            closed,
            count,
            task,
        }
    }
    pub async fn held(&mut self, count: usize) -> BTreeMap<String, oneshot::Sender<()>> {
        let mut result = BTreeMap::new();
        for _ in 0..count {
            let (path, grant) = tokio::time::timeout(Duration::from_secs(2), self.arrivals.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(result.insert(path, grant).is_none());
        }
        result
    }
    pub async fn closed(&mut self, count: usize) -> Vec<String> {
        let mut result = Vec::new();
        for _ in 0..count {
            result.push(
                tokio::time::timeout(Duration::from_secs(2), self.closed.recv())
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        result.sort();
        result
    }
}
impl Drop for Site {
    fn drop(&mut self) {
        self.task.abort();
    }
}
