//! Minimal text WebSocket client for both native and browser builds.

use futures::{Sink, SinkExt, Stream, StreamExt};
use std::pin::Pin;

// `Send` on both targets: matchbox requires signallers to be `Send + Sync`.
pub type Uscita = Pin<Box<dyn Sink<String, Error = String> + Send>>;
pub type Entrata = Pin<Box<dyn Stream<Item = String> + Send>>;

#[cfg(not(target_arch = "wasm32"))]
pub async fn connetti(url: &str) -> Result<(Uscita, Entrata), String> {
    use async_tungstenite::tungstenite::{Error, Message};
    let (ws, _) = async_tungstenite::smol::connect_async(url)
        .await
        .map_err(|e| e.to_string())?;
    let (tx, rx) = StreamExt::split(ws);
    let tx = tx
        .with(|s: String| async move { Ok::<_, Error>(Message::text(s)) })
        .sink_map_err(|e| e.to_string());
    let rx = rx
        .take_while(|m| futures::future::ready(m.is_ok()))
        .filter_map(|m| async move {
            match m {
                Ok(Message::Text(t)) => Some(t.to_string()),
                _ => None,
            }
        });
    Ok((Box::pin(tx), Box::pin(rx)))
}

#[cfg(target_arch = "wasm32")]
pub async fn connetti(url: &str) -> Result<(Uscita, Entrata), String> {
    use ws_stream_wasm::{WsErr, WsMessage, WsMeta};
    let (_meta, ws) = WsMeta::connect(url, None)
        .await
        .map_err(|e| e.to_string())?;
    let (tx, rx) = ws.split();
    let tx = tx
        .with(|s: String| async move { Ok::<_, WsErr>(WsMessage::Text(s)) })
        .sink_map_err(|e| e.to_string());
    let rx = rx.filter_map(|m| async move {
        match m {
            WsMessage::Text(t) => Some(t),
            WsMessage::Binary(_) => None,
        }
    });
    Ok((Box::pin(tx), Box::pin(rx)))
}
