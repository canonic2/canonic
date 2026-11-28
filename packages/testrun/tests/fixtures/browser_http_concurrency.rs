//! Owned loopback requests remain held until the test grants each exact path.
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
    pub count: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}
impl Site {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (sender, arrivals) = mpsc::channel(32);
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        let task = tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept()=>{
                        let (mut stream,_)=accepted.unwrap();
                        let sender=sender.clone();let observed=observed.clone();
                        tasks.spawn(async move {
                            let mut header=Vec::new();
                            while !header.ends_with(b"\r\n\r\n") {
                                assert!(header.len()<16384);
                                let Ok(byte)=stream.read_u8().await else {return};
                                header.push(byte);
                            }
                            let path=String::from_utf8(header).unwrap().split_whitespace().nth(1).unwrap().to_owned();
                            observed.fetch_add(1,Ordering::SeqCst);
                            let (grant,wait)=oneshot::channel();
                            if sender.send((path.clone(),grant)).await.is_err() {return;}
                            if wait.await.is_err() {return;}
                            let status=match path.as_str(){"/A"=>201,"/B"=>202,_=>203};
                            let response=format!("HTTP/1.1 {status} Acme\r\nX-Acme: {path}\r\nSet-Cookie: Acme{}=one; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{path}",&path[1..],path.len());
                            let _=stream.write_all(response.as_bytes()).await;
                        });
                    }
                    result=tasks.join_next(),if !tasks.is_empty()=>{result.unwrap().unwrap();}
                }
            }
        });
        Self {
            url,
            arrivals,
            count,
            task,
        }
    }
    pub async fn held(&mut self, count: usize) -> BTreeMap<String, oneshot::Sender<()>> {
        let mut held = BTreeMap::new();
        for _ in 0..count {
            let (path, grant) = tokio::time::timeout(Duration::from_secs(2), self.arrivals.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(held.insert(path, grant).is_none());
        }
        held
    }
}
impl Drop for Site {
    fn drop(&mut self) {
        self.task.abort();
    }
}
