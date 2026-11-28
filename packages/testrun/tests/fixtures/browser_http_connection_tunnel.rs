//! An owned CONNECT relay reaches only the explicitly selected loopback ports.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Notify,
};

pub struct Tunnel {
    pub url: String,
    task: tokio::task::JoinHandle<()>,
    count: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    changed: Arc<Notify>,
}
impl Drop for Tunnel {
    fn drop(&mut self) {
        self.task.abort();
    }
}
struct Active(Arc<AtomicUsize>, Arc<Notify>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
        self.1.notify_waiters();
    }
}
impl Tunnel {
    pub async fn start(ports: [u16; 2]) -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let changed = Arc::new(Notify::new());
        let (observed, live, notify) = (count.clone(), active.clone(), changed.clone());
        let task = tokio::spawn(async move {
            let mut calls = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept(),if calls.len()<64=>{
                        let (mut socket,_)=accepted.unwrap();
                        observed.fetch_add(1,Ordering::SeqCst);live.fetch_add(1,Ordering::SeqCst);
                        let guard=Active(live.clone(),notify.clone());
                        calls.spawn(async move {
                            let _guard=guard;let mut bytes=Vec::new();
                            while !bytes.ends_with(b"\r\n\r\n") {
                                assert!(bytes.len()<16384,"Bounded owned CONNECT head");
                                let Ok(byte)=socket.read_u8().await else{return;};bytes.push(byte);
                            }
                            let header=String::from_utf8(bytes).unwrap();assert!(header.starts_with("CONNECT "));
                            let authority=header.split_whitespace().nth(1).unwrap();
                            let (host,port)=authority.rsplit_once(':').unwrap();
                            assert!(matches!(host,"127.0.0.1"|"localhost"));
                            let port:u16=port.parse().unwrap();assert!(ports.contains(&port));
                            let mut origin=tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST,port)).await.unwrap();
                            socket.write_all(b"HTTP/1.1 200 Acme tunnel\r\n\r\n").await.unwrap();
                            let _=tokio::io::copy_bidirectional(&mut socket,&mut origin).await;
                        });
                    },
                    Some(done)=calls.join_next(),if !calls.is_empty()=>{done.unwrap();}
                }
            }
        });
        Self {
            url,
            task,
            count,
            active,
            changed,
        }
    }
    pub async fn settled(&self, expected: usize) {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let next = self.changed.notified();
            let count = self.count.load(Ordering::SeqCst);
            assert!(
                count <= expected,
                "No unexpected proxy connection or replay"
            );
            if count == expected && self.active.load(Ordering::SeqCst) == 0 {
                return;
            }
            tokio::time::timeout_at(deadline, next)
                .await
                .expect("Owned tunnels must settle");
        }
    }
}
