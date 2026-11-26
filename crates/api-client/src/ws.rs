//! Conexão WebSocket: no navegador sobre o `gloo-net`, no app desktop sobre o
//! `tokio-tungstenite`.
//!
//! [`connect`] abre o socket e devolve duas metades tipadas: um `Sink` de [`ClientEvent`] e um
//! `Stream` de [`ServerEvent`]. O stream termina quando a conexão cai; reconectar é com quem
//! chama.

use futures::{Sink, SinkExt, StreamExt};
#[cfg(target_arch = "wasm32")]
use gloo_net::websocket::{Message, WebSocketError as SocketError, futures::WebSocket};
use proto::{ClientEvent, ServerEvent};
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::tungstenite::{Error as SocketError, Message};

use crate::{ApiClient, ApiError};

/// Os eventos do servidor. No desktop, o stream é `Send`, para rodar no runtime do tokio.
#[cfg(target_arch = "wasm32")]
pub type Events = futures::stream::LocalBoxStream<'static, ServerEvent>;
#[cfg(not(target_arch = "wasm32"))]
pub type Events = futures::stream::BoxStream<'static, ServerEvent>;

#[derive(Debug, thiserror::Error)]
pub enum WsError {
    /// O ticket não saiu (ex.: sessão expirada).
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("não abriu o WebSocket: {0}")]
    Open(String),
    #[error("{0}")]
    Socket(#[from] SocketError),
}

/// Pede um ticket e abre o WebSocket. As metades não emprestam o `api` (`use<>`), então podem ir
/// para tarefas separadas.
pub async fn connect(
    api: &ApiClient,
) -> Result<(impl Sink<ClientEvent, Error = WsError> + use<>, Events), WsError> {
    let ticket = api.ws_ticket().await?;
    let url = api.ws_url(&ticket.ticket);
    #[cfg(target_arch = "wasm32")]
    let socket = WebSocket::open(&url).map_err(|err| WsError::Open(err.to_string()))?;
    #[cfg(not(target_arch = "wasm32"))]
    let (socket, _) = tokio_tungstenite::connect_async(url.as_str()).await?;
    let (sink, stream) = socket.split();

    let sink = sink.sink_map_err(WsError::from).with(|event: ClientEvent| async move {
        let json = serde_json::to_string(&event).expect("ClientEvent sempre serializa");
        // No navegador o texto já é `String`; no tungstenite, `Utf8Bytes`.
        #[allow(clippy::useless_conversion)]
        Ok::<_, WsError>(Message::Text(json.into()))
    });

    // Para no primeiro erro (conexão caiu) e ignora o que não for um evento conhecido.
    let stream =
        stream.take_while(|msg| futures::future::ready(msg.is_ok())).filter_map(|msg| async move {
            match msg {
                Ok(Message::Text(text)) => serde_json::from_str::<ServerEvent>(&text).ok(),
                _ => None,
            }
        });
    #[cfg(target_arch = "wasm32")]
    let stream = stream.boxed_local();
    #[cfg(not(target_arch = "wasm32"))]
    let stream = stream.boxed();
    Ok((sink, stream))
}
