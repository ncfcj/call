// Hook PreToolUse: no repositório Call, busca em código passa pelo CodeGraph (ver CLAUDE.md).
//
// Bloqueia Grep, Glob e grep/rg/find/Select-String/git grep no shell quando o alvo é código do
// repositório. Deixa passar: arquivos que o índice não cobre (Markdown, TOML, YAML, CSS, .env,
// logs...), buscas fora do repositório (ex.: dependências em ~/.cargo) e grep que só filtra a
// saída de outro comando (`cargo test | grep ...`).
import { readFileSync } from "node:fs";
import path from "node:path";

const NOT_INDEXED = /\.(md|markdown|toml|ya?ml|css|env|log|txt|lock|json|sql|html|ps1|sh|example)$|(^|[\\/])\.env(\.[\w-]+)?$|(^|[\\/])\.gitignore$/i;
const CODE_EXT = /\.(rs|js|mjs|ts|tsx|jsx)\b/i;

let input;
try {
  input = JSON.parse(readFileSync(0, "utf8"));
} catch {
  process.exit(0); // entrada inesperada: não trava o trabalho
}
const tool = input.tool_name;
const args = input.tool_input ?? {};
const repo = path.resolve(input.cwd ?? process.cwd());

const norm = (p) => p.replace(/^\/([a-z])\//i, "$1:/").replace(/^~(?=[\\/]|$)/, process.env.USERPROFILE ?? process.env.HOME ?? "~");
const inRepo = (p) => {
  if (!p) return true;
  const abs = path.resolve(repo, norm(p));
  const rel = path.relative(repo, abs);
  return !rel.startsWith("..") && !path.isAbsolute(rel) && !rel.split(/[\\/]/).includes(".git");
};
const notIndexed = (s) => NOT_INDEXED.test(s) && !CODE_EXT.test(s);

function block(why) {
  process.stdout.write(JSON.stringify({
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason:
        `Neste repositório, busca em código usa o CodeGraph (CLAUDE.md): ${why} ` +
        "Use a ferramenta codegraph_explore ou a CLI (`codegraph explore <consulta>`, " +
        "`codegraph query <símbolo>`, `codegraph node <símbolo ou arquivo>`, `codegraph callers <símbolo>`, " +
        "`codegraph files`). Grep fica para Markdown, TOML, YAML, CSS, .env e logs, ou fora do repositório.",
    },
  }));
  process.exit(0);
}

if (tool === "Grep") {
  const target = args.path ?? "";
  const filter = args.glob ?? args.type ?? "";
  if (!inRepo(target)) process.exit(0);
  if (notIndexed(target) || (filter && notIndexed(filter.replace(/[{}*]/g, "")))) process.exit(0);
  if (["md", "toml", "yaml", "css", "json", "log"].includes(args.type)) process.exit(0);
  block("Grep em código do repositório.");
}

if (tool === "Glob") {
  if (!inRepo(args.path ?? "")) process.exit(0);
  const pattern = args.pattern ?? "";
  if (notIndexed(pattern.replace(/[{}*]/g, ""))) process.exit(0);
  block("Glob para achar arquivos de código (`codegraph files` lista a estrutura).");
}

if (tool === "Bash" || tool === "PowerShell") {
  // O corpo de um heredoc é dado (script, texto), não comando: não é analisado.
  const command = (args.command ?? "").replace(
    /<<-?\s*(['"]?)(\w+)\1[^\n]*\n[\s\S]*?\n\s*\2\s*(?=\n|$)/g,
    "",
  );
  // Cada comando de uma sequência (&&, ||, ;, quebra de linha); dentro dele, só o 1º trecho
  // antes de um pipe busca em arquivos. Os trechos depois de `|` filtram saída e passam.
  for (const statement of command.split(/&&|\|\||;|\n/)) {
    const first = statement.split("|")[0].trim();
    // Comandos de shell são minúsculos; só o cmdlet do PowerShell ignora maiúsculas.
    const m =
      first.match(/^(?:\w+=\S+\s+)*(grep|egrep|rg|find|git\s+grep)\b(.*)$/) ??
      first.match(/^(Select-String|sls)\b(.*)$/i);
    if (!m) continue;
    const tokens = (m[2].match(/"[^"]*"|'[^']*'|\S+/g) ?? []).map((t) => t.replace(/^["']|["']$/g, ""));
    const args = tokens.filter((t) => !t.startsWith("-"));
    // No grep/rg/git grep o 1º argumento é o padrão, não um caminho (a menos que venha com -e).
    const patternFirst = /grep|rg/.test(m[1]) && !tokens.some((t) => /^-(e|f|-regexp)/.test(t));
    const paths = (patternFirst ? args.slice(1) : args).filter((t) => /[\\/.~$]/.test(t));
    const searched = paths.length ? paths : ["."];
    // Busca só fora do repositório, ou só em arquivos que o índice não cobre: passa.
    if (searched.every((p) => !inRepo(p.replace(/\$\{?(HOME|TMP|TEMP|USERPROFILE)\}?/, process.env.USERPROFILE ?? "~")))) continue;
    if (searched.every((p) => notIndexed(p)) || tokens.some((t) => /--include=\S*\.(md|toml|ya?ml|css|log)\b/.test(t) && !/\.(rs|js|ts)/.test(t))) continue;
    block(`\`${m[1]}\` em arquivos do repositório.`);
  }
}

process.exit(0);
