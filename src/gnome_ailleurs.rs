//! Le bureau GNOME, là où GNOME Shell n'existe pas.
//!
//! L'extension de L Katta parle à GNOME Shell par D-Bus (`zbus`) : liste des fenêtres,
//! raccourci clavier global, « ne pas déranger ». Rien de tout cela n'a de sens ailleurs, et
//! `zbus` n'est plus une dépendance que pour Linux — voir `Cargo.toml`.
//!
//! Ce module expose l'API de `gnome.rs` que le reste du code emploie vraiment, pour que les
//! appelants (chat, controle, bureau) n'aient pas à savoir sur quel système ils tournent.
//! `main.rs` choisit entre les deux fichiers par une `cfg`, donc un seul des deux est jamais
//! compilé.
//!
//! Ce que chaque réponse veut dire, et pourquoi ce n'est pas une approximation :
//!
//! - `bureau_gnome` reprend la **même règle** que la version Linux : le bureau est GNOME si
//!   l'environnement détecté le dit. `environnement` est portable, donc la règle reste juste
//!   ici — elle rend simplement toujours `false`, puisque aucun compositeur Windows ne s'appelle
//!   « gnome ». Presque tous les appelants testent cette fonction d'abord : ils s'écartent donc
//!   d'eux-mêmes, sans avoir à connaître ce module.
//! - `fenetres` et `action` rendent une erreur explicite, jamais une liste vide : une liste vide
//!   dirait « aucune fenêtre ouverte », ce qui est faux.
//! - Les raccourcis globaux demandent `RegisterHotKey` côté Windows, donc les liaisons Win32 et
//!   une dépendance nouvelle que le `Cargo.lock` ne peut pas encore accueillir. C'est l'étape 2
//!   du portage.

/// Le bureau est-il GNOME (ou dérivé : Ubuntu, Pop!_OS…) ?
pub fn bureau_gnome() -> bool {
    crate::environnement::courant().compositeur == "gnome"
}

/// L'extension tourne-t-elle dans GNOME Shell ?
///
/// `false` : il n'y a pas de GNOME Shell ici. C'est la réponse que la version Linux donnerait
/// elle aussi, son objet D-Bus étant absent du bus.
pub fn active() -> bool {
    false
}

/// Fenêtres vues par GNOME Shell.
pub fn fenetres() -> Result<Vec<crate::toplevels::Fenetre>, String> {
    Err(t!("l'extension GNOME de L Katta ne sert que sous GNOME Shell").to_string())
}

/// Demander une action sur une fenêtre (activer, fermer…) à l'extension.
pub fn action(_id: u32, _a: &str) -> Result<(), String> {
    Err(t!("l'extension GNOME de L Katta ne sert que sous GNOME Shell").to_string())
}

/// Installer l'extension dans GNOME Shell.
///
/// Inatteignable en pratique : `/gnome` commence par tester `bureau_gnome`, et `chat` répond
/// « Ce bureau n'est pas GNOME » avant d'arriver ici. L'erreur est là pour que le cas reste
/// vrai même si un appelant oublie ce test.
pub fn installer() -> Result<String, String> {
    Err(t!("l'extension GNOME de L Katta ne sert que sous GNOME Shell").to_string())
}

/// Retirer l'extension. Même remarque que `installer`.
pub fn retirer() -> Result<String, String> {
    Err(t!("l'extension GNOME de L Katta ne sert que sous GNOME Shell").to_string())
}

/// Ce que l'extension aurait à faire au démarrage (se réinstaller si ses fichiers ont vieilli).
///
/// `None` : c'est exactement ce que rend la version Linux quand `bureau_gnome()` est faux, et
/// c'est toujours le cas ici.
pub fn au_demarrage() -> Option<String> {
    None
}

/// « Ne pas déranger » : les bannières de notification sont-elles coupées ?
///
/// `false`, c'est-à-dire « ne rien retenir ». Windows a bien un équivalent (concentration,
/// notifications désactivées), mais le lire demande les liaisons Win32. En attendant L Katta
/// notifie normalement : annoncer une notification que l'utilisateur ne verra pas serait pire,
/// mais retenir toutes les notifications sur un simple doute le serait aussi.
pub fn ne_pas_deranger() -> bool {
    false
}

/// Poser le raccourci clavier global de L Katta.
///
/// Ne fait rien ici. Sous Windows ce serait `RegisterHotKey` — étape 2 du portage. Les
/// appelants n'y viennent que si `bureau_gnome()` est vrai, donc ce corps n'est pas exécuté en
/// pratique ; il existe pour que l'API reste la même.
pub fn raccourci(_seq: &str) {}

/// Retirer le raccourci posé par `raccourci`.
pub fn retirer_raccourci() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_is_never_the_desktop_here() {
        // Toute la cohérence du module tient là : les appelants testent `bureau_gnome` d'abord,
        // donc ils s'écartent d'eux-mêmes des chemins GNOME. Si cette fonction rendait vrai un
        // jour, ils appelleraient `installer` ou `fenetres` et tomberaient sur des erreurs.
        assert!(!bureau_gnome());
        assert!(!active());
        assert!(!ne_pas_deranger());
        assert!(au_demarrage().is_none());
    }

    #[test]
    fn asking_for_windows_fails_rather_than_saying_there_are_none() {
        // Une liste vide se lirait « aucune fenêtre ouverte ». L'erreur, elle, fait choisir un
        // autre chemin à `controle`, qui en a plusieurs.
        assert!(fenetres().is_err());
        assert!(action(1, "activer").is_err());
        assert!(installer().is_err());
        assert!(retirer().is_err());
    }

    #[test]
    fn setting_a_global_shortcut_is_a_no_op_that_does_not_panic() {
        // Rien n'est posé, mais rien ne casse non plus : `bureau` appelle `raccourci` dans un
        // fil détaché, où une panique tuerait le fil sans que personne ne le sache.
        raccourci("<Super>K");
        retirer_raccourci();
    }
}
