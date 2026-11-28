//! Owned loopback pages; shutdown joins the server after every browser is released.
pub(super) struct Site {
    port: u16,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Site {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let (shutdown, stop) = tokio::sync::oneshot::channel();
        let html = include_str!("../../../tests/fixtures/browser/agent-journey.html").replace(
            "<iframe id=cross>",
            &format!("<iframe id=cross src='http://localhost:{port}/frame'>"),
        );
        let app=axum::Router::new().route("/",axum::routing::get(move||{let html=html.clone();async move{axum::response::Html(html)}}))
            .route("/redirect",axum::routing::get(||async{axum::response::Redirect::temporary("/")}))
            .route("/frame",axum::routing::get(||async{axum::response::Html("<button onclick=\"this.textContent=event.isTrusted?'Pressed':'Untrusted'\">Frame button</button>")}))
            .route("/popup",axum::routing::get(||async{axum::response::Html("<h1>Acme popup</h1>")}))
            .route("/article",axum::routing::get(||async{axum::response::Html("<title>Acme article</title><main>Acme article</main><script>document.body.textContent='Scripts must not run';</script>")}));
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stop.await;
                })
                .await
                .unwrap();
        });
        Self {
            port,
            shutdown: Some(shutdown),
            task: Some(task),
        }
    }
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
    pub async fn stop(mut self) {
        let _ = self.shutdown.take().unwrap().send(());
        self.task.take().unwrap().await.unwrap();
    }
}
impl Drop for Site {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
