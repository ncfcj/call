# Dependências JavaScript guardadas no repositório

O projeto não usa npm. Os arquivos aqui são cópias exatas de pacotes publicados, conferidas
pelo hash `integrity` do registro do npm.

| Arquivo | Pacote | Origem | Licença |
| --- | --- | --- | --- |
| `livekit-client-2.25.0.umd.js` | `livekit-client` 2.25.0, `dist/livekit-client.umd.js` (só sem a última linha, a do source map) | https://registry.npmjs.org/livekit-client/-/livekit-client-2.25.0.tgz, `sha512-QrXeBqrTCPujCu7uMmT9DXOIYTCGz0Ig90EGJx4B05fhQX3BLZJzAbEpRqvFwhQZyQV5uf2KeYsjeVo6as1kZw==` | Apache-2.0 (`livekit-client.LICENSE`) |

O build UMD já traz as dependências do pacote e expõe o global `LivekitClient`. Quem usa é a
ponte em `crates/voice/js/voice.js`.

Para atualizar: baixe o `.tgz` da nova versão, confira o `sha512` com o campo `dist.integrity`
de `https://registry.npmjs.org/livekit-client/<versão>`, copie `dist/livekit-client.umd.js` e
atualize o nome do arquivo em `web/index.html` e esta tabela.
