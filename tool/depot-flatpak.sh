#!/usr/bin/env bash
# Prépare le dépôt Flatpak de L Katta, servi par le site (GitHub Pages) :
#
#   tool/depot-flatpak.sh [dépôt OSTree] [archive]
#       signe repo-flatpak avec la clé GPG du dépôt et écrit dist/flatpak-depot.tar.gz
#       (dossier repo/ + lkatta.flatpakrepo). Le flux « Site vitrine » le dépose sous /flatpak/.
#
# La clé publique est dans packaging/flatpak/lkatta-depot.gpg ; la clé privée reste dans le
# trousseau GPG de la personne qui publie (jamais dans Git). La créer une fois :
#
#   gpg --quick-generate-key "L Katta (dépôt Flatpak)" ed25519 sign never
#   gpg --export "L Katta (dépôt Flatpak)" > packaging/flatpak/lkatta-depot.gpg
#
# Sans clé publique, rien n'est produit (code 3) : la version part avec le seul paquet .flatpak.
set -euo pipefail
cd "$(dirname "$0")/.."

depot="${1:-repo-flatpak}"
archive="${2:-dist/flatpak-depot.tar.gz}"
pub="${LKATTA_DEPOT_GPG:-packaging/flatpak/lkatta-depot.gpg}"
url="${LKATTA_DEPOT_URL:-https://raantss18.github.io/L-Katta/flatpak/repo/}"
id="io.github.raantss18.LKatta"

[ -d "$depot" ] || { echo "dépôt OSTree introuvable : $depot" >&2; exit 1; }
[ -f "$pub" ] || { echo "(pas de clé $pub : pas de dépôt Flatpak, voir tool/depot-flatpak.sh)" >&2; exit 3; }
empreinte="$(gpg --show-keys --with-colons "$pub" | awk -F: '/^fpr/{print $10; exit}')"
[ -n "$empreinte" ] || { echo "clé publique illisible : $pub" >&2; exit 1; }
gpg --list-secret-keys "$empreinte" >/dev/null 2>&1 || { echo "la clé privée $empreinte n'est pas dans ce trousseau GPG" >&2; exit 1; }

# Signe la version et le sommaire ; ne garde que la dernière version de chaque branche.
flatpak build-sign "$depot" --gpg-sign="$empreinte"
flatpak build-update-repo "$depot" --gpg-sign="$empreinte" --title="L Katta" --default-branch=master --prune --prune-depth=0 >/dev/null

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
cp -a "$depot" "$tmp/repo"
rm -rf "$tmp/repo/tmp" "$tmp/repo/.lock"
cat > "$tmp/lkatta.flatpakrepo" <<FIN
[Flatpak Repo]
Title=L Katta
Url=$url
Homepage=https://raantss18.github.io/L-Katta/
Comment=Dépôt officiel de L Katta
Description=Le petit maki assistant de bureau
DefaultBranch=master
GPGKey=$(base64 -w0 "$pub")
FIN
mkdir -p "$(dirname "$archive")"
tar czf "$archive" -C "$tmp" repo lkatta.flatpakrepo
echo "$archive"
