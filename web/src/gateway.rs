//! Loop do WebSocket: conecta, mantém o heartbeat e reconecta com backoff exponencial.
//!
//! Ao (re)conectar, o [`Store::on_connected`] reinscreve em todos os servidores e busca com
//! `?after=` o que chegou enquanto a conexão estava caída.

use call_core::{Backoff, ConnState};
use futures::{StreamExt, channel::mpsc};
use gloo_timers::future::TimeoutFuture;
use leptos::{prelude::*, task::spawn_local};
use proto::{ClientEvent, ServerEvent};

use crate::store::Store;

/// Roda enquanto a sessão `epoch` for a atual.
pub fn spawn(store: Store, epoch: u64) {
    spawn_local(async move {
        let mut backoff = Backoff::new();
        while store.is_current(epoch) {
            store.conn.set(ConnState::Connecting);
            match api_client::ws::connect(&store.api()).await {
                Ok((sink, mut events)) => {
                    // Fila de saída: quem quiser mandar um evento usa `store.send_ws`.
                    // Fechar a fila (logout ou queda) faz o `forward` fechar o socket.
                    let (tx, rx) = mpsc::unbounded::<ClientEvent>();
                    spawn_local(async move {
                        let _ = rx.map(Ok).forward(sink).await;
                    });

                    while let Some(event) = events.next().await {
                        if !store.is_current(epoch) {
                            break;
                        }
                        if let ServerEvent::Hello { heartbeat_interval_ms, .. } = &event {
                            backoff.reset();
                            store.ws_tx.set_value(Some(tx.clone()));
                            store.conn.set(ConnState::Connected);
                            heartbeat(
                                tx.clone(),
                                u32::try_from(*heartbeat_interval_ms).unwrap_or(u32::MAX),
                            );
                            store.on_connected();
                        }
                        store.apply(event);
                    }
                    tx.close_channel();
                    if store.is_current(epoch) {
                        store.ws_tx.set_value(None);
                        store.on_disconnected();
                    }
                }
                Err(api_client::ws::WsError::Api(err)) if err.is_unauthorized() => {
                    store.report(&err);
                    return;
                }
                Err(err) => leptos::logging::warn!("WebSocket: {err}"),
            }
            if !store.is_current(epoch) {
                return;
            }

            // Espera com contagem regressiva na tela.
            let delay = backoff.next_delay(js_sys::Math::random());
            let mut remaining_ms = u32::try_from(delay.as_millis()).unwrap_or(u32::MAX);
            while remaining_ms > 0 && store.is_current(epoch) {
                store.conn.set(ConnState::Reconnecting { in_secs: remaining_ms.div_ceil(1000) });
                let step = remaining_ms.min(1000);
                TimeoutFuture::new(step).await;
                remaining_ms -= step;
            }
        }
    });
}

/// Manda `HEARTBEAT` no intervalo pedido até a fila fechar.
fn heartbeat(tx: mpsc::UnboundedSender<ClientEvent>, interval_ms: u32) {
    spawn_local(async move {
        loop {
            TimeoutFuture::new(interval_ms).await;
            if tx.unbounded_send(ClientEvent::Heartbeat).is_err() {
                return;
            }
        }
    });
}
