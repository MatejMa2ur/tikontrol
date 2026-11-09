#!/bin/sh
# Prepares a fresh CHR from src/scripts/chr.sh: admin password (random, kept in $DIR/admin.pw), an API-SSL
# certificate, and the home-router config from chr-defconf.rsc (CHR_DEFCONF=0 skips it). Honours CHR_DIR and
# CHR_OFFSET like chr.sh. Also writes $DIR/ssh-chr.sh for later commands.
set -eu
DIR=${CHR_DIR:-$HOME/.cache/tikontrol-chr}
SSH_PORT=$((2222 + ${CHR_OFFSET:-0}))
HERE=$(cd "$(dirname "$0")" && pwd)
cd "$DIR"
umask 077
[ -f admin.pw ] || openssl rand -hex 12 > admin.pw
printf '#!/bin/sh\necho ""\n' > askpass-empty.sh
printf '#!/bin/sh\ncat "%s/admin.pw"\n' "$DIR" > askpass-pw.sh
chmod 700 askpass-empty.sh askpass-pw.sh
cat > ssh-chr.sh <<EOF
#!/bin/sh
SSH_ASKPASS=$DIR/askpass-pw.sh SSH_ASKPASS_REQUIRE=force exec ssh -q -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10 -p $SSH_PORT admin@127.0.0.1 "\$@"
EOF
chmod 700 ssh-chr.sh
ssh_empty() { SSH_ASKPASS=$DIR/askpass-empty.sh SSH_ASKPASS_REQUIRE=force ssh -q -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10 -p $SSH_PORT admin@127.0.0.1 "$@"; }
echo "waiting for CHR to boot..."
i=0
until ssh_empty ':put ok' 2>/dev/null | grep -q ok || ./ssh-chr.sh ':put ok' 2>/dev/null | grep -q ok; do
  i=$((i + 1)); [ $i -gt 90 ] && { echo "CHR did not come up"; exit 1; }; sleep 2
done
if ssh_empty ':put ok' 2>/dev/null | grep -q ok; then
  ssh_empty "/user set admin password=\"$(cat admin.pw)\""
fi
run() { ./ssh-chr.sh "$(grep -v '^#' "$1" | grep -v '^$' | tr '\n' ';' | sed 's/;$//')"; }
if [ -z "$(./ssh-chr.sh ':put [/certificate find name=api]' | tr -d "[:space:]")" ]; then
  ./ssh-chr.sh '/certificate add name=tik-ca common-name=tik-ca key-usage=key-cert-sign,crl-sign; /certificate sign tik-ca; :delay 3; /certificate add name=api common-name=chr.test subject-alt-name=IP:127.0.0.1; /certificate sign api ca=tik-ca; :delay 3; /ip/service set api-ssl certificate=api disabled=no; /system/identity set name=chr-test' >/dev/null
fi
if [ "${CHR_DEFCONF:-1}" = 1 ] && [ -z "$(./ssh-chr.sh ':put [/interface/bridge find name=bridge]' | tr -d "[:space:]")" ]; then
  run "$HERE/chr-defconf.rsc"
fi
echo "CHR ready: api-ssl on 127.0.0.1:$((8729 + ${CHR_OFFSET:-0})), ssh via $DIR/ssh-chr.sh"
