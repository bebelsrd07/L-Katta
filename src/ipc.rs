//! Petit protocole texte entre la ligne de commande / le panneau et le démon.
//!
//! Ce module ne contient que le protocole : les messages, et l'analyse des lignes qui les
//! portent. Le tuyau, lui, est dans `crate::transport` — socket Unix sous Linux, tube nommé sous
//! Windows — choisi par une `cfg` dans `main.rs`. Les deux transports parlent en lignes de texte,
//! donc ce module n'a pas à savoir lequel est en dessous.

#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    /// Parole demandée (toujours affichée).
    Say(String),
    /// Remarque spontanée (soumise à la discrétion).
    Hint(String),
    /// Rappel échu (toujours affiché).
    Reminder(String),
    Flip,
    Wave,
    Tree,
    Teleport,
    Climb,
    /// Ouvre ou ferme la bulle de discussion.
    Chat,
    /// Bulle typée ; `true` = toujours affichée, `false` = remarque spontanée.
    Bubble(crate::conseils::Message, bool),
    /// Fenêtre active (script KWin).
    Fenetre(crate::bureau::Fenetre),
    /// « Ne pas déranger » / micro utilisé.
    Occupe { ne_pas_deranger: bool, micro: bool },
    /// Le raccourci global a été pressé : ouvre (ou ferme) la bulle, clavier prêt.
    Raccourci,
    /// Écran où l'on travaille (nom KWin, ex. « HDMI-A-1 »).
    ActiveOutput(String),
    /// Relit config.toml.
    Reload,
    /// Va sur un autre écran.
    Screen,
    /// Le bureau virtuel courant a changé (identifiant KWin).
    Desktop(String),
    /// Discrétion : Some(v) fixe, None bascule.
    Quiet(Option<bool>),
    /// Masquer le maki : Some(v) fixe, None bascule.
    Hide(Option<bool>),
    /// Capture d'écran en cours : rien n'est dessiné (le maki ne bouge pas).
    Capture(bool),
    Quit,
}

pub fn parse(line: &str) -> Option<Msg> {
    let (cmd, rest) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
    let rest = rest.trim().to_string();
    let flag = |r: &str| match r {
        "on" | "oui" | "yes" | "1" => Some(true),
        "off" | "non" | "no" | "0" => Some(false),
        _ => None,
    };
    Some(match cmd {
        "dire" | "say" if !rest.is_empty() => Msg::Say(rest),
        "remarque" if !rest.is_empty() => Msg::Hint(rest),
        "rappel" | "reminder" if !rest.is_empty() => Msg::Reminder(rest),
        "acrobatie" | "flip" => Msg::Flip,
        "salut" | "wave" => Msg::Wave,
        "arbre" | "tree" => Msg::Tree,
        "teleporter" | "teleport" => Msg::Teleport,
        "grimper" | "climb" => Msg::Climb,
        "discuter" | "panneau" | "chat" | "panel" => Msg::Chat,
        // Raccourci global d'un bureau sans KGlobalAccel (GNOME) : bulle ouverte, clavier prêt.
        "raccourci" | "shortcut" => Msg::Raccourci,
        "ecran" | "screen" => Msg::Screen,
        "recharger" | "reload" => Msg::Reload,
        // bulle <genre> <titre> | <texte>   (genres : bonjour, astuce, rappel, alerte, nouvelle, reponse, info)
        "bulle" => {
            let (kind, rest) = rest.split_once(' ').unwrap_or((rest.as_str(), ""));
            let kind = crate::conseils::Kind::parse(kind)?;
            let (title, text) = rest.split_once('|').unwrap_or(("", rest));
            Msg::Bubble(crate::conseils::Message::new(kind, title.trim(), text.trim()), true)
        }
        "astuce" => Msg::Bubble(crate::conseils::astuce(), true),
        "discret" | "quiet" => Msg::Quiet(flag(&rest)),
        "cacher" | "hide" => Msg::Hide(flag(&rest)),
        "capture" => Msg::Capture(flag(&rest).unwrap_or(false)),
        "quitter" | "quit" => Msg::Quit,
        _ => return None,
    })
}

/// Écoute les commandes (bloquant, à lancer dans un fil).
///
/// Une ligne comprise est appliquée et reçoit « ok » ; une ligne inconnue reçoit le message
/// d'erreur, que la ligne de commande affiche tel quel.
pub fn listen(on_msg: impl Fn(Msg) + Send + 'static) -> std::io::Result<()> {
    crate::transport::ecouter(move |ligne| match parse(ligne) {
        Some(m) => {
            on_msg(m);
            "ok\n".to_string()
        }
        None => format!("{}\n", t!("commande inconnue")),
    })
}

/// Envoie une commande au démon. Erreur si le démon ne tourne pas.
pub fn send(line: &str) -> std::io::Result<String> {
    crate::transport::envoyer(line)
}

pub fn daemon_running() -> bool {
    crate::transport::present()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing() {
        assert_eq!(parse("dire Bonjour toi"), Some(Msg::Say("Bonjour toi".into())));
        assert_eq!(parse("discret on"), Some(Msg::Quiet(Some(true))));
        assert_eq!(parse("cacher"), Some(Msg::Hide(None)));
        assert_eq!(parse("say Hello"), Some(Msg::Say("Hello".into())));
        assert_eq!(parse("quiet yes"), Some(Msg::Quiet(Some(true))));
        assert_eq!(parse("quit"), Some(Msg::Quit));
        assert_eq!(parse("dire"), None);
        assert_eq!(parse("n'importe"), None);
    }
}
