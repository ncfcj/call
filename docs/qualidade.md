# Qualidade e segurança do código

Tudo aqui é gratuito e de código aberto. O CI (`.github/workflows/ci.yml`) roda em cada PR e em
cada push na `main`; o check **`ci-ok`** só fica verde se todos os portões passarem.

## Portões do CI

| Job | Garante | Ferramenta | Falha quando |
| --- | --- | --- | --- |
| `lint` | Formatação e análise estática nos dois alvos (nativo e wasm32) | `rustfmt`, `clippy` com `[workspace.lints]` | Código fora do formato ou qualquer aviso do clippy |
| `test` | 100% dos testes passando, unitários e de integração com Postgres; mede a cobertura | `cargo test` via `cargo-llvm-cov` | Qualquer teste falhar |
| `web` | O client compila em release para o navegador | `trunk` | O build falhar |
| `supply-chain` | Dependências sem vulnerabilidade conhecida, com licença compatível, só do crates.io | `cargo-deny` (`deny.toml`) | Vulnerabilidade, crate sem manutenção, licença fora da lista, origem desconhecida |
| `mutation` | Os testes pegam mudanças no código: nenhuma mutação sobrevive | `cargo-mutants` (`.cargo/mutants.toml`) | Alguma mutação não for pega por nenhum teste |
| `ci-ok` | Resumo para a proteção de branch | Actions | Qualquer job acima não passar |

Na aba **Summary** de cada execução aparecem a cobertura por arquivo e, se houver, as mutações
que sobreviveram.

### Análise estática

As regras ficam em `[workspace.lints]` no `Cargo.toml` e valem para todos os crates:

- `unsafe_code`, `unused_must_use`: proibidos.
- `dbg_macro`, `todo`, `unimplemented`, `print_stdout`, `print_stderr`: nada de restos de
  depuração ou código inacabado (o servidor loga com `tracing`).
- `cast_possible_truncation`, `cast_sign_loss`, `cast_possible_wrap`: conversões numéricas que
  podem perder ou estourar sem avisar (use `try_from` ou aritmética saturada).
- Clareza: `redundant_clone`, `needless_pass_by_value`, `manual_let_else`,
  `uninlined_format_args`, entre outras.

Uma exceção precisa de `#[allow(...)]` no lugar, com um comentário dizendo por quê.

### Testes de mutação

O `cargo-mutants` troca operadores, devolve valores fixos e apaga chamadas, uma mudança por vez,
e roda os testes. Se nenhum teste falhar, a mutação "sobreviveu": é um comportamento que nenhum
teste verifica. Em PRs, o CI testa só as linhas alteradas (`--in-diff`); toda segunda, o
workspace inteiro.

Ficam de fora (em `.cargo/mutants.toml`) o código que só compila para o navegador
(`crates/voice/src/room.rs`, `crates/api-client/src/ws.rs` e `web/`) e o `main.rs` do servidor.
Eles são cobertos pelos testes no navegador de cada etapa.

### Dependências

`deny.toml` lista as licenças aceitas e as exceções de vulnerabilidade, cada uma com o motivo:

| Exceção | Motivo |
| --- | --- |
| RUSTSEC-2023-0071 (`rsa`) | Vem do `jsonwebtoken`, mas o projeto só usa HMAC (HS256); o código RSA nunca roda |
| RUSTSEC-2026-0387 (`crypto-bigint`) | Idem, para ECDSA (p256/p384) |
| RUSTSEC-2026-0543 (`chrono` sem manutenção) | Vem também do `sqlx` e da `livekit-api`; migrar só o nosso código não o remove |

O **Dependabot** (`.github/dependabot.yml`) abre PRs semanais com as atualizações do Cargo e
das Actions, e eles passam pelos mesmos portões.

## Rodando localmente

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p proto -p api-client -p call-core -p voice -p web --all-targets --target wasm32-unknown-unknown -- -D warnings
cargo test --workspace                      # precisa do docker compose up -d
cargo llvm-cov --workspace --summary-only   # cobertura (cargo install cargo-llvm-cov)
cargo deny check                            # cargo install cargo-deny
cargo mutants -p call-core                  # cargo install cargo-mutants
```

**Mutação no Windows com antivírus:** o `cargo-mutants` compila cópias do projeto numa pasta
temporária, e o Kaspersky bloqueia os scripts de build de lá ("Acesso negado"). Aponte a pasta
temporária para dentro de `~/.cargo-target` e tire o `CARGO_TARGET_DIR` (com ele, as cópias
dividem o mesmo cache de build e o resultado fica errado):

```powershell
$env:TEMP = "$HOME\.cargo-target\mutants-tmp"; $env:TMP = $env:TEMP
Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
cargo mutants -p call-core -j 3
```

## Bloquear o merge sem o CI verde

O GitHub só impede o merge com a **proteção de branch** (ou um ruleset) exigindo o check
`ci-ok`. Em repositório **privado** de conta gratuita esse recurso não existe; as opções são:

1. Assinar o **GitHub Pro** (pago) e ativar a proteção da `main`.
2. Tornar o repositório **público**: a proteção fica gratuita, e também o CodeQL e o SonarQube
   Cloud.
3. Manter privado e gratuito: o CI continua marcando o PR em vermelho, mas o botão de merge não
   é bloqueado. A regra vira disciplina.

Com a proteção disponível, exija na `main`: o check `ci-ok`, PR antes do merge, branch
atualizada com a `main` e nenhum push direto.
