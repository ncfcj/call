#!/bin/bash
# Instala ou atualiza o Call nesta VM (Ubuntu 24.04), como root. Chamado pelo scripts/deploy.ps1, que
# deixa os arquivos na pasta deste script. Pode rodar quantas vezes for preciso.
set -euo pipefail

dominio=$1
cd "$(dirname "$0")"
if [[ ! $dominio =~ ^[a-z0-9.-]+$ ]]; then
    echo "domínio inválido: $dominio" >&2
    exit 1
fi

if ! command -v caddy >/dev/null; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -q
    apt-get install -yq caddy ufw unattended-upgrades curl openssl
fi

# Firewall e SSH só por chave. Sem nenhuma chave instalada, desligar a senha trancaria a VM.
ufw allow 22/tcp >/dev/null
ufw allow 80/tcp >/dev/null
ufw allow 443/tcp >/dev/null
ufw --force enable >/dev/null
if [[ -s $(getent passwd "${SUDO_USER:-root}" | cut -d: -f6)/.ssh/authorized_keys ]]; then
    printf 'PasswordAuthentication no\nKbdInteractiveAuthentication no\n' \
        >/etc/ssh/sshd_config.d/00-call.conf
    sshd -t && systemctl reload ssh
else
    echo "AVISO: ${SUDO_USER:-root} sem chave SSH; o login por senha continua ligado" >&2
fi

mkdir -p /etc/call /opt/call
if [[ ! -f /etc/call/call.env ]]; then
    (
        umask 077
        sed -e "s|__DOMINIO__|$dominio|g" -e "s|__JWT__|$(openssl rand -hex 48)|" \
            call.env.example >/etc/call/call.env
    )
fi
if grep -q =PREENCHA /etc/call/call.env; then
    echo "Falta preencher: rode 'sudo nano /etc/call/call.env' na VM e faça o deploy de novo." >&2
    exit 2
fi

# Guarda a versão atual para voltar se a nova não subir.
[[ -f /opt/call/call-server ]] && cp -p /opt/call/call-server /opt/call/call-server.antes
install -m 755 call-server /opt/call/call-server.novo
mv -f /opt/call/call-server.novo /opt/call/call-server
install -m 644 call-server.service /etc/systemd/system/call-server.service
systemctl daemon-reload
systemctl enable -q call-server
systemctl restart call-server

saudavel() {
    for _ in $(seq 30); do
        sleep 1
        curl -fsS http://127.0.0.1:3000/health 2>/dev/null | grep -q '"database":true' && return 0
    done
    return 1
}
if ! saudavel; then
    journalctl -u call-server -n 30 --no-pager >&2
    if [[ -f /opt/call/call-server.antes ]]; then
        # shortcut: as migrations novas já rodaram e não voltam; só mude o banco de forma compatível.
        mv -f /opt/call/call-server.antes /opt/call/call-server
        systemctl restart call-server
        echo "A versão nova não subiu; voltei para a anterior." >&2
    fi
    exit 1
fi

# O client só troca depois da API no ar.
rm -rf /opt/call/web.novo
cp -r web /opt/call/web.novo
chmod -R a+rX /opt/call/web.novo
rm -rf /opt/call/web
mv /opt/call/web.novo /opt/call/web
sed "s|__DOMINIO__|$dominio|g" Caddyfile >/etc/caddy/Caddyfile
caddy validate --adapter caddyfile --config /etc/caddy/Caddyfile >/dev/null 2>&1
systemctl enable -q caddy
systemctl reload-or-restart caddy
echo "No ar: https://$dominio"
