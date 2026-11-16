//! La fenêtre « Ouvrir » du bureau, là où le portail freedesktop n'existe pas.
//!
//! Sous Linux, `portail.rs` ouvre `org.freedesktop.portal.FileChooser` par D-Bus : la fenêtre
//! habituelle du système. Ici il n'y a ni D-Bus ni portail.
//!
//! Le natif Windows est `IFileOpenDialog` (COM), qui est justement la fenêtre habituelle du
//! système — l'équivalent exact, avec les mêmes raccourcis et les mêmes derniers dossiers. Il
//! s'écrit sans dépendance nouvelle, comme le reste du backend Win32 (voir `docs/PLAN.md` §10),
//! mais c'est du COM à la main : initialiser l'appartement, instancier l'objet, poser les
//! options, récupérer le tableau de `IShellItem`, libérer dans le bon ordre. C'est un morceau
//! de l'étape 1, pas une ligne.
//!
//! En attendant cette fonction rend une erreur, et c'est voulu :
//!
//! - `fichiers::choisir` essaie d'abord le portail, puis son repli (zenity, yad, kdialog), puis
//!   rend « aucune fenêtre de choix de fichier disponible (…) ». L'erreur d'ici remonte donc
//!   jusqu'à l'utilisateur **avec sa cause**, au lieu de disparaître.
//! - Rendre une liste vide aurait dit « l'utilisateur n'a rien choisi », ce qui est faux : il n'a
//!   jamais eu de fenêtre. L Katta aurait continué sans pièce jointe en croyant l'utilisateur
//!   d'accord.

use std::path::PathBuf;

/// Ouvre la fenêtre « Ouvrir » du bureau ; renvoie les fichiers choisis (vide : annulé).
pub fn ouvrir(_titre: &str) -> Result<Vec<PathBuf>, String> {
    Err(t!("la fenêtre « Ouvrir » du système n'est pas encore câblée ici").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_reports_it_is_missing_rather_than_returning_no_files() {
        // Une liste vide se lirait « l'utilisateur a annulé ». L'erreur, elle, remonte jusqu'à
        // `choisir`, qui essaie son repli puis dit clairement qu'aucune fenêtre n'existe.
        let r = ouvrir("Choisir un fichier");
        assert!(r.is_err(), "le portail a prétendu réussir là où il n'y a pas de fenêtre");
        assert!(r.unwrap_err().contains("câblée"), "l'erreur devrait dire ce qui manque");
    }
}
