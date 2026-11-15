#!/usr/bin/env python3
"""Passe à la version suivante : chaque chiffre va de 0 à 9, avec retenue (0.0.9 → 0.1.0).

    tool/version.py            affiche la version et la suivante
    tool/version.py --bump     écrit la suivante dans Cargo.toml, Cargo.lock, CHANGELOG.md
                               (« ## À venir » devient « ## X.Y.Z — date ») et le metainfo Flatpak
"""
import datetime
import pathlib
import re
import sys

RACINE = pathlib.Path(__file__).resolve().parent.parent


def suivante(v: str) -> str:
    x, y, z = (int(n) for n in v.split("."))
    assert all(0 <= n <= 9 for n in (x, y, z)), f"version hors règle : {v}"
    n = x * 100 + y * 10 + z + 1
    assert n <= 999, "9.9.9 est la dernière version possible"
    return f"{n // 100}.{n // 10 % 10}.{n % 10}"


def remplacer(chemin: str, motif: str, par: str) -> None:
    p = RACINE / chemin
    texte, n = re.subn(motif, par, p.read_text(), count=1, flags=re.M)
    assert n == 1, f"{chemin} : motif introuvable ({motif})"
    p.write_text(texte)


def main() -> None:
    cargo = (RACINE / "Cargo.toml").read_text()
    v = re.search(r'^version = "(\d\.\d\.\d)"', cargo, re.M).group(1)
    s = suivante(v)
    if "--bump" not in sys.argv:
        print(f"{v} → {s}")
        return
    jour = datetime.date.today().isoformat()
    remplacer("Cargo.toml", rf'^version = "{re.escape(v)}"', f'version = "{s}"')
    remplacer("Cargo.lock", rf'(name = "lkatta"\nversion = "){re.escape(v)}"', rf'\g<1>{s}"')
    remplacer("CHANGELOG.md", r"^## À venir$", f"## {s} — {jour}")
    remplacer(
        "packaging/flatpak/io.github.raantss18.LKatta.metainfo.xml",
        r"^(\s*)<releases>\n",
        rf'\g<0>\g<1>  <release version="{s}" date="{jour}"/>\n',
    )
    print(f"{v} → {s}")


if __name__ == "__main__":
    main()
