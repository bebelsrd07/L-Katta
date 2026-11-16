//! Le canal entre le démon et l'affichage.
//!
//! Le démon tient plusieurs fils — socket de commande, rappels, veille du bureau, mise à jour —
//! et un seul endroit où afficher. Tout passe par ce canal, et c'est le démon qui le crée, avant
//! de donner l'émetteur à ses fils et le récepteur à l'affichage.
//!
//! **Sous Linux le récepteur est une source d'événements calloop** : la boucle Wayland l'insère
//! (`insert_source`) et est réveillée dès qu'un message arrive. C'est ce qui fait que la bulle
//! réagit immédiatement sans tourner à vide.
//!
//! **Ailleurs c'est un `std::sync::mpsc`** : il n'y a pas de boucle où l'insérer, donc l'affichage
//! le sonde. Les deux ont `try_recv`, et les deux erreurs portent les mêmes noms (`Empty`,
//! `Disconnected`) : le code qui sonde est donc le même des deux côtés.
//!
//! Ce n'est pas une découverte faite en écrivant ce module — c'est vérifié dans la source de
//! calloop 0.14 (`src/sources/channel.rs`) : `channel<T>()` n'impose aucune borne à `T`,
//! `Sender<T>` est `Clone` sans borne, et `send` prend `&self`. `std::sync::mpsc` a exactement les
//! trois mêmes formes. Voilà pourquoi une simple paire d'alias suffit, et pourquoi **aucune
//! dépendance nouvelle** n'est nécessaire : le `Cargo.lock` ne bouge pas.
//!
//! Ce que ça ne règle pas, et qui reste à l'étape 1 : sous Windows, sonder un canal veut dire se
//! réveiller souvent, donc soit une temporisation courte dans la boucle de messages, soit un
//! `PostMessage` depuis l'émetteur pour réveiller `GetMessage` tout de suite. Le second est le
//! bon — il garde la réactivité sans rien faire tourner à vide.

/// Côté envoi. Clonable : chaque fil du démon en tient un.
#[cfg(target_os = "linux")]
pub type Emetteur<T> = smithay_client_toolkit::reexports::calloop::channel::Sender<T>;

#[cfg(not(target_os = "linux"))]
pub type Emetteur<T> = std::sync::mpsc::Sender<T>;

/// Côté réception. Un seul : c'est l'affichage.
#[cfg(target_os = "linux")]
pub type Recepteur<T> = smithay_client_toolkit::reexports::calloop::channel::Channel<T>;

#[cfg(not(target_os = "linux"))]
pub type Recepteur<T> = std::sync::mpsc::Receiver<T>;

/// Crée la paire. Sous Linux le récepteur est insérable dans la boucle calloop.
#[cfg(target_os = "linux")]
pub fn paire<T>() -> (Emetteur<T>, Recepteur<T>) {
    smithay_client_toolkit::reexports::calloop::channel::channel()
}

#[cfg(not(target_os = "linux"))]
pub fn paire<T>() -> (Emetteur<T>, Recepteur<T>) {
    std::sync::mpsc::channel()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un type qui ressemble à `ipc::Msg` par ses exigences : clonable, envoyable d'un fil à
    /// l'autre, et portant du texte.
    #[derive(Debug, Clone, PartialEq)]
    enum Message {
        Dire(String),
        Quitter,
    }

    #[test]
    fn a_message_sent_is_the_message_received() {
        let (tx, rx) = paire::<Message>();
        tx.send(Message::Dire("bonjour".into())).unwrap();
        tx.send(Message::Quitter).unwrap();
        // `try_recv` existe des deux côtés, avec les mêmes variantes d'erreur : c'est ce qui
        // permet à l'affichage de sonder le canal sans savoir sur quel système il tourne.
        assert_eq!(rx.try_recv().unwrap(), Message::Dire("bonjour".into()));
        assert_eq!(rx.try_recv().unwrap(), Message::Quitter);
        assert!(rx.try_recv().is_err(), "un canal vide doit le dire, pas bloquer");
    }

    #[test]
    fn each_daemon_thread_gets_its_own_clone_of_the_sender() {
        // C'est la forme réelle du démon : un émetteur cloné par fil, tous vers le même
        // récepteur. `clone` et `send(&self)` doivent exister sans borne particulière.
        let (tx, rx) = paire::<Message>();
        let mut fils = Vec::new();
        for i in 0..4 {
            let tx = tx.clone();
            fils.push(std::thread::spawn(move || tx.send(Message::Dire(format!("fil {i}"))).is_ok()));
        }
        let mut recus: Vec<String> = Vec::new();
        for f in fils {
            assert!(f.join().unwrap(), "l'envoi depuis un fil a échoué");
        }
        while let Ok(Message::Dire(s)) = rx.try_recv() {
            recus.push(s);
        }
        assert_eq!(recus.len(), 4, "un message sur quatre s'est perdu");
        // L'ordre n'est pas garanti entre les fils, et ce n'est pas un défaut : le démon ne
        // promet rien sur l'ordre des remarques spontanées.
        recus.sort();
        assert_eq!(recus, vec!["fil 0".to_string(), "fil 1".into(), "fil 2".into(), "fil 3".into()]);
    }

    #[test]
    fn a_dropped_sender_leaves_the_receiver_able_to_tell() {
        // L'affichage doit pouvoir distinguer « rien encore » de « plus personne n'enverra
        // jamais rien », sinon il attend indéfiniment un démon qui n'existe plus.
        let (tx, rx) = paire::<Message>();
        assert!(rx.try_recv().is_err());
        drop(tx);
        assert!(rx.try_recv().is_err(), "même sans émetteur, la réception ne doit pas bloquer");
    }
}
