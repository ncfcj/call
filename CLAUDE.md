# Call

MVP de chat de texto e voz estilo Discord, num monorepo Rust. Veja o `README.md` e o `Plano de MVP.md`.

## Agent skills

### Issue tracker

Issues e specs ficam como arquivos markdown em `.scratch/<feature>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Os cinco rótulos padrão (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`), registrados numa linha `Status:` de cada issue. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: um `GLOSSARY.md` e `docs/adr/` na raiz. See `docs/agents/domain.md`.

## Busca no código: CodeGraph

Use sempre o CodeGraph para qualquer busca nos arquivos do repositório (símbolos, definições,
quem chama o quê, impacto de uma mudança, estrutura de pastas). Prefira-o ao Grep, ao Glob e ao
`grep`/`find` no shell.

- Pelo MCP: a ferramenta `codegraph_explore`, configurada em `.mcp.json`.
- Pela CLI, quando o MCP não estiver carregado: `codegraph explore <consulta>`,
  `codegraph query <símbolo>`, `codegraph node <símbolo ou arquivo>`, `codegraph callers <símbolo>`,
  `codegraph impact <símbolo>` e `codegraph files`.
- O índice fica em `.codegraph/` (fora do git). O servidor MCP sincroniza sozinho; pela CLI, rode
  `codegraph sync` depois de mudanças grandes.
- Grep só para texto que o índice não cobre: Markdown, `.env`, logs, CSS e mensagens de erro.

