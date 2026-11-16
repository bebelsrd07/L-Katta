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
//! La fenêtre Windows réutilise le dessin et la physique communs. Les changements d'écran
//! explicites et le suivi de la fenêtre active sont pris en charge ; le passage animé entre
//! écrans voisins reste à implémenter et à vérifier sur un bureau Windows interactif.

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
