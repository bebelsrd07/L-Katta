//! L'accessibilité du bureau là où AT-SPI n'existe pas.
//!
//! AT-SPI est le bus D-Bus de l'accessibilité sous Linux, et c'est par lui que L Katta lit le
//! texte de la fenêtre active. Ailleurs — Windows au premier chef — l'équivalent est UI
//! Automation, qui demande les liaisons Win32 : donc une dépendance nouvelle, et le
//! `Cargo.lock` ne peut pas être régénéré d'ici, cargo y est absent et la CI tourne en
//! `--locked`. C'est l'étape 2 du portage.
//!
//! Ce module expose exactement l'API de `atspi.rs`, pour que les appelants (chat, tools,
//! mascot, main) n'aient pas à savoir sur quel système ils tournent. `main.rs` choisit entre
//! les deux fichiers par une `cfg`, donc un seul des deux est jamais compilé.
//!
//! Deux règles suivies ici :
//!
//! - **Ce qui marche partout marche pour de vrai.** `noter_fenetre` et `derniere_fenetre` ne
//!   sont qu'un mémo en mémoire : rien ne les empêche de fonctionner ici, et les appauvrir
//!   ferait perdre du contexte à la bulle pour aucune raison.
//! - **Ce qui ne marche pas le dit.** Chaque fonction rend une erreur explicite plutôt qu'un
//!   résultat vide. Un texte de fenêtre vide ressemble à « cette fenêtre n'a pas de texte »,
//!   ce qui est faux : L Katta répondrait à côté de ce que l'utilisateur regarde.

use std::sync::Mutex;

/// Ce qui a été lu dans une fenêtre : de quoi répondre sans capture d'écran.
pub struct Lu {
    pub application: String,
    pub titre: String,
    pub texte: String,
}

/// Dernière fenêtre active connue du gestionnaire de fenêtres (titre, classe) : la bulle de
/// L Katta prend le clavier, et certaines applications cessent alors de se dire « actives ».
static DERNIERE: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));

/// Retient la fenêtre que l'utilisateur vient de quitter. Identique à la version Linux : ce
/// mémo ne dépend d'aucun bus.
pub fn noter_fenetre(titre: &str, classe: &str) {
    if classe.to_lowercase().contains("lkatta") || titre.is_empty() {
        return;
    }
    if let Ok(mut d) = DERNIERE.lock() {
        *d = (titre.to_string(), classe.to_string());
    }
}

pub fn derniere_fenetre() -> (String, String) {
    DERNIERE.lock().map(|d| d.clone()).unwrap_or_default()
}

/// L'accessibilité est-elle active dans la session ?
///
/// `false`, et non « inconnu » : sans accessibilité il n'y a rien à lire, et c'est bien ce que
/// répondent les appelants — `chat` propose alors la capture d'écran, qui elle est portable.
pub fn active() -> bool {
    false
}

/// Comme `active`, sans interroger le système : le résultat ne peut pas changer ici.
pub fn active_cache() -> bool {
    false
}

/// Activer l'accessibilité de la session.
///
/// Sous Linux c'est un réglage de session réversible (`org.a11y.Status.IsEnabled`). Il n'y a
/// pas d'équivalent à basculer ici : UI Automation est toujours disponible, c'est son accès qui
/// n'est pas encore câblé. Le message le dit plutôt que de prétendre avoir activé quelque chose.
pub fn activer(_oui: bool) -> Result<(), String> {
    Err(t!("l'accessibilité du bureau n'est pas encore câblée sur ce système").to_string())
}

/// Texte de la fenêtre active.
///
/// Rend une erreur, jamais un `Lu` vide : voir la règle en tête de module.
pub fn fenetre_active(_titre: Option<&str>) -> Result<Lu, String> {
    Err(t!("l'accessibilité du bureau n'est pas encore câblée sur ce système").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_memo_works_the_same_as_on_linux() {
        // Ce mémo ne dépend d'aucun bus : il doit marcher ici comme sous Linux, sinon la bulle
        // perd le contexte de ce que l'utilisateur regardait.
        //
        // Un seul essai pour tout le mémo : `DERNIERE` est un `static` partagé, et deux essais
        // qui y écrivent se marcheraient dessus, les essais tournant en parallèle.
        noter_fenetre("Rapport annuel — LibreOffice", "soffice");
        assert_eq!(
            derniere_fenetre(),
            ("Rapport annuel — LibreOffice".to_string(), "soffice".to_string())
        );

        // Sans ces deux filtres, L Katta se prendrait lui-même pour « la dernière fenêtre de
        // l'utilisateur » dès que sa bulle passe au premier plan, ou perdrait ce contexte sur un
        // titre vide.
        noter_fenetre("L Katta", "lkatta");
        noter_fenetre("", "gedit");
        assert_eq!(derniere_fenetre().1, "soffice", "le mémo a été écrasé par L Katta ou par un titre vide");
    }

    #[test]
    fn reading_a_window_fails_loudly_rather_than_returning_empty_text() {
        // Un `Lu` vide ressemblerait à « cette fenêtre n'a pas de texte ». L'erreur, elle, fait
        // basculer l'appelant sur la capture d'écran, qui est portable.
        assert!(fenetre_active(None).is_err());
        assert!(fenetre_active(Some("LibreOffice")).is_err());
        assert!(activer(true).is_err());
        assert!(!active());
        assert!(!active_cache());
    }
}
