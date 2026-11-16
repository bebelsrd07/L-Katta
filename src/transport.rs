//! Le transport du protocole de commande, sous Linux : un socket Unix.
//!
//! Le protocole lui-même (les lignes de texte, leur analyse) est dans `ipc.rs`, qui est
//! portable. Ici il n'y a que le tuyau : un socket Unix dans le dossier d'exécution, en droits
//! 0600, donc joignable par ce compte seul.
//!
//! Un client muet ou bavard ne bloque pas les suivants : 2 s de délai de lecture et 64 Ko au
//! plus par commande.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::Duration;

use crate::paths;

/// Écoute le socket et lance un fil pour cela. `repondre` transforme une ligne reçue en la ligne
/// à renvoyer, terminaison comprise.
pub fn ecouter(repondre: impl Fn(&str) -> String + Send + 'static) -> std::io::Result<()> {
    let path = paths::socket_file();
    std::fs::create_dir_all(path.parent().unwrap())?;
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    crate::os::restreindre_fichier(&path)?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut w = match stream.try_clone() {
                Ok(w) => w,
                Err(_) => continue,
            };
            let mut line = String::new();
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            if BufReader::new(stream.take(64 * 1024)).read_line(&mut line).is_ok() {
                let _ = w.write_all(repondre(&line).as_bytes());
            }
        }
    });
    Ok(())
}

/// Envoie une commande et attend la réponse. Erreur si le démon ne tourne pas.
pub fn envoyer(ligne: &str) -> std::io::Result<String> {
    let mut s = UnixStream::connect(paths::socket_file())?;
    s.write_all(ligne.replace('\n', " ").as_bytes())?;
    s.write_all(b"\n")?;
    let mut out = String::new();
    BufReader::new(s).read_line(&mut out)?;
    Ok(out.trim().to_string())
}

/// Le démon est-il là ? On essaie de se connecter, voilà tout.
pub fn present() -> bool {
    UnixStream::connect(paths::socket_file()).is_ok()
}
