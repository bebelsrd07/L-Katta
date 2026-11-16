# Plan — L-Katta × coucou-bottom

> Document de travail. Réponses simples d'abord, détails techniques seulement là où c'est utile.

---

## 1. Ce que j'ai vérifié dans l'espace de travail

| Dépôt | État | Chiffres |
|---|---|---|
| `/home/user/L-Katta` (ton fork, v0.3.2) | ✅ présent, branche `arena/f612355e-l-katta` | 27 322 lignes Rust · **232 crates** · 62 modules · 100 % Linux |
| `github.com/bebelsrd07/coucou-bottom` | ✅ **public, je peux le cloner** | ~43 000 lignes Rust (Tauri) · **626 crates** · 91 fichiers TypeScript · WebView2 |

Donc pas besoin d'archive `.zip` : j'ai bien accès à tes deux projets.

---

## 2. Ta question : « L-Katta n'est-il pas déjà parfait en sécurité, légèreté et nativeté ? »

**Réponse honnête : oui sur deux points, et le troisième est exactement le trou que tu veux combler.**

- **Sécurité → oui, et nettement au-dessus de coucou.** `regles.toml` appliqué *par le code* (pas seulement par le prompt), deux niveaux cumulés machine + utilisateur où le plus restrictif gagne, secrets uniquement dans le trousseau OS, `[CONTENU EXTERNE]`, confirmation explicite pour toute écriture, autonomie enfermée dans un bac bubblewrap sans réseau, mises à jour signées Ed25519. Coucou n'a rien d'équivalent.
- **Légèreté → oui.** 232 crates contre 626 (×2,7). Pas de WebView, pas de tokio, pas de sherpa-onnx. ~15 Mo de RAM.
- **Nativeté → oui sur Linux, non sur Windows.** C'est le seul vrai manque.

**Bonne nouvelle : le code est déjà préparé pour Windows.** Quatre choses existent déjà et rendent le portage *additif* plutôt qu'une réécriture :

1. `src/mascot/mod.rs` sépare déjà **scène commune** (`App::nouvelle`, `App::pointeur`, `Sortie`) et **backend d'affichage** (`App.wl`, `App.x11`).
2. `LKATTA_AFFICHAGE` choisit déjà le backend (wayland / x11) → il suffira d'ajouter `win32`.
3. `crate::hote::commande(..)` encapsule déjà **tous** les appels à des programmes externes.
4. `regles.rs` filtre déjà la liste des outils exposés à l'IA.

→ **On ajoute `src/os/` et `src/mascot/win32.rs`, on ne réécrit pas le maki.**

---

## 3. Faut-il assembler les deux projets ? → **Non. Et c'est la décision la plus importante du plan.**

Si on fusionne les applications, L-Katta perd son **seul** avantage unique. Concrètement :

| | L-Katta | coucou-bottom | Fusion |
|---|---|---|---|
| Interface | tiny-skia, dessin logiciel | WebView2 (Tauri 2) | dépendance au runtime WebView2 |
| Crates | 232 | 626 | ~700 |
| Exécution | synchrone, threads | tokio partout | async imposé |
| RAM | ~15 Mo | plusieurs centaines de Mo | ✗ |
| Binaire | 1 fichier | installeur + WebView | ✗ |

**Ce qu'on fait à la place :** L-Katta reste **l'application hôte unique**. On prend à coucou :

- **1 crate presque tel quel** (le relais de hooks, qui est du Rust pur sans Tauri — voir §4)
- **6 concepts d'interface**
- **0 ligne de code Tauri/WebView**

C'est ça, « combiner les deux » : garder le corps de L-Katta, lui greffer les réflexes de coucou.

---

## 4. Ce qu'on garde, ce qu'on emprunte, ce qu'on jette

### 🟢 On garde intact de L-Katta (on n'y touche pas)

Le maki, la physique, les 6 espèces, le rendu tiny-skia · `regles.rs` + `autonomie.rs` · `[CONTENU EXTERNE]` et les confirmations · le moteur IA gratuit intégré (14 fournisseurs + Ollama) · la mémoire sémantique · l'index FTS5 · le trousseau OS · `maj.rs` signé · `t!()` + `src/lang/en_*.rs`.

### 🟡 On emprunte à coucou-bottom

| # | Quoi | Source chez coucou | Comment | Valeur |
|---|---|---|---|---|
| 1 | **`lkatta-hook`**, le relais que les agents de code appellent | `windows/hook/` — **1 657 lignes, dépendances = `serde_json` seul** (+ `windows`/`libc` minuscules). Zéro Tauri. | portage quasi direct : on renomme le tuyau, on garde le contrôle SID | ⭐⭐⭐ la meilleure prise |
| 2 | **Table de normalisation multi-agents** | `hook/src/normalize.rs`, 267 lignes | portage direct : Claude Code, Cursor, Codex, Gemini CLI, Antigravity, OpenCode, Copilot | ⭐⭐⭐ |
| 3 | **Discipline « ne jamais bloquer l'agent »** | constantes de `hook/src/main.rs` : connexion 300 ms, tir-et-oubli 2 s, accusé de réception 800 ms, décision 110 s ; champs énormes jetés ; longueur plafonnée par champ | on recopie le *design*, pas le code | ⭐⭐⭐ sécurité + robustesse |
| 4 | **Cartes Autoriser / Refuser / Toujours + diff en direct** | `pipe.rs`, `island.rs` | nouveau type de bulle dans `conseils.rs`, **dessiné en tiny-skia** | ⭐⭐ |
| 5 | **Pastilles de services** (GitHub, Notion, n8n…) | `integrations.rs` | badges légers **sur le maki**, pas des sondes permanentes | ⭐⭐ |
| 6 | **Code Notion de référence** (recherche + parsing de page) | `integrations.rs` lignes 969-1060 | simple référence : notre `notion.rs` sera beaucoup plus riche (lecture **et** écriture + cache FTS5) | ⭐ |

### 🔴 On ne prend PAS — et pourquoi

| Écarté | Raison |
|---|---|
| Tauri 2 + WebView2 | tue les 15 Mo et impose un runtime tiers |
| `sherpa-onnx` / Parakeet (dictée) | liaison statique très lourde → **plus tard**, en option `feature` ou via un `whisper.cpp` externe |
| `phone_link/` (serveur TLS, mDNS, relais WebSocket) | 8 crates lourdes pour un compagnon mobile → **phase ultérieure** |
| WhatsApp / GOWA | télécharge un binaire tiers : incompatible avec le modèle de sécurité de L-Katta |
| tokio + reqwest | L-Katta est synchrone avec `ureq` ; on le reste |
| Le concept d'« île » / notch | **le lémurien sur le bord de l'écran EST l'identité de L-Katta** |

---

## 5. Idées d'interface (concret, sans WebView)

1. **Le maki *est* la pastille.** Pas de barre de notch. Un petit point coloré discret sur le maki quand un service a une nouvelle ; un clic ouvre la bulle. Identité préservée.
2. **Carte d'autorisation d'agent.** Dans la bulle : nom de l'agent + session, l'action demandée, le diff `+N −M` en vert/rouge, trois boutons **Autoriser / Refuser / Toujours**, plus **« Répondre dans le terminal »**. Si L-Katta est fermé, le CLI continue normalement — jamais bloqué.
3. **Onglet « Intégrations »** dans les réglages de la bulle : champ de token masqué, bouton **« Tester la connexion »**, choix des bases par défaut, et **un interrupteur par intégration qui écrit dans `regles.toml`** — donc l'interrupteur est appliqué par le code, pas seulement par l'interface.
4. **Recherche unifiée `/trouve`.** Fichiers locaux **et** pages Notion dans le même index FTS5, résultats étiquetés 📁 local / 📄 Notion.
5. **Accueil matinal enrichi.** Rappels locaux + liste à faire + tâches Notion du jour et en retard, dans une seule bulle.
6. **Capture → Notion en un geste.** PDF glissé sur le maki, `/lis` ou `/ecran` → carte « Résumer et envoyer dans Notion ? » avec la base déjà choisie.

---

## 6. Le plan d'exécution

| Étape | Livrable | Vérifiable sur Linux ? | État |
|---|---|---|---|
| **0 — Socle** | `src/os/`, `Cargo.toml` découpé par cible, canal portable, **job CI `windows-latest`** | ✅ oui | 🟢 **finie** : `src/os/` livré (10 concerns dont le canal), `Cargo.toml` découpé, six jumeaux (AT-SPI, GNOME, portail, canal, transport, mascotte). **67 → 44 → 7 → 0 erreur Windows**, et le job `windows` passe en `success` |
| **1 — MVP Windows** | `mascot/win32.rs`, Named Pipes, Credential Manager, outils de base | ⚠️ compile via CI | 🟡 **entamée, mesurée** : `lkatta.exe` compile en release (**5 146 805 octets**) et est publié comme artefact ; le maki s'affiche par `UpdateLayeredWindow`, les commandes passent par un tube nommé vérifié dans les deux sens. Restent la bulle de discussion, le suivi de la fenêtre active, le multi-écrans, Credential Manager et `RegisterHotKey` (§10) |
| **2 — Parité Windows** | discrétion, `controle.rs`, UI Automation, capture écran, Terminal, auto-update `.exe` | ⚠️ compile via CI | ⬜ |
| **3 — Notion natif** | `src/integrations/notion.rs`, 6 outils IA, accueil matinal, `/notion`, cache FTS5, onglet Intégrations | ✅ **oui, entièrement** | 🟢 **6 outils + CLI + onglet Intégrations livrés, CI verte** ; restent accueil matinal, `/notion`, cache FTS5 |
| **4 — Écosystème** | `lkatta-hook` + pont agents, client MCP, webhooks n8n/Make | ✅ oui (sauf le tuyau Windows) | ⬜ |

### L'ordre suivi, et pourquoi

L'étape 3 (Notion) est passée en premier, avant le socle Windows. Trois raisons, qui se sont
vérifiées :

- C'était du **Rust pur, compilé et testé sur Linux** → un résultat vrai dès le premier patch,
  plutôt qu'un portage dont on ne peut rien montrer.
- Ça a créé le squelette `src/integrations/` que le MCP et les webhooks réutiliseront.
- L'étape 0 touche `Cargo.toml` partout : sans compilateur Windows sous la main à chaque patch,
  c'est là qu'on casse les choses en silence.

Et le tout premier commit a été le job CI `windows-latest`, comme prévu : il ne coûte rien et c'est
lui qui a rendu le portage vérifiable (voir §7). Sans lui, aucun des chiffres du §10 n'existerait —
ils viennent tous d'un run, jamais d'une estimation.

---

## 7. ⚠️ Une contrainte que tu dois connaître

**Il n'y a pas de toolchain Rust dans cet espace de travail** : `cargo` et `rustc` sont absents, et `static.rust-lang.org` est bloqué par la liste des hôtes autorisés. Je ne peux donc **pas** lancer `cargo check` ni `cargo test` ici.

**La solution : GitHub Actions devient mon compilateur.** `gh` est authentifié en tant que `bebelsrd07`, donc je peux pousser sur la branche et laisser la CI vérifier pour de vrai :

- `ubuntu-latest` → `cargo test --locked` (déjà dans `.github/workflows/ci.yml`)
- `windows-latest` → nouveau job `cargo check --target x86_64-pc-windows-msvc`, puis la fabrication du `.exe`

Ainsi chaque patch est **réellement compilé et testé**, pas seulement écrit.

Le job `windows` construit maintenant en release et publie `lkatta.exe`, donc le livrable sort de
la CI directement — il n'y a plus à le compiler ailleurs pour l'obtenir.

Une limite à connaître, parce qu'elle a coûté : **le journal et les artefacts d'un run ne se
téléchargent que depuis des hôtes à part**, hors de la liste autorisée ici. La seule information
qui remonte est donc ce qu'un pas de la CI écrit lui-même sur la demande de fusion. D'où le
commentaire automatique du job `windows`, qui est l'unique instrument de mesure du portage — et
dont le format a été écrit pour dire deux choses séparées : ce qui empêche de compiler, et ce qui
compile mais n'est pas encore branché.

---

## 8. Ce qui a été décidé

Ces questions se posaient avant le premier patch. Elles sont tranchées, et le code en porte la
trace — elles ne sont conservées ici que comme explication des choix, pas comme demandes.

1. **Ordre des étapes : 3 (Notion) puis 0 (socle) puis 1 (MVP Windows).** Notion d'abord parce
   que c'était le seul morceau qui ne dépendait pas du portage, et qu'il a donné une CI verte
   complète avant de toucher aux parties sensibles.
2. **Connecteurs : Notion seul pour l'instant.** Les autres attendent que le portage soit fini —
   ajouter une surface réseau avant que le binaire tourne n'aurait fait que multiplier ce qu'il
   faut vérifier d'un coup.
3. **La CI comme compilateur**, faute de toolchain ici.
4. **100 % natif Windows, sans WebView2 ni Tauri.** C'est la contrainte la plus structurante :
   elle a écarté la piste coucou-bottom comme base, et elle impose d'écrire soi-même les
   déclarations Win32 (§10).
5. **`Cargo.lock` inchangé.** Aucune dépendance nouvelle nulle part ; les bibliothèques du système
   (`kernel32`, `user32`, `gdi32`, `advapi32`) sont liées par `#[link]`, et les fonctions sont
   déclarées à la main.
6. **Le code Linux reste identique au octet près quand un jumeau est utilisé.** Les jumeaux
   ajoutent un fichier choisi par `cfg`, ils ne modifient pas l'existant — c'est ce qui permet à la
   CI Linux de rester verte à chaque envoi, et de vérifier cette affirmation par `git diff`.

---

## 9. Écran de verrouillage Windows : ce qui est possible, et ce qui ne l'est pas

Tu as demandé les widgets de verrouillage de Windows, et même l'écran de verrouillage lui-même. Réponse honnête : il y a un mur technique **et** un mur de sécurité, et le second est de ton côté.

### 🔴 Ce que Windows refuse

**Dessiner sur l'écran de verrouillage.** Session verrouillée = Windows bascule sur un *bureau sécurisé* (celui de Winlogon). Aucune application ordinaire n'y est visible, aucune ne peut y dessiner. C'est précisément ce qui protège l'invite de mot de passe et l'UAC : un programme capable de dessiner là pourrait imiter une invite de mot de passe — la technique de base des voleurs d'identifiants. Chercher à le faire, c'est produire un binaire que les antivirus signalent, et casser ta règle n° 2 (le modèle de sécurité intact). **Donc non, et c'est une bonne nouvelle.**

**Les widgets de l'écran de verrouillage de Windows 11** (météo, sport, trafic) viennent de la plateforme Widgets de Microsoft, qui choisit elle-même ce qu'elle y affiche. Il n'existe pas d'API publique pour y placer son propre widget.
*À revérifier documentation en main : depuis cet espace de travail, seuls `github.com`, `pypi.org` et `registry.npmjs.org` sont joignables — pas `learn.microsoft.com`. Ce paragraphe vient donc de ce que je sais, pas de ce que j'ai pu lire aujourd'hui.*

**Les widgets tiers du volet Widgets (Win+W)** existent, mais ce sont des widgets **web** : paquet MSIX, HTML/JS, cartes adaptatives — WebView2 sous le capot. Frontalement contraire à « pas de WebView, un seul binaire léger ».

**Le badge sur l'écran de verrouillage** (la pastille chiffrée des applications UWP) existe vraiment, mais exige un paquet **MSIX** signé plus l'accord de l'utilisateur dans Paramètres › Personnalisation › Écran de verrouillage. MSIX, c'est un installeur et une signature : on perd « un seul `.exe` ». Envisageable un jour comme **option facultative**, pas comme socle.

### 🟢 Ce qui est 100 % natif, sans WebView, et qui donne le même résultat en pratique

1. **Les notifications sur l'écran de verrouillage.** Le seul contenu tiers que Windows affiche vraiment sur le vrai écran de verrouillage — et il ne demande **aucune API spéciale** : émettre des toasts Windows suffit (déjà prévu à l'étape 2, `notifs.rs`). L'utilisateur coche « Afficher les notifications sur l'écran de verrouillage » pour L Katta. Concrètement : « 3 tâches Notion en retard », « Hery t'a écrit », « sauvegarde terminée » — lisibles sans déverrouiller.
2. **Un économiseur d'écran : `lkatta.exe /s`.** La place native du maki en plein écran quand la machine est inactive. Un `.scr` n'est qu'un `.exe` qui répond à `/s` (lancer), `/c` (configurer), `/p` (aperçu) : Win32 pur, aucune dépendance nouvelle, ni paquet ni signature. **Le même binaire** fait l'assistant et l'écran de veille — on reste à « un seul `.exe` ». Beaucoup d'options : espèce, taille, nombre de makis, fond, réveil au mouvement, verrouiller à la reprise.
   *Honnêteté : un économiseur d'écran n'est pas un verrou, il est décoratif. La session n'est protégée que si « À la reprise, afficher l'écran de connexion » est coché — et alors c'est Windows qui verrouille, pas nous.*
3. **Un widget de bureau.** Petite fenêtre toujours au-dessus, épinglée dans un coin : prochaine tâche, tâches Notion du jour, morceau en cours. Même rendu que la bulle (tiny-skia → `UpdateLayeredWindow`), donc rien de nouveau côté dessin. Options : contenu, coin, opacité, masqué en plein écran.
4. **La pastille sur l'icône de la barre des tâches** (`ITaskbarList3::SetOverlayIcon`) : un nombre de non-lus, un point d'enregistrement. Win32 pur.
5. **La liste de sauts** (`ICustomDestinationList`) : clic droit sur l'icône → « Nouvelle note Notion », « Chercher… », « Regarder l'écran ». Win32 pur.

### Ce que je propose

Prendre **1 + 2 + 3 + 4 + 5** comme « intégration native à 100 % avec beaucoup d'options », laisser de côté le dessin direct sur l'écran de verrouillage — pas par manque d'ambition : Windows le refuse, et ta règle de sécurité le refuse aussi. Les notifications sur l'écran de verrouillage donnent l'essentiel, l'information utile visible sans déverrouiller ; l'économiseur d'écran donne le maki en plein écran.

**Ordre concret :** étapes 0 → 1 → 2 d'abord (rien de tout ceci n'existe sans le portage), puis dans l'étape 2 : toasts (1), barre des tâches (4), liste de sauts (5) ; enfin le widget de bureau (3) et l'économiseur d'écran (2), qui sont du dessin pur et se greffent sur `mascot/win32.rs`.

---

## 10. Où en est le portage Windows

Le job CI `windows` compte les erreurs de `cargo check --target x86_64-pc-windows-msvc` et
publie la liste sur la demande de fusion. C'est la seule mesure fiable d'ici : **cargo n'est pas
installé dans cet espace de travail** et ne peut pas l'être, donc rien ne peut être compilé ni
testé localement. Chaque chiffre ci-dessous vient d'un rapport de CI, pas d'une estimation.

| Envoi | Erreurs | Ce qui a changé |
|---|---|---|
| `65571f2` | 67 | `Cargo.toml` découpé par cible : les 9 crates Linux sont sous `[target.'cfg(target_os = "linux")'.dependencies]` |
| `1d9cf54` | 67 | `src/os/` : les droits POSIX, `prctl`, `setsid`, `exec()`, `ioprio` et les chemins en octets passent par le socle (les erreurs « cannot find `unix` in `os` » tombent de 10 à 1) |
| `f71d76b` | 52 | AT-SPI et GNOME ont leur jumeau hors Linux ; `notify-rust` passe par le socle |
| `67c592b` | 44 | Le canal devient portable (`src/os/canal.rs`), le portail de fichiers a son jumeau |
| `05236c8` | 7 | La mascotte a son jumeau Win32 (`src/mascot_win/`), le transport de commandes est séparé du protocole, le service D-Bus de `bureau.rs` passe sous `cfg` |
| `a5ad705` | **0** | Le reste venait de mon propre code Win32 : des poignées `isize` sans `is_null()`, un double emprunt, et `futures-lite` devenu Linux seul. **`cargo build --release` passe, `lkatta.exe` est produit** |

**Le portage compile.** `a5ad705` est le premier envoi où le job `windows` finit en `success` :
`cargo build --release --locked --target x86_64-pc-windows-msvc` aboutit, et `lkatta.exe`
(5 146 805 octets) est publié comme artefact `lkatta-windows-x64` du run. La CI Linux reste verte
à chaque envoi, et `mascot/mod.rs`, `mascot/x11.rs` et `Cargo.lock` n'ont pas été modifiés d'un
octet : les jumeaux ajoutent, ils ne remplacent pas.

### Ce qui reste : 31 avertissements, et ce qu'ils veulent dire

Plus aucune erreur. Le rapport de CI liste maintenant les avertissements `dead_code`, qui sont
l'information la plus utile qu'il donne : **une fonction « never used » sous Windows est une
fonctionnalité qui n'y est pas encore branchée.** Le relevé de `a5ad705` en compte 31 sur 14
fichiers, dont 7 identifiés dans le journal tronqué :

| Fonction jamais appelée sous Windows | Ce que ça veut dire | Étape |
|---|---|---|
| `toplevels::{etats, mettre_a_jour, prendre_actions, marquer_disponible}` | Le suivi des fenêtres n'est pas branché : `Ctx.avoid` et `Ctx.toit` restent vides, donc le maki ne monte pas sur les fenêtres et ne les évite pas | 2 |
| `notifs::compte` | Le compteur de notifications n'a pas encore de source sous Windows | 2 |
| `os::osstring_depuis_octets` | Le socle a la fonction, personne ne l'appelle encore sur cette cible | 2 |
| `saisie`, `chat` (4 avertissements chacun) | La bulle de discussion : `draw::chat_bubble` est portable et prêt, mais le clavier et l'enchaînement vers `brain` ne sont pas câblés | 1 |
| `win32.rs` : cast d'une fonction en entier | Corrigé dans l'envoi suivant : le rappel de fenêtre est maintenant un type `WNDPROC` déclaré, que le compilateur vérifie | — |

Le rapport n'affichait que la fin du journal, donc 7 des 31 sont identifiés ; le format publié
depuis en liste jusqu'à 80, triés par fichier avec leur message, donc le prochain run donnera la
liste complète. C'est elle qui tient lieu de liste de choses à faire pour l'étape 2.

Les jumeaux se traitent par le moyen déjà éprouvé sur `atspi`, `gnome` et `portail` : **choisi par
`cfg` dans `main.rs`, même nom de module, donc aucun site d'appel à modifier.** Le code Linux reste
inchangé, ce qui est la raison pour laquelle la CI Linux reste verte à chaque envoi.

`mascot/mod.rs` ne se traitait pas ainsi, et c'était le morceau annoncé comme difficile : il ne
s'agissait pas de doubler un module Linux, mais de couper en deux un module qui mélange l'état de
l'application et la boucle d'événements d'un seul système. **C'est fait, et la difficulté annoncée
était surestimée.** La mesure l'a tranchée avant d'écrire quoi que ce soit : `behavior.rs`,
`draw.rs`, `dessin.rs` et `especes.rs` — 5 730 des 8 896 lignes — ne contiennent aucune référence
à Wayland, X11, D-Bus ou `std::os::unix`. Ce qui était propre à Linux était donc la surcouche, pas
le personnage.

Le jumeau `src/mascot_win/mod.rs` fait 63 lignes : il redéclare les quatre sous-modules avec un
`#[path]` vers les fichiers existants (leurs `super::behavior` continuent de résoudre, puisque
leurs frères sont déclarés là aussi), puis il expose `run` avec la même signature. Rien n'est
dupliqué, et si le comportement du maki change, les deux cibles changent ensemble.

Ce qui est écrit pour de bon, c'est `src/mascot_win/win32.rs` (851 lignes) : une fenêtre en
calques `WS_EX_LAYERED` avec `UpdateLayeredWindow`, qui donne la zone cliquable gratuitement —
Windows laisse passer la souris là où l'alpha est nul, donc aucun `WM_NCHITTEST` ni région à
recalculer à chaque image. Elle est aussi `NOACTIVATE` : prendre le clavier de l'utilisateur pour
un lémurien serait une faute. Le rendu suit l'enchaînement exact de la version Linux, mêmes
constantes (470×560 pixels logiques, même calcul d'échelle, même rectangle selon la pose).

### Le backend Win32 n'a pas besoin de dépendance nouvelle

C'était la contrainte que je croyais la plus dure, et elle ne l'est pas — autant le dire ici
parce qu'elle aurait borné toute l'étape 1 à tort.

Le backend Win32 (`UpdateLayeredWindow` pour la bulle, Credential Manager pour les secrets,
`RegisterHotKey` pour le raccourci global, `CreateNamedPipeW` pour le canal) passe par des crates
comme `windows` ou `windows-sys`. Or **ces crates ne font rien d'autre que déclarer les fonctions
de l'API** : ce sont des blocs `extern "system"` avec un attribut `#[link]`. On peut écrire les
mêmes blocs soi-même, sans aucune crate :

```rust
#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterHotKey(fenetre: isize, id: i32, modificateurs: u32, touche: u32) -> i32;
}
```

Le mot `unsafe` devant `extern` n'est pas décoratif : ce crate est en édition 2024, qui l'exige
(`glibc.rs` écrit de même `#[unsafe(no_mangle)]`). Sans lui, la compilation échoue.

`kernel32`, `user32` et `advapi32` sont des bibliothèques du système, présentes sur toute
installation de Windows : l'attribut `#[link]` dit à l'éditeur de liens de les prendre, et il n'y
a rien à télécharger. **Le `Cargo.lock` ne bouge donc pas**, ce qui lève la contrainte — il ne
pouvait pas être régénéré d'ici, cargo y est absent et la CI tourne en `--locked`.

Ce que ça coûte en échange, et il faut le savoir avant de choisir :

- C'est du `unsafe` qu'on possède. Chaque signature doit être exacte : une déclaration fausse ne
  se voit pas à la compilation, elle se voit à l'exécution, et `cargo check` sous Windows ne
  vérifie que les types Rust, pas l'ABI.
- Il n'y a pas de mise à jour automatique quand Microsoft change une signature (rare, mais ça
  arrive).
- Les constantes (`WS_EX_LAYERED`, `GENERIC_WRITE`…) doivent être recopiées à la main.

Pour quelques dizaines de fonctions, c'est un coût acceptable et c'est ce que fait ce projet
partout ailleurs : `libc::prctl` et `libc::setsid` sont déjà des appels directs, et le socle
`src/os/` les a rangés derrière des fonctions sûres. Les déclarations Win32 sont allées au même
endroit, avec les mêmes règles : `src/transport_win.rs` pour `kernel32` et `advapi32`,
`src/mascot_win/win32.rs` pour `user32` et `gdi32`. Une fonction sûre par besoin, le `unsafe`
confiné dedans, et un `SAFETY:` qui dit pourquoi c'est juste.

**C'est maintenant vérifié par la CI, plus seulement plausible** : `a5ad705` compile en release
sans aucune dépendance nouvelle, `Cargo.lock` octet pour octet identique. Le pari est tenu.

Ce que la compilation ne vérifie toujours pas, et qui reste la vraie règle de prudence : une
signature fausse ne se voit qu'à l'exécution, parce que `cargo check` contrôle les types Rust et
non l'ABI. Deux exemples vécus dans ce portage, tous deux invisibles pour le compilateur :
l'ordre réel de `OpenProcess` est (accès, hérite, pid) — un PID passé comme masque d'accès n'aurait
rien planté, il aurait seulement fait échouer chaque vérification d'identité en silence ; et la
hauteur de l'en-tête DIB doit être négative pour que l'image soit dans le sens d'un écran, sinon le
maki apparaît la tête en bas. Ces deux-là se relisent, elles ne se compilent pas.
