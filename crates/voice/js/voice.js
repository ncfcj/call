// Ponte fina entre o crate `voice` (Rust) e o livekit-client (global `LivekitClient`, carregado
// por web/vendor/livekit-client-*.umd.js).
//
// É um script comum (não um módulo) carregado pelo web/index.html, que expõe a classe no global
// `CallVoice`. Assim o Trunk põe o hash do conteúdo no nome do arquivo e cada versão tem uma URL
// nova. Como "snippet" do wasm-bindgen, a URL não mudava, e um navegador com a versão antiga em
// cache recusava o arquivo novo (integridade) e o app abria em branco.
//
// Este é o único arquivo que conhece a API do LiveKit. Para o Rust ele expõe uma classe com
// poucos métodos e um callback de eventos, sempre em JSON `{"t": "TIPO", "d": ...}`, que o lado
// Rust desserializa em `VoiceEvent`. Mantenha os dois lados em sincronia.

class VoiceSession {
  constructor(onEvent) {
    this.onEvent = onEvent;
    this.room = null;
    // Deaf: o áudio dos outros fica mudo, inclusive o de quem entrar depois.
    this.deafened = false;
  }

  emit(t, d) {
    this.onEvent(JSON.stringify(d === undefined ? { t } : { t, d }));
  }

  // Quem está na sala: você e os outros, com o estado do microfone de cada um.
  participants() {
    const room = this.room;
    if (!room) return [];
    const describe = (p, local) => {
      const mic = p.getTrackPublication(globalThis.LivekitClient.Track.Source.Microphone);
      return {
        identity: p.identity,
        name: p.name || p.identity,
        local,
        mic: !!mic && !mic.isMuted,
      };
    };
    return [
      describe(room.localParticipant, true),
      ...Array.from(room.remoteParticipants.values()).map((p) => describe(p, false)),
    ];
  }

  emitParticipants() {
    this.emit("PARTICIPANTS", this.participants());
  }

  // O id, se o dispositivo ainda existir; senão "" (padrão do sistema).
  static async present(kind, id) {
    if (!id) return "";
    try {
      const devices = await navigator.mediaDevices.enumerateDevices();
      return devices.some((d) => d.kind === kind && d.deviceId === id) ? id : "";
    } catch (_) {
      return "";
    }
  }

  // Microfones e saídas de áudio, em JSON. Pede a permissão do microfone se ainda não houver,
  // porque sem ela o navegador esconde os nomes dos dispositivos.
  static async listDevices() {
    const LK = globalThis.LivekitClient;
    const describe = (d) => ({ id: d.deviceId, label: d.label || "" });
    let inputs = [];
    let outputs = [];
    try {
      inputs = (await LK.Room.getLocalDevices("audioinput", true)).map(describe);
    } catch (_) {
      // Sem permissão: a lista fica vazia e a UI explica.
    }
    try {
      outputs = (await LK.Room.getLocalDevices("audiooutput", false)).map(describe);
    } catch (_) {}
    return JSON.stringify({
      inputs,
      outputs,
      can_choose_output: LK.supportsAudioOutputSelection(),
    });
  }

  // `micEnabled` e `deafened` são o estado com que a pessoa entra (ela pode entrar mutada).
  // `inputId` e `outputId` são os dispositivos escolhidos ("" = padrão do sistema).
  async connect(url, token, micEnabled, deafened, inputId, outputId) {
    this.deafened = deafened;
    // O LiveKit pede o dispositivo como `exact`: um headset salvo e depois desconectado faria o
    // microfone falhar. Dispositivo que sumiu volta ao padrão do sistema.
    inputId = await VoiceSession.present("audioinput", inputId);
    outputId = await VoiceSession.present("audiooutput", outputId);
    const LK = globalThis.LivekitClient;
    if (!LK) throw new Error("o livekit-client não carregou");

    const room = new LK.Room({
      // O processamento do navegador evita eco e microfonia. Não desligue.
      audioCaptureDefaults: {
        echoCancellation: true,
        noiseSuppression: true,
        autoGainControl: true,
        deviceId: inputId || undefined,
      },
      audioOutput: outputId ? { deviceId: outputId } : undefined,
      adaptiveStream: false,
      dynacast: false,
    });
    this.room = room;
    const E = LK.RoomEvent;
    const update = () => this.emitParticipants();

    room
      .on(E.ParticipantConnected, update)
      .on(E.ParticipantDisconnected, (p) => {
        this.removeAudio((el) => el.dataset.participant === p.identity);
        update();
      })
      .on(E.TrackMuted, update)
      .on(E.TrackUnmuted, update)
      .on(E.LocalTrackPublished, update)
      .on(E.LocalTrackUnpublished, update)
      .on(E.TrackPublished, update)
      .on(E.TrackUnpublished, update)
      .on(E.TrackSubscribed, (track, publication, participant) => {
        // O áudio remoto toca num <audio> escondido, marcado para ser achado na saída.
        if (track.kind === "audio") {
          const el = track.attach();
          el.dataset.voice = "remote";
          el.dataset.participant = participant.identity;
          el.dataset.track = publication.trackSid;
          el.muted = this.deafened;
          document.body.appendChild(el);
        }
        update();
      })
      .on(E.TrackUnsubscribed, (track, publication) => {
        // Quando a pessoa sai, o LiveKit já fez o detach; por isso remove pela marca.
        track.detach();
        this.removeAudio((el) => el.dataset.track === publication.trackSid);
        update();
      })
      .on(E.ActiveSpeakersChanged, (speakers) =>
        this.emit("ACTIVE_SPEAKERS", speakers.map((s) => s.identity)),
      )
      .on(E.Reconnecting, () => this.emit("RECONNECTING"))
      .on(E.Reconnected, () => {
        this.emit("RECONNECTED");
        update();
      })
      .on(E.Disconnected, (reason) => {
        this.cleanup();
        this.emit("DISCONNECTED", { reason: String(reason ?? "") });
      })
      .on(E.AudioPlaybackStatusChanged, () => this.emit("AUDIO_BLOCKED", !room.canPlaybackAudio));

    await room.connect(url, token);
    try {
      // Mutado na entrada: nem pede o microfone; ele só abre quando a pessoa desmutar.
      if (micEnabled) await room.localParticipant.setMicrophoneEnabled(true);
    } catch (err) {
      // Sem microfone (negado ou ausente), a pessoa continua na sala só ouvindo.
      this.emit("MIC_ERROR", { message: String((err && err.message) || err) });
    }
    await this.startAudio();
    update();
  }

  async setMicrophoneEnabled(enabled) {
    if (!this.room) return;
    await this.room.localParticipant.setMicrophoneEnabled(enabled);
    this.emitParticipants();
  }

  // Troca o microfone ("audioinput") ou a saída ("audiooutput") durante a chamada.
  async switchDevice(kind, deviceId) {
    if (!this.room) return;
    const id = await VoiceSession.present(kind, deviceId);
    await this.room.switchActiveDevice(kind, id || "default");
  }

  // Deaf: silencia (ou volta) o áudio de todos os outros, sem sair da sala.
  setDeafened(deafened) {
    this.deafened = deafened;
    document.querySelectorAll('audio[data-voice="remote"]').forEach((el) => {
      el.muted = deafened;
    });
  }

  // Destrava o áudio quando o navegador bloqueou a reprodução automática. Chame num clique.
  async startAudio() {
    if (!this.room) return;
    try {
      await this.room.startAudio();
    } catch (_) {
      // Continua bloqueado; o evento AUDIO_BLOCKED avisa a UI.
    }
    this.emit("AUDIO_BLOCKED", !this.room.canPlaybackAudio);
  }

  async disconnect() {
    const room = this.room;
    if (room) await room.disconnect();
    this.cleanup();
  }

  removeAudio(matches) {
    document
      .querySelectorAll('audio[data-voice="remote"]')
      .forEach((el) => matches(el) && el.remove());
  }

  cleanup() {
    this.removeAudio(() => true);
  }
}

globalThis.CallVoice = { VoiceSession };
