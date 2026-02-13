// HTTP/3 connection handler — QUIC transport via quinn, HTTP/3 framing via h3
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use crate::modules::Pipeline;
use bytes::{Buf, Bytes};
use std::sync::Arc;

/// Run the HTTP/3 QUIC endpoint accept loop.
pub async fn run_h3_server(endpoint: quinn::Endpoint, pipeline: Arc<Pipeline>) {
    let local = endpoint.local_addr().map(|a| a.to_string()).unwrap_or_default();
    crate::log::info(&format!("HTTP/3 (QUIC) listening on {local}"));

    loop {
        if crate::server::SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        let incoming = match endpoint.accept().await {
            Some(conn) => conn,
            None => break,
        };
        let pipe = Arc::clone(&pipeline);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(incoming, pipe).await {
                crate::log::debug(&format!("h3: connection error: {e}"));
            }
        });
    }
    endpoint.close(0u32.into(), b"shutdown");
    crate::log::info("HTTP/3 endpoint stopped");
}

async fn handle_connection(
    incoming: quinn::Incoming,
    pipeline: Arc<Pipeline>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let conn = incoming.await?;
    let peer_ip = conn.remote_address().ip().to_string();

    let quinn_conn = h3_quinn::Connection::new(conn);
    let mut h3_conn = h3::server::Connection::new(quinn_conn).await?;

    loop {
        match h3_conn.accept().await {
            Ok(Some(resolver)) => {
                let pipe = Arc::clone(&pipeline);
                let ip = peer_ip.clone();
                tokio::spawn(async move {
                    match resolver.resolve_request().await {
                        Ok((req, stream)) => {
                            if let Err(e) = handle_request(req, stream, pipe, ip).await {
                                crate::log::debug(&format!("h3: request error: {e}"));
                            }
                        }
                        Err(e) => {
                            crate::log::debug(&format!("h3: resolve error: {e}"));
                        }
                    }
                });
            }
            Ok(None) => break,
            Err(e) => {
                crate::log::debug(&format!("h3: accept error: {e}"));
                break;
            }
        }
    }
    Ok(())
}

async fn handle_request(
    request: http::Request<()>,
    mut stream: h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
    pipeline: Arc<Pipeline>,
    peer_ip: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut body = Vec::new();
    while let Some(data) = stream.recv_data().await? {
        body.extend_from_slice(data.chunk());
        if body.len() > crate::http::MAX_BODY_SIZE {
            let resp = http::Response::builder().status(413).body(()).unwrap();
            stream.send_response(resp).await?;
            stream.send_data(Bytes::from_static(b"Payload Too Large")).await?;
            stream.finish().await?;
            return Ok(());
        }
    }

    let parts = request.into_parts().0;
    let mut headers = Vec::new();
    for (name, value) in parts.headers.iter() {
        if let Ok(v) = value.to_str() {
            headers.push((name.to_string(), v.to_string()));
        }
    }
    let path = parts
        .uri
        .path_and_query()
        .map(|pq| pq.to_string())
        .unwrap_or_else(|| "/".to_string());
    let method = parts.method.to_string();

    crate::metrics::inc_requests();
    crate::metrics::add_bytes_in(body.len() as u64);
    crate::log::request(&method, &path, &peer_ip);

    let req = HttpRequest {
        method,
        path,
        version: "HTTP/3".to_string(),
        headers,
        body,
    };

    let resp = tokio::task::spawn_blocking(move || {
        let mut r = req;
        let mut ctx = Context::new();
        ctx.set("_client_ip", peer_ip);
        ctx.set("_protocol", "h3".to_string());
        pipeline.handle(&mut r, &mut ctx)
    })
    .await
    .unwrap_or_else(|_| HttpResponse::error(500, "Internal error"));

    crate::log::response(resp.status_code, 0, false);
    if resp.status_code < 400 {
        crate::metrics::inc_requests_ok();
    } else {
        crate::metrics::inc_requests_err();
    }

    let mut builder = http::Response::builder().status(resp.status_code);
    for (name, value) in &resp.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let h3_resp = builder.body(()).unwrap();

    stream.send_response(h3_resp).await?;
    if !resp.body.is_empty() {
        crate::metrics::add_bytes_out(resp.body.len() as u64);
        stream.send_data(Bytes::from(resp.body)).await?;
    }
    stream.finish().await?;
    Ok(())
}
