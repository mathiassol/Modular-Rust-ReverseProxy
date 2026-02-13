// HTTP/2 connection handler — converts h2 frames to/from pipeline HttpRequest/HttpResponse
use crate::context::Context;
use crate::http::{HttpRequest, HttpResponse};
use crate::modules::Pipeline;
use bytes::Bytes;
use h2::server;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};

/// Handle one HTTP/2 connection (may carry many streams).
pub async fn handle_connection<S>(
    io: S,
    pipeline: Arc<Pipeline>,
    peer_ip: String,
    alt_svc: Option<String>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut conn = match server::handshake(io).await {
        Ok(c) => c,
        Err(e) => {
            crate::log::error(&format!("h2: handshake failed: {e}"));
            return;
        }
    };

    while let Some(result) = conn.accept().await {
        let (request, respond) = match result {
            Ok(pair) => pair,
            Err(e) => {
                if !e.is_go_away() {
                    crate::log::warn(&format!("h2: stream error: {e}"));
                }
                break;
            }
        };

        let pipe = Arc::clone(&pipeline);
        let ip = peer_ip.clone();
        let alt = alt_svc.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_stream(request, respond, pipe, ip, alt).await {
                crate::log::debug(&format!("h2: stream error: {e}"));
            }
        });
    }
}

async fn handle_stream(
    request: http::Request<h2::RecvStream>,
    mut respond: server::SendResponse<Bytes>,
    pipeline: Arc<Pipeline>,
    peer_ip: String,
    alt_svc: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (parts, mut body_stream) = request.into_parts();

    let mut body = Vec::new();
    while let Some(chunk) = body_stream.data().await {
        let data = chunk?;
        let _ = body_stream.flow_control().release_capacity(data.len());
        body.extend_from_slice(&data);
        if body.len() > crate::http::MAX_BODY_SIZE {
            let resp = http::Response::builder().status(413).body(()).unwrap();
            let mut send = respond.send_response(resp, false)?;
            send.send_data(Bytes::from_static(b"Payload Too Large"), true)?;
            return Ok(());
        }
    }

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
        version: "HTTP/2".to_string(),
        headers,
        body,
    };

    let resp = tokio::task::spawn_blocking(move || {
        let mut r = req;
        let mut ctx = Context::new();
        ctx.set("_client_ip", peer_ip);
        ctx.set("_protocol", "h2".to_string());
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
    if let Some(ref alt) = alt_svc {
        builder = builder.header("Alt-Svc", alt.as_str());
    }
    let h2_resp = builder.body(()).unwrap();

    let is_empty = resp.body.is_empty();
    let mut send = respond.send_response(h2_resp, is_empty)?;
    if !is_empty {
        crate::metrics::add_bytes_out(resp.body.len() as u64);
        send.send_data(Bytes::from(resp.body), true)?;
    }
    Ok(())
}
