#!/usr/bin/env bash
# Met les paquets AUR (packaging/aur/lkatta-bin, packaging/aur/lkatta) à la version publiée :
# pkgver, empreintes, .SRCINFO. À lancer APRÈS tool/publier.sh.
#
#   tool/aur.sh              prépare les fichiers (rien n'est envoyé)
#   tool/aur.sh --envoyer    les pousse aussi sur aur.archlinux.org (compte AUR et clé SSH
#                            enregistrée dans le compte nécessaires ; seulement après un « oui »)
set -euo pipefail
cd "$(dirname "$0")/.."
v="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
url="https://github.com/raantss18/L-Katta"

# Binaire : l'archive de la version, empreinte prise dans dist/SHA256SUMS (signé).
bin_sum="$(grep " lkatta-$v-x86_64-linux.tar.gz\$" dist/SHA256SUMS | cut -d' ' -f1)"
[ -n "$bin_sum" ] || { echo "dist/SHA256SUMS ne connaît pas lkatta-$v : publier d'abord" >&2; exit 1; }
# Sources : l'archive que GitHub fabrique pour l'étiquette.
src_sum="$(curl -fsSL "$url/archive/refs/tags/v$v.tar.gz" | sha256sum | cut -d' ' -f1)"

maj() { # dossier empreinte
  sed -i -e "s/^pkgver=.*/pkgver=$v/" -e "s/^pkgrel=.*/pkgrel=1/" -e "s/^sha256sums=.*/sha256sums=('$2')/" "$1/PKGBUILD"
  (cd "$1" && makepkg --printsrcinfo > .SRCINFO)
}
maj packaging/aur/lkatta-bin "$bin_sum"
maj packaging/aur/lkatta "$src_sum"
echo "Paquets AUR à la version $v (packaging/aur)."

[ "${1:-}" = "--envoyer" ] || exit 0
for p in lkatta-bin lkatta; do
  d="$(mktemp -d)"
  git clone -q "ssh://aur@aur.archlinux.org/$p.git" "$d"
  cp "packaging/aur/$p/PKGBUILD" "packaging/aur/$p/.SRCINFO" "$d/"
  (cd "$d" && git add PKGBUILD .SRCINFO && { git diff --cached --quiet || git commit -qm "Version $v"; } && git push -q origin HEAD:master)
  rm -rf "$d"
  echo "AUR : $p $v envoyé."
done
