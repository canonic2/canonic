//! Retained Chrome actor, responsive control lanes and owner-driven teardown.
use super::*;
pub(super) async fn run(
    mut worker: Worker,
    bundle: BackendStart,
    options: LaunchOptions,
    startup: Uuid,
    mut ordinary: mpsc::Receiver<Command>,
    mut controls: mpsc::Receiver<Command>,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    let result = match bundle {
        BackendStart::Launch {
            bundle,
            disposable_profile,
        } => {
            worker
                .start(bundle, options, startup, deadline, disposable_profile)
                .await
        }
        BackendStart::Attach(attachment) => {
            worker.attach(attachment, options, startup, deadline).await
        }
    };
    if result.is_err() {
        worker.lose("BROWSER_START_FAILED");
    }
    worker.finish(startup, result).await;
    loop {
        worker.http_poll().await;
        worker.poll_trace().await;
        if let Some(queue) = worker.control_commands.as_mut()
            && let Ok(command) = queue.try_recv()
        {
            worker.apply_native_control(command).await;
            continue;
        }
        if let Ok(command) = controls.try_recv() {
            worker.perform(command).await;
            continue;
        }
        tokio::select! {
            command=control::next(&mut worker.control_commands)=>if let Some(command)=command{worker.apply_native_control(command).await;},
            response=next_response(&mut worker.responses)=>if let Some(response)=response {worker.respond(response).await;},
            execution=next_execution(&mut worker.deferred_executions, &mut worker.executions)=>if let Some(execution)=execution {worker.perform_independent(execution).await;},
            ()=tokio::time::sleep(Duration::from_millis(25)), if !worker.http_requests.is_empty() || worker.trace_timer_pending() || worker.startup_pending() || worker.shared.control.needs_poll() || worker.media_active() || !worker.waits.is_empty() || !worker.interactions.dialogs.is_empty() || !worker.interactions.routes.is_empty() || worker.binding_count()>0 || !worker.transfers.choosers.is_empty() || !worker.inputs.held.is_empty() || worker.transfers.downloads.values().any(|download|download.collect||download.cancel||download.summary["state"]=="pending")=>{
                worker.poll_trace().await;
                worker.service_startup(Instant::now()+Duration::from_secs(5)).await;
                worker.poll_waits().await;
                worker.poll_har_routes().await;
                worker.expire_callbacks().await;
                worker.expire_inputs().await;
                if worker.expire_control().await.is_err(){worker.lose("INPUT_CLEANUP_PENDING");}
                worker.poll_transfers().await;
                if worker.poll_media(startup,Instant::now()+Duration::from_secs(5)).await.is_err(){worker.fail_native_media();}
                worker.publish();
            },
            command = controls.recv() => match command {
                Some(command) => worker.perform(command).await,
                None => break,
            },
            event = next_event(&mut worker.events) => {
                match event {
                    Some(item) => {
                        if worker.apply(item).is_err() { worker.lose("BACKEND_PROTOCOL"); }
                        else {
                            worker.poll_har_routes().await;
                            worker.discard_diagnostic_objects().await;
                            worker.release_evaluation().await;
                            worker.release_webmcp_objects().await;
                            worker.poll_transfers().await;
                            if worker.reconcile(startup, Instant::now() + Duration::from_secs(5)).await.is_err() { worker.lose("TARGET_SETUP_FAILED"); }
                        }
                    },
                    None => { worker.events = None; worker.lose("BACKEND_LOST"); }
                }
            },
            command = ordinary.recv() => match command {
                Some(command) => worker.perform(command).await,
                None => break,
            },
        }
    }
    // Composition keeps a Session until verified disconnect. If its
    // channels disappear unexpectedly, retain a bounded cleanup attempt.
    while worker
        .close(Instant::now() + Duration::from_secs(30))
        .await
        .is_err()
    {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
