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
| **0 — Socle** | `src/os/`, `Cargo.toml` découpé par cible, canal portable, **job CI `windows-latest`** | ✅ oui | 🟡 entamée : job CI en place, `Cargo.toml` découpé par cible |
| **1 — MVP Windows** | `mascot/win32.rs`, Named Pipes, Credential Manager, outils de base | ⚠️ compile via CI | ⬜ |
| **2 — Parité Windows** | discrétion, `controle.rs`, UI Automation, capture écran, Terminal, auto-update `.exe` | ⚠️ compile via CI | ⬜ |
| **3 — Notion natif** | `src/integrations/notion.rs`, 6 outils IA, accueil matinal, `/notion`, cache FTS5, onglet Intégrations | ✅ **oui, entièrement** | 🟢 **6 outils + CLI + onglet Intégrations livrés, CI verte** ; restent accueil matinal, `/notion`, cache FTS5 |
| **4 — Écosystème** | `lkatta-hook` + pont agents, client MCP, webhooks n8n/Make | ✅ oui (sauf le tuyau Windows) | ⬜ |

### 👉 Ma recommandation : **commencer par l'étape 3 (Notion), pas par l'étape 0**

Trois raisons simples :

- C'est du **Rust pur qui compile et se teste sur Linux aujourd'hui** → tu vois un résultat vrai dès le premier patch.
- Ça crée le squelette `src/integrations/` que le MCP et les webhooks réutiliseront.
- L'étape 0 touche `Cargo.toml` partout : sans compilateur Windows sous la main à chaque patch, c'est là qu'on casse les choses en silence.

Et **le tout premier commit, quel que soit l'ordre choisi, devrait être le job CI `windows-latest`** : il ne coûte rien et c'est lui qui rend le portage vérifiable (voir §7).

---

## 7. ⚠️ Une contrainte que tu dois connaître

**Il n'y a pas de toolchain Rust dans cet espace de travail** : `cargo` et `rustc` sont absents, et `static.rust-lang.org` est bloqué par la liste des hôtes autorisés. Je ne peux donc **pas** lancer `cargo check` ni `cargo test` ici.

**La solution : GitHub Actions devient mon compilateur.** `gh` est authentifié en tant que `bebelsrd07`, donc je peux pousser sur la branche et laisser la CI vérifier pour de vrai :

- `ubuntu-latest` → `cargo test --locked` (déjà dans `.github/workflows/ci.yml`)
- `windows-latest` → nouveau job `cargo check --target x86_64-pc-windows-msvc`, puis la fabrication du `.exe`

Ainsi chaque patch est **réellement compilé et testé**, pas seulement écrit.

*Variante si tu préfères :* je te donne les commandes, tu les lances chez toi et tu me colles la sortie.

---

## 8. Ce que je te demande avant d'écrire le premier patch

1. Par quelle étape on démarre — **3 (Notion)** comme je le recommande, **0 (socle Windows)**, ou **4 (pont agents façon coucou)** ?
2. Quels connecteurs prioritaires en plus de Notion ?
3. CI comme compilateur, ou tu compiles chez toi ?

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
