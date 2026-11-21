//! Conexão WebSocket no navegador (só wasm32), sobre o `gloo-net`.
//!
//! [`connect`] abre o socket e devolve duas metades tipadas: um `Sink` de [`ClientEvent`] e um
//! `Stream` de [`ServerEvent`]. O stream termina quando a conexão cai; reconectar é com quem
//! chama.

use futures::{Sink, SinkExt, StreamExt, stream::LocalBoxStream};
use gloo_net::websocket::{Message, WebSocketError, futures::WebSocket};
use proto::{ClientEvent, ServerEvent};

use crate::{ApiClient, ApiError};

#[derive(Debug, thiserror::Error)]
pub enum WsError {
    /// O ticket não saiu (ex.: sessão expirada).
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("não abriu o WebSocket: {0}")]
    Open(String),
    #[error("{0}")]
    Socket(#[from] WebSocketError),
}

/// Pede um ticket e abre o WebSocket. As metades não emprestam o `api` (`use<>`), então podem ir
/// para tarefas separadas.
#[allow(clippy::type_complexity)]
pub async fn connect(
    api: &ApiClient,
) -> Result<
    (impl Sink<ClientEvent, Error = WsError> + use<>, LocalBoxStream<'static, ServerEvent>),
    WsError,
> {
    let ticket = api.ws_ticket().await?;
    open(api.ws_url(&ticket.ticket))
}

fn open(
    url: String,
) -> Result<(impl Sink<ClientEvent, Error = WsError>, LocalBoxStream<'static, ServerEvent>), WsError>
{
    let socket = WebSocket::open(&url).map_err(|err| WsError::Open(err.to_string()))?;
    let (sink, stream) = socket.split();

    let sink = sink.sink_map_err(WsError::from).with(|event: ClientEvent| async move {
        let json = serde_json::to_string(&event).expect("ClientEvent sempre serializa");
        Ok::<_, WsError>(Message::Text(json))
    });

    // Para no primeiro erro (conexão caiu) e ignora o que não for um evento conhecido.
    let stream = stream
        .take_while(|msg| futures::future::ready(msg.is_ok()))
        .filter_map(|msg| async move {
            match msg {
                Ok(Message::Text(text)) => serde_json::from_str::<ServerEvent>(&text).ok(),
                _ => None,
            }
        })
        .boxed_local();
    Ok((sink, stream))
}
