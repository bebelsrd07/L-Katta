//! L Katta sous Windows.
//!
//! Ce module remplace `mascot/mod.rs` quand la cible n'est pas Linux — `main.rs` choisit entre
//! les deux par une `cfg`, et le nom du module reste `crate::mascot`, donc tout ce qui
//! l'appelle (`chat`, `config`, `daemon`, `ecran`, `film`, `install`, `main`) est inchangé.
//!
//! ## Ce qui est réutilisé, et c'est l'essentiel
//!
//! Les quatre sous-modules du maki sont déclarés ici tels quels, avec un `#[path]` qui pointe
//! vers les fichiers existants :
//!
//! ```text
//! behavior.rs   2047 lignes   la physique et l'humeur du maki
//! draw.rs       2608 lignes   le dessin des bulles, de l'interface, du texte
//! dessin.rs      747 lignes   le dessin du maki lui-même
//! especes.rs     328 lignes   les espèces, leurs palettes, leur démarche
//! ```
//!
//! 5 730 lignes, **aucune dépendance à Wayland, X11 ou D-Bus** — c'est vérifié, pas supposé. Ce
//! qui était propre à Linux n'était donc pas le maki mais la façon de l'afficher. Les références
//! `super::behavior`, `super::especes` et `super::dessin` de ces fichiers continuent de résoudre,
//! puisque leurs frères sont déclarés ici aussi.
//!
//! Ce qui est écrit pour de bon dans ce dossier, c'est la surcouche : la fenêtre Win32 et sa
//! boucle de messages, dans `win32.rs`.
//!
//! ## Ce que cette surcouche ne fait pas encore
//!
//! Elle est plus courte que `mascot/mod.rs`, et c'est assumé. Manquent à ce jour :
//!
//! - **les boutons d'action sous une bulle spontanée** : `actions_for` vit dans `mascot/mod.rs`
//!   et dépend de ses outils (historique, fils, courriel). La bulle de discussion, elle, est
//!   branchée entièrement — clics sur les 27 zones du tracé, clavier, presse-papiers, historique
//!   des fils, réglages et choix de fichiers ;
//! - **rien pour la fenêtre active** : elle est demandée au système par `bureau::demarrer`
//!   (`GetForegroundWindow`, sondé toutes les 400 ms), donc le maki monte sur les fenêtres, les
//!   évite, et se tait en visioconférence ou en plein écran. Ce qui manque encore est « Ne pas
//!   déranger » et le micro occupé : leurs sondes passent par `gdbus` et `pactl`, qui n'existent
//!   pas ici et rendent toujours faux — la règle de discrétion est donc incomplète, pas absente ;
//! - **plusieurs écrans** : un seul moniteur est pris en compte, donc `Ctx.exits` est fermé
//!   partout et le maki ne passe pas d'un écran à l'autre.
//!
//! Chaque absence est rappelée à l'endroit du code où elle se voit, plutôt que rassemblée ici
//! seulement.

#[path = "../mascot/behavior.rs"]
pub mod behavior;
#[path = "../mascot/dessin.rs"]
pub mod dessin;
#[path = "../mascot/draw.rs"]
pub mod draw;
#[path = "../mascot/especes.rs"]
pub mod especes;

pub mod win32;

use crate::config::Config;
use crate::ipc::Msg;

/// Lance le maki et ne revient qu'à sa fermeture.
///
/// Même signature que la version Linux, au type du canal près — et ce type est le même des deux
/// côtés à travers `crate::os::canal`, donc `daemon.rs` appelle l'un ou l'autre sans le savoir.
pub fn run(cfg: Config, rx: crate::os::canal::Recepteur<Msg>) -> Result<(), String> {
    win32::run(cfg, rx)
}
