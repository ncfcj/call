//! Loop do WebSocket como uma `Subscription` do iced: conecta, mantém o heartbeat e reconecta
//! com backoff exponencial, como o `web/src/gateway.rs`.
//!
//! Tudo roda numa tarefa só: quando a subscription sai da tela (logout), o iced a derruba e o
//! socket fecha junto.

use std::{
    hash::{Hash, Hasher},
    time::{Duration, SystemTime},
};

use api_client::{ApiClient, ws::WsError};
use call_core::Backoff;
use iced::{
    Subscription,
    futures::{SinkExt, StreamExt, channel::mpsc},
};
use proto::{ClientEvent, ServerEvent};
use tokio::time::{Interval, interval_at, sleep};

#[derive(Debug, Clone)]
pub enum Event {
    /// Chegou o `HELLO`: dá para mandar eventos por aqui.
    Connected(mpsc::UnboundedSender<ClientEvent>),
    Server(ServerEvent),
    Disconnected,
    Reconnecting {
        in_secs: u32,
    },
    /// O ticket foi recusado: a sessão acabou.
    Unauthorized,
}

/// Uma conexão por sessão: um `epoch` novo (outro login) começa outro loop.
struct Key {
    epoch: u64,
    api: ApiClient,
}

impl Hash for Key {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.epoch.hash(state);
    }
}

pub fn subscription(api: &ApiClient, epoch: u64) -> Subscription<Event> {
    Subscription::run_with(Key { epoch, api: api.clone() }, |key| {
        let api = key.api.clone();
        iced::stream::channel(100, async move |out| run(api, out).await)
    })
}

async fn run(api: ApiClient, mut out: mpsc::Sender<Event>) {
    let mut backoff = Backoff::new();
    loop {
        match api_client::ws::connect(&api).await {
            Ok((sink, mut events)) => {
                let mut sink = Box::pin(sink);
                let (tx, mut rx) = mpsc::unbounded::<ClientEvent>();
                let mut beat: Option<Interval> = None;
                loop {
                    tokio::select! {
                        event = events.next() => {
                            let Some(event) = event else { break };
                            if let ServerEvent::Hello { heartbeat_interval_ms, .. } = &event {
                                backoff.reset();
                                let every = Duration::from_millis(*heartbeat_interval_ms);
                                beat = Some(interval_at(tokio::time::Instant::now() + every, every));
                                let _ = out.send(Event::Connected(tx.clone())).await;
                            }
                            if out.send(Event::Server(event)).await.is_err() {
                                return;
                            }
                        }
                        Some(event) = rx.next() => {
                            if sink.send(event).await.is_err() {
                                break;
                            }
                        }
                        () = tick(&mut beat) => {
                            if sink.send(ClientEvent::Heartbeat).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                let _ = out.send(Event::Disconnected).await;
            }
            Err(WsError::Api(err)) if err.is_unauthorized() => {
                let _ = out.send(Event::Unauthorized).await;
                return;
            }
            Err(_) => {}
        }

        // Espera com contagem regressiva na tela.
        let mut remaining = backoff.next_delay(jitter());
        while !remaining.is_zero() {
            let in_secs = u32::try_from(remaining.as_millis().div_ceil(1000)).unwrap_or(u32::MAX);
            if out.send(Event::Reconnecting { in_secs }).await.is_err() {
                return;
            }
            let step = remaining.min(Duration::from_secs(1));
            sleep(step).await;
            remaining -= step;
        }
    }
}

/// O próximo heartbeat; antes do `HELLO`, nunca.
async fn tick(beat: &mut Option<Interval>) {
    match beat {
        Some(beat) => {
            beat.tick().await;
        }
        None => std::future::pending().await,
    }
}

/// De 0.0 a 1.0, para os clients não reconectarem todos no mesmo instante. Não precisa ser
/// aleatório de verdade.
fn jitter() -> f64 {
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    f64::from(nanos.subsec_nanos()) / 1e9
}
