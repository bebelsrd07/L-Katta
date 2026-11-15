#!/usr/bin/env bash
# Vérifie que le binaire démarre sur les distributions courantes : aucun symbole obligatoire
# de glibc plus récent que $2 (2.35 par défaut : Ubuntu 22.04, Debian 12 a 2.36).
#   tool/glibc-max.sh target/release/lkatta [2.35]
set -euo pipefail
bin="${1:?binaire}"
max="${2:-2.35}"
# Symboles non faibles (les faibles, comme pidfd_spawnp, sont facultatifs à l'exécution).
trop=$(objdump -T "$bin" | awk '$2 != "w" && /GLIBC_[0-9]/ { match($0, /GLIBC_[0-9.]+/); v = substr($0, RSTART + 6, RLENGTH - 6); print v, $NF }' \
  | while read -r v nom; do
      [ "$(printf '%s\n%s\n' "$v" "$max" | sort -V | tail -n1)" = "$max" ] || echo "$nom (glibc $v)"
    done)
if [ -n "$trop" ]; then
  echo "Ce binaire exige une glibc plus récente que $max :" >&2
  echo "$trop" >&2
  echo "(voir src/glibc.rs pour fournir ces fonctions)" >&2
  exit 1
fi
echo "glibc : rien au-delà de $max"
